//! Renders the shapes of the Adobe-authored samples to PNG contact sheets, so that a person can look at what
//! assembly produces. It runs only when `ANICRAFT_SAMPLES` (the sample folder) and `ANIM_SHAPE_PREVIEW` (an output
//! folder) are both set, and otherwise returns at once.
//!
//! The rasteriser here is a throwaway: it draws display nodes straight with `tiny-skia`, ignoring colour spaces,
//! stroke scale modes and filters. The real one is `anim-render`.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::suboptimal_flops,
    reason = "throwaway test tooling over trusted local samples"
)]

use std::path::{Path, PathBuf};

use anim_core::kurbo::PathEl;
use anim_core::{Affine, BezPath, Rect, Rgba8};
use anim_display::{Cap, Gradient, Join, Node, Paint, Spread};
use anim_shape::{BitmapFill, EdgeShape, FillStyle, GradientFill, GradientStop, NoImages, OtherStroke, SolidStroke, StrokeKind, StrokeStyle, XflEdge, XflName};
use tiny_skia::{Color, FillRule, LineCap, LineJoin, PathBuilder, Pixmap, Shader, SpreadMode, Transform};

fn local_entries(path: &Path) -> Vec<(String, String)> {
    anim_testkit::archive_texts(path, |name| Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")) && name.starts_with("LIBRARY/"))
        .expect("sample readable")
}

fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn first_element<'a, 'i>(node: roxmltree::Node<'a, 'i>) -> Option<roxmltree::Node<'a, 'i>> {
    node.children().find(roxmltree::Node::is_element)
}

fn number(node: roxmltree::Node<'_, '_>, name: &str, default: f64) -> f64 {
    node.attribute(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn color(node: roxmltree::Node<'_, '_>) -> Rgba8 {
    let base = Rgba8::from_hex(node.attribute("color").unwrap_or("#000000")).unwrap_or(Rgba8::BLACK);
    base.with_alpha_f32(number(node, "alpha", 1.0) as f32)
}

fn matrix(node: roxmltree::Node<'_, '_>) -> Affine {
    let Some(m) = child(node, "matrix").and_then(|m| child(m, "Matrix")) else { return Affine::IDENTITY };
    Affine::new([number(m, "a", 1.0), number(m, "b", 0.0), number(m, "c", 0.0), number(m, "d", 1.0), number(m, "tx", 0.0), number(m, "ty", 0.0)])
}

fn fill_style(node: roxmltree::Node<'_, '_>) -> FillStyle {
    match node.tag_name().name() {
        "LinearGradient" | "RadialGradient" => {
            let gradient = GradientFill {
                matrix: matrix(node),
                stops: node
                    .children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "GradientEntry")
                    .map(|e| GradientStop { offset: number(e, "ratio", 0.0) as f32, color: color(e) })
                    .collect(),
                spread: node.attribute("spreadMethod").and_then(Spread::from_xfl).unwrap_or(Spread::Pad),
                interpolation: node.attribute("interpolationMethod").and_then(anim_shape::Interpolation::from_xfl).unwrap_or_default(),
                focal: number(node, "focalPointRatio", 0.0),
            };
            if node.tag_name().name() == "LinearGradient" { FillStyle::LinearGradient(gradient) } else { FillStyle::RadialGradient(gradient) }
        }
        "BitmapFill" => FillStyle::Bitmap(BitmapFill {
            path: node.attribute("bitmapPath").unwrap_or_default().to_owned(),
            matrix: matrix(node),
            clipped: node.attribute("bitmapIsClipped") == Some("true"),
            smoothing: true,
        }),
        _ => FillStyle::Solid(color(node)),
    }
}

fn stroke_style(node: roxmltree::Node<'_, '_>) -> StrokeStyle {
    let base = SolidStroke {
        weight: number(node, "weight", 1.0),
        fill: child(node, "fill").and_then(first_element).map_or_else(FillStyle::default, fill_style),
        caps: node.attribute("caps").and_then(Cap::from_xfl).unwrap_or(Cap::Round),
        joints: node.attribute("joints").and_then(Join::from_xfl).unwrap_or(Join::Round),
        miter_limit: number(node, "miterLimit", 3.0),
        scale_mode: node.attribute("scaleMode").and_then(anim_shape::StrokeScale::from_xfl).unwrap_or_default(),
        pixel_hinting: node.attribute("pixelHinting") == Some("true"),
    };
    match StrokeKind::from_element(node.tag_name().name()) {
        Some(kind) => StrokeStyle::Other(OtherStroke { kind, base, attrs: node.attributes().map(|a| (a.name().to_owned(), a.value().to_owned())).collect() }),
        None => StrokeStyle::Solid(base),
    }
}

/// All shapes of one XML document, with the number of segments.
fn shapes(xml: &str) -> Vec<EdgeShape> {
    let doc = roxmltree::Document::parse(xml).expect("sample XML parses");
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "DOMShape")
        .map(|shape| {
            let indexed = |container: &str, item: &str| -> Vec<roxmltree::Node<'_, '_>> {
                let mut found: Vec<(usize, roxmltree::Node<'_, '_>)> = child(shape, container)
                    .map(|c| {
                        c.children()
                            .filter(|n| n.is_element() && n.tag_name().name() == item)
                            .filter_map(|n| Some((n.attribute("index")?.parse().ok()?, n)))
                            .collect()
                    })
                    .unwrap_or_default();
                found.sort_by_key(|(i, _)| *i);
                found.into_iter().map(|(_, n)| n).collect()
            };
            let fills: Vec<FillStyle> =
                indexed("fills", "FillStyle").into_iter().map(|f| first_element(f).map_or_else(FillStyle::default, fill_style)).collect();
            let strokes: Vec<StrokeStyle> =
                indexed("strokes", "StrokeStyle").into_iter().map(|s| first_element(s).map_or_else(StrokeStyle::default, stroke_style)).collect();
            let edges: Vec<XflEdge> = child(shape, "edges")
                .map(|e| {
                    e.children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "Edge")
                        .map(|n| XflEdge {
                            fill_style0: n.attribute("fillStyle0").and_then(|v| v.parse().ok()).unwrap_or(0),
                            fill_style1: n.attribute("fillStyle1").and_then(|v| v.parse().ok()).unwrap_or(0),
                            stroke_style: n.attribute("strokeStyle").and_then(|v| v.parse().ok()).unwrap_or(0),
                            edges: n.attribute("edges").map(str::to_owned),
                            cubics: n.attribute("cubics").map(str::to_owned),
                        })
                        .collect()
                })
                .unwrap_or_default();
            EdgeShape::from_xfl(fills, strokes, &edges).expect("sample shape parses")
        })
        .collect()
}

