//! Predict / render / inspect fixtures for the exporters. Each test builds a hard synthetic scene, checks the probes written in
//! `tests/fixtures/visual/<name>.expect.md` against the direct render, re-rasterises the exported SVG in headless Chrome (skipped
//! when Chrome is missing) and checks the same probes there, and saves every picture under the visual output folder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::suboptimal_flops,
    clippy::missing_const_for_fn,
    clippy::chunks_exact_to_as_chunks
)]

use crate::common;

use anim_core::{Affine, BezPath, BlendMode, ColorTransform, Point, Rgba8, Size};
use anim_display::{
    Bitmap, Cap, DisplayList, Fill, FillRule, Filter, Gradient, GradientStop, Group, Image, ImageStore, Interpolation, Join, Mesh, Node, Paint, Spread, Stroke,
    StrokeScale, StrokeStyle,
};
use anim_export::{RasterOptions, RgbaFrame, SvgOptions, VecFrameSource, display_list_to_svg, render_frame};
use common::vis::{chrome_raster, compare, crop_zoom, diff_image, flatten, miss, px, save};
use common::{rect_path, solid_rect};

const BLUE: Rgba8 = Rgba8::opaque(30, 60, 200);
const WHITE: [u8; 3] = [255, 255, 255];

fn polygon(points: &[(f64, f64)]) -> BezPath {
    let mut p = BezPath::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        if i == 0 {
            p.move_to((x, y));
        } else {
            p.line_to((x, y));
        }
    }
    p.close_path();
    p
}

fn fill(path: BezPath, rule: FillRule, paint: Paint) -> Node {
    Node::Fill(Fill { transform: Affine::IDENTITY, path, rule, paint })
}

fn direct(list: &DisplayList, images: &ImageStore) -> RgbaFrame {
    let source = VecFrameSource::new(list.size, 24.0, vec![list.clone()], images.clone());
    render_frame(&source, 0, &RasterOptions::new()).unwrap()
}

/// Direct render and Chrome render of the SVG, both flattened on white, saved with a diff picture.
fn both(name: &str, list: &DisplayList, images: &ImageStore) -> (RgbaFrame, Option<RgbaFrame>, String) {
    let d = direct(list, images);
    save(&format!("{name}.direct"), &d);
    let svg = display_list_to_svg(list, images, &SvgOptions::default()).unwrap();
    let c = chrome_raster(name, &svg, d.width, d.height, None).map(|c| flatten(&c, WHITE));
    if let Some(c) = &c {
        save(&format!("{name}.diff"), &diff_image(&flatten(&d, WHITE), c));
    }
    (flatten(&d, WHITE), c, svg)
}

