//! SVG import through `usvg`.
//!
//! `usvg` resolves styles, `use` elements, units and text, so what arrives here is a tree of groups, paths and
//! images with absolute transforms. Groups keep their ids as names; paths and images become leaf nodes.

use anim_core::kurbo::Shape as _;
use anim_core::{Affine, BezPath, BlendMode, Point, Rgba8, Size, num};
use anim_display::{Cap, Fill, FillRule, Gradient, GradientStop, Interpolation, Join, Node, Paint, Spread, Stroke, StrokeScale, StrokeStyle};
use usvg::tiny_skia_path::{PathSegment, Transform};

use crate::raster::resample;
use crate::tree::{ArtMask, ArtNode, ArtworkTree};
use crate::{ArtworkError, Limits, check_input};

/// Imports an SVG or SVGZ document.
///
/// Group ids become node names (an element without an id has an empty name). Fills, strokes, linear and radial
/// gradients, opacity, `mix-blend-mode`, clip paths and embedded raster images are carried over. Masks, filters,
/// patterns and nested SVG images are not; each leaves a warning.
///
/// # Errors
/// [`ArtworkError::Svg`] if the document does not parse, [`ArtworkError::Limit`] if it has too many nodes.
pub fn import_svg(bytes: &[u8], limits: &Limits) -> Result<ArtworkTree, ArtworkError> {
    check_input(bytes, limits)?;
    let mut options = usvg::Options::default();
    // System fonts are slow to enumerate; only documents with text need them.
    if bytes.windows(5).any(|w| w == b"<text") || bytes.starts_with(&[0x1F, 0x8B]) {
        options.fontdb_mut().load_system_fonts();
    }
    let tree = usvg::Tree::from_data(bytes, &options).map_err(|e| ArtworkError::Svg(e.to_string()))?;
    let size = Size::new(f64::from(tree.size().width()), f64::from(tree.size().height()));
    let mut ctx = Ctx { limits, warnings: Vec::new(), nodes: 0, pixels: 0, noted: Vec::new() };
    let children = ctx.children(tree.root(), 0)?;
    Ok(ArtworkTree { size, root: ArtNode::group("root", children), warnings: ctx.warnings })
}

struct Ctx<'a> {
    limits: &'a Limits,
    warnings: Vec<String>,
    nodes: usize,
    pixels: u64,
    /// Warnings already given once.
    noted: Vec<&'static str>,
}