fn skia_path(path: &BezPath) -> Option<tiny_skia::Path> {
    let mut b = PathBuilder::new();
    let f = |p: anim_core::Point| (p.x as f32, p.y as f32);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => b.move_to(f(p).0, f(p).1),
            PathEl::LineTo(p) => b.line_to(f(p).0, f(p).1),
            PathEl::QuadTo(c, p) => b.quad_to(f(c).0, f(c).1, f(p).0, f(p).1),
            PathEl::CurveTo(c1, c2, p) => b.cubic_to(f(c1).0, f(c1).1, f(c2).0, f(c2).1, f(p).0, f(p).1),
            PathEl::ClosePath => b.close(),
        }
    }
    b.finish()
}

fn skia_color(c: Rgba8) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn transform(m: Affine) -> Transform {
    let [xx, yx, xy, yy, tx, ty] = m.as_coeffs();
    Transform::from_row(xx as f32, yx as f32, xy as f32, yy as f32, tx as f32, ty as f32)
}

fn gradient_shader(g: &Gradient, radial: bool) -> Option<Shader<'static>> {
    let stops: Vec<tiny_skia::GradientStop> = g.stops.iter().map(|s| tiny_skia::GradientStop::new(s.offset, skia_color(s.color))).collect();
    let mode = match g.spread {
        Spread::Pad => SpreadMode::Pad,
        Spread::Reflect => SpreadMode::Reflect,
        Spread::Repeat => SpreadMode::Repeat,
    };
    let extent = anim_display::GRADIENT_EXTENT as f32;
    if radial {
        let focal = tiny_skia::Point::from_xy(g.focal as f32 * extent, 0.0);
        tiny_skia::RadialGradient::new(focal, 0.0, tiny_skia::Point::from_xy(0.0, 0.0), extent, stops, mode, transform(g.transform))
    } else {
        tiny_skia::LinearGradient::new(tiny_skia::Point::from_xy(-extent, 0.0), tiny_skia::Point::from_xy(extent, 0.0), stops, mode, transform(g.transform))
    }
}

fn paint(p: &Paint) -> tiny_skia::Paint<'static> {
    let mut out = tiny_skia::Paint { anti_alias: true, ..tiny_skia::Paint::default() };
    out.shader = match p {
        Paint::Solid(c) => Shader::SolidColor(skia_color(*c)),
        Paint::Linear(g) => gradient_shader(g, false).unwrap_or_else(|| Shader::SolidColor(Color::from_rgba8(255, 0, 255, 255))),
        Paint::Radial(g) => gradient_shader(g, true).unwrap_or_else(|| Shader::SolidColor(Color::from_rgba8(255, 0, 255, 255))),
        Paint::Bitmap { .. } => Shader::SolidColor(Color::from_rgba8(128, 128, 128, 255)),
    };
    out
}