type Probe = (u32, u32, [u8; 3], u8, &'static str);

fn check(name: &str, d: &RgbaFrame, c: Option<&RgbaFrame>, probes: &[Probe]) {
    let mut misses = Vec::new();
    for &(x, y, want, tol, what) in probes {
        misses.extend(miss(d, x, y, want, tol, &format!("{name} direct: {what}")));
        if let Some(c) = c {
            misses.extend(miss(c, x, y, want, tol, &format!("{name} chrome: {what}")));
        }
    }
    assert!(misses.is_empty(), "{} probe(s) failed:\n{}", misses.len(), misses.join("\n"));
}

fn agree(name: &str, d: &RgbaFrame, c: Option<&RgbaFrame>, thr: u8, max_frac: f64, max_mean: f64) {
    if let Some(c) = c {
        let (mean, frac) = compare(d, c, thr);
        eprintln!("{name}: direct vs chrome mean abs err {mean:.3}, fraction over {thr}: {frac:.4}");
        assert!(frac <= max_frac && mean <= max_mean, "{name}: direct and Chrome disagree (mean {mean:.3}, over-{thr} fraction {frac:.4})");
    }
}

// ---------------------------------------------------------------------------------------------------------------------

fn fillrules() -> (DisplayList, ImageStore) {
    let mut list = DisplayList::new(Size::new(240.0, 160.0), Some(Rgba8::WHITE));
    let star = |cx: f64, cy: f64| {
        let v: Vec<(f64, f64)> = (0..5).map(|k| (-90.0_f64 + 72.0 * f64::from(k)).to_radians()).map(|a| (cx + 30.0 * a.cos(), cy + 30.0 * a.sin())).collect();
        polygon(&[v[0], v[2], v[4], v[1], v[3]])
    };
    list.nodes.push(fill(star(40.0, 40.0), FillRule::EvenOdd, Paint::Solid(BLUE)));
    list.nodes.push(fill(star(120.0, 40.0), FillRule::NonZero, Paint::Solid(BLUE)));
    list.nodes.push(fill(polygon(&[(170.0, 10.0), (230.0, 70.0), (230.0, 10.0), (170.0, 70.0)]), FillRule::NonZero, Paint::Solid(BLUE)));
    let squares = |x0: f64, rule: FillRule, opposite: bool| {
        let mut p = polygon(&[(x0 + 10.0, 90.0), (x0 + 70.0, 90.0), (x0 + 70.0, 150.0), (x0 + 10.0, 150.0)]);
        let inner = if opposite {
            polygon(&[(x0 + 25.0, 105.0), (x0 + 25.0, 135.0), (x0 + 55.0, 135.0), (x0 + 55.0, 105.0)])
        } else {
            polygon(&[(x0 + 25.0, 105.0), (x0 + 55.0, 105.0), (x0 + 55.0, 135.0), (x0 + 25.0, 135.0)])
        };
        p.extend(inner.elements().iter().copied());
        fill(p, rule, Paint::Solid(BLUE))
    };
    list.nodes.push(squares(0.0, FillRule::EvenOdd, false));
    list.nodes.push(squares(80.0, FillRule::NonZero, false));
    list.nodes.push(squares(160.0, FillRule::NonZero, true));
    (list, ImageStore::new())
}

#[test]
fn svg_fillrules() {
    let (list, images) = fillrules();
    let (d, c, svg) = both("svg-fillrules", &list, &images);
    let b = [30, 60, 200];
    let probes: &[Probe] = &[
        (40, 40, WHITE, 2, "even-odd pentagram centre is a hole"),
        (40, 34, WHITE, 2, "even-odd pentagram, inside the central pentagon"),
        (40, 16, b, 2, "even-odd pentagram arm"),
        (53, 22, WHITE, 2, "even-odd pentagram valley"),
        (120, 40, b, 2, "non-zero pentagram centre filled"),
        (120, 34, b, 2, "non-zero pentagram inner"),
        (120, 16, b, 2, "non-zero pentagram arm"),
        (133, 22, WHITE, 2, "non-zero pentagram valley"),
        (180, 40, b, 2, "bow-tie left lobe"),
        (220, 40, b, 2, "bow-tie right lobe"),
        (200, 15, WHITE, 2, "bow-tie top gap"),
        (200, 65, WHITE, 2, "bow-tie bottom gap"),
        (40, 120, WHITE, 2, "even-odd square hole"),
        (15, 120, b, 2, "even-odd square ring"),
        (120, 120, b, 2, "non-zero same winding is filled"),
        (200, 120, WHITE, 2, "non-zero opposite winding is a hole"),
        (175, 120, b, 2, "opposite winding square ring"),
        (5, 85, WHITE, 2, "outside"),
    ];
    check("fillrules", &d, c.as_ref(), probes);
    assert_eq!(svg.matches("fill-rule=\"evenodd\"").count(), 2);
    agree("fillrules", &d, c.as_ref(), 40, 0.015, 1.0);
    if let Some(c) = &c {
        save("svg-fillrules.chrome-zoom", &crop_zoom(c, 20, 20, 40, 40, 8));
    }
}

// ---------------------------------------------------------------------------------------------------------------------

fn grad(cx: f64, cy: f64, s: f64, stops: &[(f32, Rgba8)], spread: Spread, interpolation: Interpolation, focal: f64) -> Gradient {
    Gradient {
        transform: Affine::translate((cx, cy)) * Affine::scale(s),
        stops: stops.iter().map(|&(offset, color)| GradientStop { offset, color }).collect(),
        spread,
        interpolation,
        focal,
    }
}

fn gradients() -> (DisplayList, ImageStore) {
    let mut list = DisplayList::new(Size::new(240.0, 160.0), Some(Rgba8::WHITE));
    let red = Rgba8::opaque(255, 0, 0);
    let green = Rgba8::opaque(0, 255, 0);
    let blue = Rgba8::opaque(0, 0, 255);
    let rgb = Interpolation::Rgb;
    list.nodes.push(fill(
        rect_path(0.0, 0.0, 120.0, 30.0),
        FillRule::NonZero,
        Paint::Linear(grad(60.0, 15.0, 0.05, &[(0.0, red), (0.5, green), (0.5, blue), (1.0, Rgba8::WHITE)], Spread::Pad, rgb, 0.0)),
    ));
    list.nodes.push(fill(
        rect_path(0.0, 35.0, 120.0, 30.0),
        FillRule::NonZero,
        Paint::Linear(grad(60.0, 50.0, 0.01, &[(0.0, red), (1.0, blue)], Spread::Reflect, rgb, 0.0)),
    ));
    list.nodes.push(fill(
        rect_path(0.0, 70.0, 120.0, 30.0),
        FillRule::NonZero,
        Paint::Linear(grad(60.0, 85.0, 0.01, &[(0.0, red), (1.0, blue)], Spread::Repeat, rgb, 0.0)),
    ));
    list.nodes.push(fill(
        rect_path(140.0, 10.0, 80.0, 80.0),
        FillRule::NonZero,
        Paint::Radial(grad(180.0, 50.0, 0.05, &[(0.0, Rgba8::WHITE), (1.0, blue)], Spread::Pad, rgb, 0.5)),
    ));
    list.nodes.push(fill(
        rect_path(0.0, 105.0, 120.0, 30.0),
        FillRule::NonZero,
        Paint::Linear(grad(60.0, 120.0, 0.05, &[(0.0, red), (1.0, Rgba8::new(0, 0, 255, 0))], Spread::Pad, rgb, 0.0)),
    ));
    list.nodes.push(fill(
        rect_path(130.0, 105.0, 100.0, 30.0),
        FillRule::NonZero,
        Paint::Linear(grad(180.0, 120.0, 0.05, &[(0.0, red), (1.0, green)], Spread::Pad, Interpolation::LinearRgb, 0.0)),
    ));
    (list, ImageStore::new())
}

#[test]
fn svg_gradients() {
    let (list, images) = gradients();
    let (d, c, _svg) = both("svg-gradients", &list, &images);
    let probes: &[Probe] = &[
        (10, 15, [255, 0, 0], 4, "A pad start"),
        (110, 15, [255, 255, 255], 4, "A pad end"),
        (40, 15, [121, 134, 0], 8, "A first segment"),
        (58, 15, [9, 246, 0], 10, "A just before hard stop"),
        (62, 15, [10, 10, 255], 14, "A just after hard stop"),
        (52, 50, [255, 0, 0], 12, "B reflect start"),
        (76, 50, [129, 0, 126], 8, "B reflect mirrored"),
        (60, 50, [127, 0, 128], 8, "B middle"),
        (84, 50, [250, 0, 5], 14, "B near second reflect start"),
        (76, 85, [126, 0, 129], 8, "C repeat"),
        (84, 85, [5, 0, 250], 14, "C repeat near end of period"),
        (180, 50, [172, 172, 255], 10, "D focal gradient at centre"),
        (140, 50, [5, 5, 255], 14, "D near rim"),
        (200, 50, [245, 245, 255], 12, "D at the focus"),
        (60, 120, [191, 128, 191], 8, "E straight alpha interpolation"),
        (15, 120, [255, 0, 0], 4, "E pad start"),
        (105, 120, [255, 255, 255], 4, "E pad end is transparent"),
        (180, 120, [188, 188, 0], 8, "F linearRGB midpoint"),
    ];
    check("gradients", &d, c.as_ref(), probes);
    agree("gradients", &d, c.as_ref(), 24, 0.01, 1.0);
}

// ---------------------------------------------------------------------------------------------------------------------

fn stroke_node(points: &[(f64, f64)], style: StrokeStyle) -> Node {
    let mut p = BezPath::new();
    p.move_to(points[0]);
    for &q in &points[1..] {
        p.line_to(q);
    }
    Node::Stroke(Stroke { transform: Affine::IDENTITY, path: p, style, paint: Paint::Solid(Rgba8::opaque(20, 20, 20)) })
}

fn strokes() -> (DisplayList, ImageStore) {
    let mut list = DisplayList::new(Size::new(200.0, 120.0), Some(Rgba8::WHITE));
    let miter = |limit: f64| StrokeStyle { width: 6.0, cap: Cap::None, join: Join::Miter, miter_limit: limit, scale: StrokeScale::Normal, dashes: vec![] };
    list.nodes.push(stroke_node(&[(20.0, 100.0), (40.0, 20.0), (60.0, 100.0)], miter(10.0)));
    list.nodes.push(stroke_node(&[(80.0, 100.0), (100.0, 20.0), (120.0, 100.0)], miter(3.0)));
    let dashed = |cap| StrokeStyle { width: 4.0, cap, join: Join::Miter, miter_limit: 4.0, scale: StrokeScale::Normal, dashes: vec![10.0, 5.0] };
    list.nodes.push(stroke_node(&[(10.0, 110.0), (190.0, 110.0)], dashed(Cap::None)));
    list.nodes.push(stroke_node(&[(10.0, 116.0), (190.0, 116.0)], dashed(Cap::Square)));
    let line = |y: f64, scale| {
        stroke_node(&[(30.0, y), (45.0, y)], StrokeStyle { width: 2.0, cap: Cap::None, join: Join::Miter, miter_limit: 4.0, scale, dashes: vec![] })
    };
    list.nodes.push(Node::Group(Group {
        transform: Affine::scale(4.0),
        children: vec![line(5.0, StrokeScale::None), line(10.0, StrokeScale::Normal)],
        ..Group::default()
    }));
    (list, ImageStore::new())
}

#[test]
fn svg_strokes() {
    let (list, images) = strokes();
    let (d, c, _svg) = both("svg-strokes", &list, &images);
    let ink = [20, 20, 20];
    let probes: &[Probe] = &[
        (40, 10, [86, 86, 86], 16, "miter tip, 72 percent covered"),
        (40, 6, WHITE, 40, "above the miter tip"),
        (40, 22, ink, 20, "near apex"),
        (30, 60, ink, 20, "left leg"),
        (100, 10, WHITE, 40, "bevel cuts the tip"),
        (100, 19, [83, 83, 83], 16, "bevel flat top at y=19.27, 73 percent covered"),
        (15, 110, ink, 20, "dash on"),
        (22, 110, WHITE, 20, "dash gap"),
        (27, 110, ink, 20, "second dash"),
        (37, 110, WHITE, 20, "second gap"),
        (42, 110, ink, 20, "third dash"),
        (15, 106, WHITE, 20, "above dashed line"),
        (15, 112, WHITE, 20, "below dashed line"),
        (9, 116, ink, 20, "square cap start"),
        (21, 116, ink, 20, "square cap extends"),
        (22, 116, WHITE, 20, "one pixel gap between square-capped dashes"),
        (24, 116, ink, 20, "next square-capped dash"),
        (7, 116, WHITE, 20, "before the first square cap"),
        (150, 19, ink, 20, "non-scaling stroke row 19"),
        (150, 20, ink, 20, "non-scaling stroke row 20"),
        (150, 18, WHITE, 20, "non-scaling stroke above"),
        (150, 21, WHITE, 20, "non-scaling stroke below"),
        (150, 36, ink, 20, "scaling stroke top row"),
        (150, 43, ink, 20, "scaling stroke bottom row"),
        (150, 35, WHITE, 20, "scaling stroke above"),
        (150, 44, WHITE, 20, "scaling stroke below"),
    ];
    check("strokes", &d, c.as_ref(), probes);
    agree("strokes", &d, c.as_ref(), 60, 0.01, 0.6);
}

// ---------------------------------------------------------------------------------------------------------------------

fn groups() -> (DisplayList, ImageStore) {
    let bg = Rgba8::opaque(200, 230, 250);
    let mut list = DisplayList::new(Size::new(240.0, 120.0), Some(bg));
    let red = Rgba8::opaque(255, 0, 0);
    let blue = Rgba8::opaque(0, 0, 255);
    list.nodes.push(Node::Group(Group {
        color: ColorTransform::alpha(0.5),
        children: vec![solid_rect(10.0, 10.0, 40.0, 40.0, red), solid_rect(30.0, 30.0, 40.0, 40.0, blue)],
        ..Group::default()
    }));
    list.nodes.push(Node::Group(Group {
        mask: Some(vec![solid_rect(90.0, 10.0, 40.0, 40.0, Rgba8::new(255, 255, 255, 128))]),
        children: vec![solid_rect(80.0, 0.0, 60.0, 60.0, Rgba8::opaque(0, 160, 0))],
        ..Group::default()
    }));
    list.nodes.push(Node::Group(Group {
        blend: BlendMode::Multiply,
        children: vec![solid_rect(150.0, 10.0, 50.0, 40.0, Rgba8::opaque(255, 128, 0))],
        ..Group::default()
    }));
    list.nodes.push(Node::Group(Group {
        color: ColorTransform::tint(Rgba8::opaque(255, 0, 255), 0.3),
        children: vec![solid_rect(10.0, 80.0, 40.0, 30.0, Rgba8::opaque(0, 255, 0))],
        ..Group::default()
    }));
    list.nodes.push(Node::Group(Group {
        transform: Affine::translate((120.0, 95.0)) * Affine::rotate(30.0_f64.to_radians()) * Affine::translate((-120.0, -95.0)),
        children: vec![solid_rect(100.0, 85.0, 40.0, 20.0, Rgba8::opaque(120, 0, 0))],
        ..Group::default()
    }));
    list.nodes.push(Node::Group(Group {
        filters: vec![Filter::Blur { x: 3.0, y: 3.0, quality: 3 }],
        children: vec![solid_rect(190.0, 80.0, 40.0, 30.0, Rgba8::BLACK)],
        ..Group::default()
    }));
    (list, ImageStore::new())
}

#[test]
fn svg_groups() {
    let (list, images) = groups();
    let (d, c, _svg) = both("svg-groups", &list, &images);
    let bg = [200, 230, 250];
    let probes: &[Probe] = &[
        (5, 5, bg, 2, "backdrop"),
        (15, 15, [228, 115, 125], 4, "A red only at 50 percent"),
        (40, 40, [100, 115, 252], 4, "A overlap shows blue only (group alpha, not per child)"),
        (60, 60, [100, 115, 252], 4, "A blue only"),
        (110, 30, [100, 195, 125], 4, "B mask alpha 128 is a fractional coverage: 50 percent green over the backdrop"),
        (85, 30, bg, 2, "B outside mask"),
        (110, 5, bg, 2, "B above mask"),
        (170, 30, [200, 115, 0], 4, "C multiply"),
        (30, 95, [77, 179, 77], 5, "D tint"),
        (120, 95, [120, 0, 0], 4, "E rotated centre"),
        (100, 85, bg, 2, "E first pixel of the unrotated rect is outside"),
        (108, 78, [120, 0, 0], 10, "E rotated corner is inside"),
        (210, 95, [0, 0, 0], 8, "F blur centre"),
        (170, 95, bg, 5, "F blur far outside"),
        // Flash blur 3 with quality 3 is three box passes of width 3: variance 2, sigma 1.41. Coverage at the pixel centres
        // 0.5 px outside / 0.5 px inside / 2.5 px outside the edge is 0.36 / 0.64 / 0.04.
        (189, 95, [128, 147, 160], 16, "F blur 0.5 px outside the edge"),
        (190, 95, [72, 83, 90], 16, "F blur 0.5 px inside the edge"),
        (187, 95, [192, 221, 240], 12, "F blur 2.5 px outside the edge"),
    ];
    check("groups", &d, c.as_ref(), probes);
    agree("groups", &d, c.as_ref(), 40, 0.02, 2.0);
}

// ---------------------------------------------------------------------------------------------------------------------

fn bitmaps() -> (DisplayList, ImageStore) {
    let mut images = ImageStore::new();
    let id = images.insert(Image::new(2, 2, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 128, 128]).unwrap());
    let mut list = DisplayList::new(Size::new(120.0, 60.0), Some(Rgba8::opaque(30, 30, 30)));
    list.nodes.push(Node::Bitmap(Bitmap { transform: Affine::translate((5.0, 5.0)) * Affine::scale(10.0), image: id, smoothing: false }));
    list.nodes.push(Node::Bitmap(Bitmap { transform: Affine::new([-10.0, 0.0, 0.0, 10.0, 85.0, 5.0]), image: id, smoothing: false }));
    list.nodes.push(Node::Bitmap(Bitmap {
        transform: Affine::translate((110.0, 5.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale(5.0),
        image: id,
        smoothing: false,
    }));
    list.nodes.push(fill(
        rect_path(5.0, 35.0, 40.0, 20.0),
        FillRule::NonZero,
        Paint::Bitmap { image: id, transform: Affine::translate((5.0, 35.0)) * Affine::scale(10.0), repeat: true, smoothing: false },
    ));
    (list, images)
}

#[test]
fn svg_bitmaps() {
    let (list, images) = bitmaps();
    let (d, c, _svg) = both("svg-bitmaps", &list, &images);
    let (r, g, b, h) = ([255, 0, 0], [0, 255, 0], [0, 0, 255], [143, 143, 143]);
    let probes: &[Probe] = &[
        (10, 10, r, 3, "1 red"),
        (20, 10, g, 3, "1 green"),
        (10, 20, b, 3, "1 blue"),
        (20, 20, h, 4, "1 half-transparent white over dark"),
        (14, 10, r, 3, "1 last red column"),
        (15, 10, g, 3, "1 first green column"),
        (80, 10, r, 3, "2 mirrored red"),
        (70, 10, g, 3, "2 mirrored green"),
        (80, 20, b, 3, "2 mirrored blue"),
        (70, 20, h, 4, "2 mirrored half white"),
        (107, 7, r, 3, "3 rotated red"),
        (107, 12, g, 3, "3 rotated green"),
        (102, 7, b, 3, "3 rotated blue"),
        (102, 12, h, 4, "3 rotated half white"),
        (10, 40, r, 3, "4 pattern red"),
        (30, 40, r, 3, "4 pattern red tile 2"),
        (20, 40, g, 3, "4 pattern green"),
        (40, 40, g, 3, "4 pattern green tile 2"),
        (10, 50, b, 3, "4 pattern blue"),
        (30, 50, b, 3, "4 pattern blue tile 2"),
        (20, 50, h, 4, "4 pattern half white"),
        (40, 50, h, 4, "4 pattern half white tile 2"),
    ];
    check("bitmaps", &d, c.as_ref(), probes);
    agree("bitmaps", &d, c.as_ref(), 8, 0.0, 0.5);
}

// ---------------------------------------------------------------------------------------------------------------------

fn mesh() -> (DisplayList, ImageStore) {
    let mut list = DisplayList::new(Size::new(100.0, 60.0), Some(Rgba8::WHITE));
    let p = |x: f64, y: f64| Point::new(x, y);
    list.nodes.push(Node::Mesh(Mesh {
        transform: Affine::IDENTITY,
        content: vec![solid_rect(0.0, 0.0, 20.0, 40.0, Rgba8::opaque(200, 40, 40)), solid_rect(20.0, 0.0, 20.0, 40.0, Rgba8::opaque(40, 40, 200))],
        source: vec![p(0.0, 0.0), p(40.0, 0.0), p(0.0, 40.0), p(40.0, 40.0)],
        target: vec![p(10.0, 10.0), p(70.0, 15.0), p(5.0, 50.0), p(80.0, 55.0)],
        triangles: vec![[0, 1, 2], [1, 3, 2]],
    }));
    (list, ImageStore::new())
}

#[test]
fn svg_mesh() {
    let (list, images) = mesh();
    let (d, c, _svg) = both("svg-mesh", &list, &images);
    let (red, blue) = ([200, 40, 40], [40, 40, 200]);
    let probes: &[Probe] = &[
        (23, 21, red, 4, "source (10,10) red"),
        (54, 18, blue, 4, "source (30,5) blue"),
        (58, 43, blue, 4, "source (30,30) blue"),
        (2, 2, WHITE, 2, "outside"),
        (95, 5, WHITE, 2, "outside right"),
        (50, 58, WHITE, 2, "outside bottom"),
    ];
    check("mesh", &d, c.as_ref(), probes);
    for (name, frame) in [("direct", Some(&d)), ("chrome", c.as_ref())] {
        let Some(frame) = frame else { continue };
        for (x, y) in [(37, 32), (30, 36), (50, 25), (60, 20)] {
            let p = px(frame, x, y);
            assert!(p[0].min(p[1]).min(p[2]) <= 83, "{name}: seam on the shared mesh diagonal at ({x},{y}): {p:?}");
        }
    }
}