impl Ctx<'_> {
    fn note(&mut self, what: &'static str) {
        if !self.noted.contains(&what) {
            self.noted.push(what);
            self.warnings.push(what.to_owned());
        }
    }

    fn children(&mut self, group: &usvg::Group, depth: usize) -> Result<Vec<ArtNode>, ArtworkError> {
        let mut out = Vec::new();
        if depth > self.limits.max_depth {
            self.note("groups nested deeper than the limit were left out");
            return Ok(out);
        }
        for child in group.children() {
            self.nodes = self.nodes.saturating_add(1);
            if self.nodes > self.limits.max_nodes {
                return Err(ArtworkError::Limit("nodes"));
            }
            match child {
                usvg::Node::Group(sub) => out.push(self.group(sub, depth)?),
                usvg::Node::Path(path) => out.extend(self.path(path)),
                usvg::Node::Image(image) => out.extend(self.image(image)),
                usvg::Node::Text(text) => {
                    let kids = self.children(text.flattened(), depth.saturating_add(1))?;
                    out.push(ArtNode::group(text.id(), kids));
                }
            }
        }
        Ok(out)
    }

    fn group(&mut self, group: &usvg::Group, depth: usize) -> Result<ArtNode, ArtworkError> {
        let kids = self.children(group, depth.saturating_add(1))?;
        let mut node = ArtNode::group(group.id(), kids);
        node.opacity = group.opacity().get();
        node.blend = map_blend(group.blend_mode());
        if let Some(clip) = group.clip_path() {
            let mut nodes = Vec::new();
            clip_nodes(clip.root(), &mut nodes, 0);
            node.mask = Some(ArtMask::Vector { nodes });
            if clip.clip_path().is_some() {
                self.note("clip paths that are themselves clipped keep only the outer clip");
            }
        }
        if group.mask().is_some() {
            self.note("masks are not carried over");
        }
        if !group.filters().is_empty() {
            self.note("filters are not carried over");
        }
        Ok(node)
    }

    fn path(&mut self, path: &usvg::Path) -> Option<ArtNode> {
        let abs = affine(path.abs_transform());
        let bez = bez_path(path.data());
        let bounds = (abs * bez.clone()).bounding_box();
        let mut painted = Vec::new();
        let fill = path.fill().and_then(|f| {
            let paint = self.paint(f.paint(), f.opacity().get())?;
            let rule = match f.rule() {
                usvg::FillRule::NonZero => FillRule::NonZero,
                usvg::FillRule::EvenOdd => FillRule::EvenOdd,
            };
            Some(Node::Fill(Fill { transform: abs, path: bez.clone(), rule, paint }))
        });
        let stroke = path.stroke().and_then(|s| {
            let paint = self.paint(s.paint(), s.opacity().get())?;
            let style = StrokeStyle {
                width: f64::from(s.width().get()),
                cap: match s.linecap() {
                    usvg::LineCap::Butt => Cap::None,
                    usvg::LineCap::Round => Cap::Round,
                    usvg::LineCap::Square => Cap::Square,
                },
                join: match s.linejoin() {
                    usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => Join::Miter,
                    usvg::LineJoin::Round => Join::Round,
                    usvg::LineJoin::Bevel => Join::Bevel,
                },
                miter_limit: f64::from(s.miterlimit().get()),
                scale: StrokeScale::Normal,
                dashes: s.dasharray().map(|d| d.iter().map(|v| f64::from(*v)).collect()).unwrap_or_default(),
            };
            Some(Node::Stroke(Stroke { transform: abs, path: bez.clone(), style, paint }))
        });
        match path.paint_order() {
            usvg::PaintOrder::FillAndStroke => painted.extend(fill.into_iter().chain(stroke)),
            usvg::PaintOrder::StrokeAndFill => painted.extend(stroke.into_iter().chain(fill)),
        }
        if painted.is_empty() {
            return None;
        }
        let mut node = ArtNode::vector(path.id(), painted, bounds);
        node.visible = path.is_visible();
        Some(node)
    }

    fn paint(&mut self, paint: &usvg::Paint, opacity: f32) -> Option<Paint> {
        match paint {
            usvg::Paint::Color(c) => Some(Paint::Solid(Rgba8::new(c.red, c.green, c.blue, num::unit_f32_to_u8(opacity)))),
            usvg::Paint::LinearGradient(g) => {
                let (p0, p1) = (Point::new(f64::from(g.x1()), f64::from(g.y1())), Point::new(f64::from(g.x2()), f64::from(g.y2())));
                let to_space = affine(g.transform()) * linear_frame(p0, p1);
                Some(Paint::Linear(gradient(g.stops(), g.spread_method(), to_space, 0.0, opacity)))
            }
            usvg::Paint::RadialGradient(g) => {
                let (c, r) = (Point::new(f64::from(g.cx()), f64::from(g.cy())), f64::from(g.r().get()));
                let to_space = affine(g.transform()) * Affine::translate(c.to_vec2()) * Affine::scale(r / anim_display::GRADIENT_EXTENT);
                let focal = if r > 0.0 { ((f64::from(g.fx()) - c.x) / r).clamp(-1.0, 1.0) } else { 0.0 };
                Some(Paint::Radial(gradient(g.stops(), g.spread_method(), to_space, focal, opacity)))
            }
            usvg::Paint::Pattern(_) => {
                self.note("pattern paints are not carried over");
                None
            }
        }
    }

    fn image(&mut self, image: &usvg::Image) -> Option<ArtNode> {
        let data = match image.kind() {
            usvg::ImageKind::JPEG(d) | usvg::ImageKind::PNG(d) | usvg::ImageKind::GIF(d) | usvg::ImageKind::WEBP(d) => d,
            usvg::ImageKind::SVG(_) => {
                self.note("nested SVG images are not carried over");
                return None;
            }
        };
        let decoded = image::load_from_memory(data).ok()?.to_rgba8();
        let (w, h) = decoded.dimensions();
        let budget = self.limits.max_layer_pixels.min(self.limits.max_total_pixels.saturating_sub(self.pixels));
        if u64::from(w).saturating_mul(u64::from(h)) > budget {
            self.note("images over the pixel limit were left out");
            return None;
        }
        let (resampled, offset) = resample(decoded.as_raw(), w, h, affine(image.abs_transform()), budget)?;
        self.pixels = self.pixels.saturating_add(u64::from(resampled.width).saturating_mul(u64::from(resampled.height)));
        let mut node = ArtNode::raster(image.id(), resampled, offset);
        node.visible = image.is_visible();
        Some(node)
    }
}