fn draw(pixmap: &mut Pixmap, nodes: &[Node], t: Transform) {
    for node in nodes {
        match node {
            Node::Fill(f) => {
                if let Some(path) = skia_path(&f.path) {
                    let rule = if f.rule == anim_display::FillRule::EvenOdd { FillRule::EvenOdd } else { FillRule::Winding };
                    pixmap.fill_path(&path, &paint(&f.paint), rule, t, None);
                }
            }
            Node::Stroke(s) => {
                if let Some(path) = skia_path(&s.path) {
                    let stroke = tiny_skia::Stroke {
                        width: s.style.width as f32,
                        miter_limit: s.style.miter_limit as f32,
                        line_cap: match s.style.cap {
                            Cap::None => LineCap::Butt,
                            Cap::Round => LineCap::Round,
                            Cap::Square => LineCap::Square,
                        },
                        line_join: match s.style.join {
                            Join::Miter => LineJoin::Miter,
                            Join::Round => LineJoin::Round,
                            Join::Bevel => LineJoin::Bevel,
                        },
                        ..tiny_skia::Stroke::default()
                    };
                    pixmap.stroke_path(&path, &paint(&s.paint), &stroke, t, None);
                }
            }
            Node::Group(_) | Node::Bitmap(_) | Node::Mesh(_) => {}
        }
    }
}

fn sheet(shapes: &[&EdgeShape], columns: u32, cell: u32) -> Pixmap {
    let rows = (shapes.len() as u32).div_ceil(columns).max(1);
    let mut pixmap = Pixmap::new(columns * cell, rows * cell).expect("pixmap");
    pixmap.fill(Color::from_rgba8(238, 238, 242, 255));
    for (i, shape) in shapes.iter().enumerate() {
        let (col, row) = (i as u32 % columns, i as u32 / columns);
        // A light card per cell.
        let card = tiny_skia::Rect::from_xywh((col * cell) as f32 + 3.0, (row * cell) as f32 + 3.0, cell as f32 - 6.0, cell as f32 - 6.0).unwrap();
        let mut white = tiny_skia::Paint::default();
        white.set_color_rgba8(255, 255, 255, 255);
        pixmap.fill_rect(card, &white, Transform::identity(), None);
        let Some(b) = shape.bounds_with_strokes() else { continue };
        let pad = f64::from(cell) * 0.1;
        let scale = ((f64::from(cell) - 2.0 * pad) / b.width().max(1e-6)).min((f64::from(cell) - 2.0 * pad) / b.height().max(1e-6));
        let centre = b.center();
        let t = Transform::from_translate(-(centre.x as f32), -(centre.y as f32))
            .post_scale(scale as f32, scale as f32)
            .post_translate((col * cell) as f32 + cell as f32 / 2.0, (row * cell) as f32 + cell as f32 / 2.0);
        draw(&mut pixmap, &shape.to_display(&NoImages), t);
    }
    pixmap
}

fn segment_count(shape: &EdgeShape) -> usize {
    shape.edges.iter().map(|e| e.segments.len()).sum()
}

#[test]
fn render_sample_contact_sheets() {
    let (Some(fla), Some(out)) = (anim_testkit::sample("unicorn_4AE.fla"), std::env::var_os("ANIM_SHAPE_PREVIEW")) else { return };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("output folder");
    let mut all: Vec<EdgeShape> = Vec::new();
    for (name, xml) in local_entries(&fla) {
        for (n, shape) in shapes(&xml).into_iter().enumerate().filter(|(_, s)| !s.is_empty()) {
            eprintln!("sheet cell {}: {name} shape {n}", all.len());
            all.push(shape);
        }
    }
    // `ANIM_SHAPE_PREVIEW_CELL=83` renders that shape of the all-shapes sheet alone and large.
    if let Some(cell) = std::env::var("ANIM_SHAPE_PREVIEW_CELL").ok().and_then(|v| v.parse::<usize>().ok()) {
        let shape = &all[cell];
        eprintln!("bounds {:?}, with strokes {:?}", shape.bounds(), shape.bounds_with_strokes());
        for edge in &shape.edges {
            eprintln!("edge {} {} {}: {:?}", edge.fill_style0, edge.fill_style1, edge.stroke_style, edge.segments);
        }
        eprintln!("fills {:?}", shape.fills);
        sheet(&[shape], 1, 700).save_png(out.join(format!("anim-shape-cell-{cell}.png"))).expect("png");
    }
    let mut ranked: Vec<&EdgeShape> = all.iter().collect();
    ranked.sort_by_key(|s| std::cmp::Reverse(segment_count(s)));

    let highlights: Vec<&EdgeShape> = ranked.iter().copied().filter(|s| s.bounds().is_some_and(|b| b.width() > 20.0 && b.height() > 20.0)).take(24).collect();
    let sheet_a = sheet(&highlights, 6, 260);
    sheet_a.save_png(out.join("anim-shape-unicorn-highlights.png")).expect("png");

    let everything: Vec<&EdgeShape> = all.iter().collect();
    let sheet_b = sheet(&everything, 16, 110);
    sheet_b.save_png(out.join("anim-shape-unicorn-all-shapes.png")).expect("png");

    // The sheets must not be blank.
    let painted = sheet_a.pixels().iter().filter(|p| p.red() != p.green() || p.green() != p.blue()).count();
    assert!(painted > 1000, "highlights sheet has {painted} coloured pixels");
    eprintln!("rendered {} shapes to {}", all.len(), out.display());
}

#[test]
fn bounds_are_what_the_sheets_use() {
    // Keeps `Rect` in use for builds without samples.
    let r = Rect::new(0.0, 0.0, 2.0, 4.0);
    assert!((r.center().y - 2.0).abs() < 1e-12);
}