/// The frame mapping gradient space (-819.2 ..= 819.2 on x) onto the segment `p0` -> `p1`.
fn linear_frame(p0: Point, p1: Point) -> Affine {
    let mid = p0.midpoint(p1);
    let u = (p1 - p0) / (2.0 * anim_display::GRADIENT_EXTENT);
    Affine::new([u.x, u.y, -u.y, u.x, mid.x, mid.y])
}

fn gradient(stops: &[usvg::Stop], spread: usvg::SpreadMethod, transform: Affine, focal: f64, opacity: f32) -> Gradient {
    Gradient {
        transform,
        stops: stops
            .iter()
            .map(|s| {
                let c = s.color();
                GradientStop { offset: s.offset().get(), color: Rgba8::new(c.red, c.green, c.blue, num::unit_f32_to_u8(s.opacity().get() * opacity)) }
            })
            .collect(),
        spread: match spread {
            usvg::SpreadMethod::Pad => Spread::Pad,
            usvg::SpreadMethod::Reflect => Spread::Reflect,
            usvg::SpreadMethod::Repeat => Spread::Repeat,
        },
        interpolation: Interpolation::Rgb,
        focal,
    }
}

/// Every path of a clip path's content as a black fill (coverage is all that matters).
fn clip_nodes(group: &usvg::Group, out: &mut Vec<Node>, depth: usize) {
    if depth > 64 {
        return;
    }
    for child in group.children() {
        match child {
            usvg::Node::Group(sub) => clip_nodes(sub, out, depth.saturating_add(1)),
            usvg::Node::Path(p) => {
                let rule = match p.fill().map(usvg::Fill::rule) {
                    Some(usvg::FillRule::EvenOdd) => FillRule::EvenOdd,
                    _ => FillRule::NonZero,
                };
                out.push(Node::Fill(Fill { transform: affine(p.abs_transform()), path: bez_path(p.data()), rule, paint: Paint::Solid(Rgba8::BLACK) }));
            }
            usvg::Node::Text(t) => clip_nodes(t.flattened(), out, depth.saturating_add(1)),
            usvg::Node::Image(_) => {}
        }
    }
}

fn affine(t: Transform) -> Affine {
    Affine::new([f64::from(t.sx), f64::from(t.ky), f64::from(t.kx), f64::from(t.sy), f64::from(t.tx), f64::from(t.ty)])
}

fn bez_path(path: &usvg::tiny_skia_path::Path) -> BezPath {
    let p = |p: usvg::tiny_skia_path::Point| Point::new(f64::from(p.x), f64::from(p.y));
    let mut out = BezPath::new();
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(a) => out.move_to(p(a)),
            PathSegment::LineTo(a) => out.line_to(p(a)),
            PathSegment::QuadTo(a, b) => out.quad_to(p(a), p(b)),
            PathSegment::CubicTo(a, b, c) => out.curve_to(p(a), p(b), p(c)),
            PathSegment::Close => out.close_path(),
        }
    }
    out
}

const fn map_blend(mode: usvg::BlendMode) -> BlendMode {
    match mode {
        usvg::BlendMode::Normal => BlendMode::Normal,
        usvg::BlendMode::Multiply => BlendMode::Multiply,
        usvg::BlendMode::Screen => BlendMode::Screen,
        usvg::BlendMode::Overlay => BlendMode::Overlay,
        usvg::BlendMode::Darken => BlendMode::Darken,
        usvg::BlendMode::Lighten => BlendMode::Lighten,
        usvg::BlendMode::ColorDodge => BlendMode::ColorDodge,
        usvg::BlendMode::ColorBurn => BlendMode::ColorBurn,
        usvg::BlendMode::HardLight => BlendMode::HardLight,
        usvg::BlendMode::SoftLight => BlendMode::SoftLight,
        usvg::BlendMode::Difference => BlendMode::Difference,
        usvg::BlendMode::Exclusion => BlendMode::Exclusion,
        usvg::BlendMode::Hue => BlendMode::Hue,
        usvg::BlendMode::Saturation => BlendMode::Saturation,
        usvg::BlendMode::Color => BlendMode::Color,
        usvg::BlendMode::Luminosity => BlendMode::Luminosity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::ArtKind;

    const FIXTURE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
  <defs>
    <linearGradient id="grad" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/>
    </linearGradient>
    <clipPath id="clip"><rect x="0" y="0" width="50" height="50"/></clipPath>
  </defs>
  <g id="Body" opacity="0.5">
    <rect id="torso" x="10" y="10" width="80" height="60" fill="url(#grad)" stroke="#000" stroke-width="3"/>
    <g id="Head" transform="translate(100 20)" clip-path="url(#clip)">
      <circle id="face" cx="20" cy="20" r="15" fill="#ffcc00"/>
    </g>
  </g>
  <g id="Hidden"><rect id="nothing" width="5" height="5" fill="none"/></g>
</svg>"##;

    #[test]
    fn groups_keep_their_ids() {
        let tree = import_svg(FIXTURE.as_bytes(), &Limits::default()).unwrap();
        assert_eq!(tree.size, Size::new(200.0, 100.0));
        assert!(tree.find("Body").is_some_and(ArtNode::is_group));
        assert!(tree.root.find_path(&["Body", "Head", "face"]).is_some());
        assert!(tree.find("torso").is_some());
        let body = tree.find("Body").unwrap();
        assert!((body.opacity - 0.5).abs() < 1e-6);
    }

    #[test]
    fn gradient_fill_and_stroke_are_resolved() {
        let tree = import_svg(FIXTURE.as_bytes(), &Limits::default()).unwrap();
        let torso = tree.find("torso").unwrap();
        let ArtKind::Vector { nodes, bounds } = &torso.kind else { panic!("vector expected") };
        assert_eq!(nodes.len(), 2);
        let Node::Fill(fill) = &nodes[0] else { panic!("fill first") };
        let Paint::Linear(g) = &fill.paint else { panic!("linear gradient expected") };
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].color, Rgba8::opaque(255, 0, 0));
        // The gradient runs across the rectangle's width: x = 10 .. 90.
        let start = g.transform * Point::new(-anim_display::GRADIENT_EXTENT, 0.0);
        let end = g.transform * Point::new(anim_display::GRADIENT_EXTENT, 0.0);
        assert!((start.x - 10.0).abs() < 1e-3 && (end.x - 90.0).abs() < 1e-3, "{start:?} {end:?}");
        let Node::Stroke(stroke) = &nodes[1] else { panic!("stroke second") };
        assert_eq!(stroke.style.width, 3.0);
        assert!(bounds.width() >= 79.9);
    }

    #[test]
    fn clip_paths_become_masks_and_transforms_are_baked() {
        let tree = import_svg(FIXTURE.as_bytes(), &Limits::default()).unwrap();
        let head = tree.find("Head").unwrap();
        assert!(matches!(head.mask, Some(ArtMask::Vector { .. })));
        let face = tree.find("face").unwrap();
        let ArtKind::Vector { bounds, .. } = &face.kind else { panic!("vector expected") };
        assert!((bounds.x0 - 105.0).abs() < 1e-3 && (bounds.y0 - 25.0).abs() < 1e-3, "{bounds:?}");
    }

    #[test]
    fn unpainted_paths_are_dropped() {
        let tree = import_svg(FIXTURE.as_bytes(), &Limits::default()).unwrap();
        assert!(tree.find("nothing").is_none());
        assert!(tree.find("Hidden").is_none_or(|g| g.children().is_empty()));
    }

    #[test]
    fn renders_to_a_display_list() {
        let tree = import_svg(FIXTURE.as_bytes(), &Limits::default()).unwrap();
        let list = crate::to_display(&tree, &mut anim_display::ImageStore::new());
        let mut fills = 0;
        list.visit(&mut |n| fills += usize::from(matches!(n, Node::Fill(_))));
        assert_eq!(fills, 2);
    }

    #[test]
    fn rejects_non_svg_without_panicking() {
        assert!(import_svg(b"<html></html>", &Limits::default()).is_err());
        assert!(import_svg(b"", &Limits::default()).is_err());
        assert!(import_svg(&[0x1F, 0x8B, 1, 2, 3], &Limits::default()).is_err());
    }

    #[test]
    fn node_limit_is_enforced() {
        let limits = Limits { max_nodes: 2, ..Limits::default() };
        assert!(matches!(import_svg(FIXTURE.as_bytes(), &limits), Err(ArtworkError::Limit(_))));
    }

    #[test]
    fn rtl_text_is_flattened_to_paths() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"><text id="t" x="5" y="30" font-size="24">שלום abc</text></svg>"#;
        // Needs a system font with Hebrew; without one the text flattens to nothing, which must not fail the import.
        let tree = import_svg(svg.as_bytes(), &Limits::default()).unwrap();
        assert!(tree.find("t").is_none_or(ArtNode::is_group));
    }
}
