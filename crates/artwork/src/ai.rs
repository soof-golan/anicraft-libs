//! Illustrator (`.ai`) and PDF import through `VectorCraft`'s importers.
//!
//! A PDF-compatible `.ai` carries two copies of its art: the PDF page content, which flattens layers into
//! optional content groups at best, and Illustrator's own private data, which has the real hierarchy of layers,
//! sublayers and groups with their names. `VectorCraft` reads both; this module converts the resulting document
//! into an [`ArtworkTree`]. A plain PDF comes in with one layer per optional content group.

use anim_core::kurbo::Shape as _;
use anim_core::{Affine, BezPath, BlendMode, Rect, Rgba8, Size, Vec2, num};
use anim_display::{Cap, Fill, FillRule, Gradient, GradientStop, Group, Interpolation, Join, Node, Paint, Spread, Stroke, StrokeScale, StrokeStyle};
use vectorcraft_color::{Color, GradientKind, GradientPaint};
use vectorcraft_doc::{AppearanceItem, Document, ImageObject, LineCap, LineJoin, NodeKind, StrokeLayer};
use vectorcraft_pdf::{CropTo, ImportOptions, TextAs};

use crate::raster::resample;
use crate::tree::{ArtMask, ArtNode, ArtworkTree};
use crate::{ArtworkError, Limits, check_input};

/// Imports an Illustrator file or PDF.
///
/// The tree is `root` > layers > sublayers and groups > objects, in paint order (bottom first), with names
/// exactly as Illustrator has them (objects without a name have an empty one). Every path, compound path and
/// text-free object is a [`crate::ArtKind::Vector`] leaf with fills, strokes, linear and radial gradients;
/// placed images are resampled onto the canvas as rasters. Coordinates are points, with the origin at the top
/// left of the first artboard, which gives [`ArtworkTree::size`]. Hidden layers and objects are kept with
/// `visible = false`. Clipping groups keep their clip path as the group's mask.
///
/// Not carried over (each leaves a warning): text objects (use outlines in the source), opacity masks, symbols,
/// pattern paints, brushes, live effects, gradient meshes and freeform gradients (drawn as a flat colour).
///
/// # Errors
/// [`ArtworkError::Pdf`] if the file cannot be read, [`ArtworkError::Limit`] if it is too large.
pub fn import_ai(bytes: &[u8], limits: &Limits) -> Result<ArtworkTree, ArtworkError> {
    check_input(bytes, limits)?;
    // The PDF interpreter recurses deeply; give it (and the conversion) a stack of its own.
    vectorcraft_doc::depth::with_deep_stack(|| {
        let (doc, warnings) = load(bytes).map_err(ArtworkError::Pdf)?;
        convert_document(&doc, warnings, limits)
    })
}

fn load(bytes: &[u8]) -> Result<(Document, Vec<String>), String> {
    if vectorcraft_pdf::is_postscript(bytes) {
        let imported = vectorcraft_eps::import_with(bytes, true)?;
        return Ok((imported.document, imported.warnings));
    }
    let is_ai = vectorcraft_pdf::has_illustrator_data(bytes, None);
    let options = ImportOptions {
        // An Illustrator file's trim boxes are its artboards.
        crop: if is_ai { CropTo::Trim } else { CropTo::Crop },
        // Text as glyph outlines: it looks the same without the fonts, and the layers of the private data match it.
        text_as: TextAs::Outlines,
        layers: true,
        uncalibrated_rgb: is_ai,
        ..ImportOptions::default()
    };
    let report = vectorcraft_pdf::import_with_report(bytes, &options).map_err(|e| e.to_string())?;
    let (doc, mut warnings) = (report.document, report.warnings);
    if is_ai && let Some(private) = vectorcraft_pdf::illustrator_data(bytes, None) {
        // The editor's own copy of the art has the layer, sublayer and group hierarchy. `trust`: use it even where
        // it draws a little differently from the PDF part, because the structure is what we are after.
        let (layered, notes, read) = vectorcraft_eps::layered_ai(&private, doc, warnings, true, true, report.user_unit);
        warnings = notes;
        if read {
            // The layers hold the art the PDF part leaves out, so these notes no longer apply.
            warnings.retain(|w| w != vectorcraft_pdf::OFF_ARTBOARD_NOTE && w != vectorcraft_pdf::PAST_PAGE_NOTE);
        }
        return Ok((layered, warnings));
    }
    Ok((doc, warnings))
}

fn convert_document(doc: &Document, warnings: Vec<String>, limits: &Limits) -> Result<ArtworkTree, ArtworkError> {
    let board = doc.artboards.first().map(|a| a.rect);
    let origin = board.map_or(Vec2::ZERO, |r| Vec2::new(r.x0, r.y0));
    let mut ctx = Ctx { doc, limits, warnings, nodes: 0, pixels: 0, noted: Vec::new(), shift: Affine::translate(-origin) };
    if doc.artboards.len() > 1 {
        ctx.warnings.push(format!("the file has {} artboards; only the first gives the canvas, art on the others is kept off-canvas", doc.artboards.len()));
    }
    let mut children = Vec::new();
    for layer in &doc.layers {
        if let Some(node) = ctx.convert(layer, 0)? {
            children.push(node);
        }
    }
    let root = ArtNode::group("root", children);
    let size = board.map_or_else(|| root.bounds().map_or(Size::ZERO, |b| Size::new(b.x1.max(0.0), b.y1.max(0.0))), |r| Size::new(r.width(), r.height()));
    Ok(ArtworkTree { size, root, warnings: ctx.warnings })
}

struct Ctx<'a> {
    doc: &'a Document,
    limits: &'a Limits,
    warnings: Vec<String>,
    nodes: usize,
    pixels: u64,
    noted: Vec<&'static str>,
    /// Document space to canvas space (the first artboard's corner becomes the origin).
    shift: Affine,
}

impl Ctx<'_> {
    fn note(&mut self, what: &'static str) {
        if !self.noted.contains(&what) {
            self.noted.push(what);
            self.warnings.push(what.to_owned());
        }
    }

    fn convert(&mut self, node: &vectorcraft_doc::Node, depth: usize) -> Result<Option<ArtNode>, ArtworkError> {
        if depth > self.limits.max_depth {
            self.note("objects nested deeper than the limit were left out");
            return Ok(None);
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > self.limits.max_nodes {
            return Err(ArtworkError::Limit("nodes"));
        }
        let name = node.name.clone().unwrap_or_default();
        let mut art = match &node.kind {
            NodeKind::Layer { children, clip, template, .. } => {
                let mut art = self.group(&name, children, *clip, depth)?;
                if *template {
                    art.visible = false;
                }
                art
            }
            NodeKind::Group { children, clip } => self.group(&name, children, *clip, depth)?,
            NodeKind::Blend { children, .. } => {
                self.note("live blends are drawn as their key objects only");
                self.group(&name, children, false, depth)?
            }
            NodeKind::Envelope { content, .. } => {
                self.note("envelope distortions are drawn undistorted");
                self.group(&name, content, false, depth)?
            }
            NodeKind::Path { path, rule, .. } => self.vector(node, &name, &path.to_bezpath(), *rule),
            NodeKind::Compound { children, rule } => {
                let mut combined = BezPath::new();
                collect_paths(children, &mut combined, 0);
                self.vector(node, &name, &combined, *rule)
            }
            NodeKind::Image(image) => self.image(&name, image),
            NodeKind::Text(_) => {
                self.note("text objects are not carried over");
                ArtNode::vector(name, Vec::new(), Rect::ZERO)
            }
            NodeKind::SymbolInstance { .. } => {
                self.note("symbol instances are not carried over");
                ArtNode::vector(name, Vec::new(), Rect::ZERO)
            }
            NodeKind::Mesh(_) | NodeKind::Repeat(_) | NodeKind::PlacedDocument(_) => {
                self.note("gradient meshes, repeats and placed documents are not carried over");
                ArtNode::vector(name, Vec::new(), Rect::ZERO)
            }
        };
        art.visible = art.visible && node.visible;
        art.opacity = node.opacity.clamp(0.0, 1.0);
        art.blend = map_blend(node.blend);
        if node.extras().mask.is_some() {
            self.note("opacity masks are not carried over");
        }
        Ok(Some(art))
    }

    fn group(&mut self, name: &str, children: &[impl AsRef<vectorcraft_doc::Node>], clip: bool, depth: usize) -> Result<ArtNode, ArtworkError> {
        let mut kids = Vec::new();
        let mut mask = None;
        for (i, child) in children.iter().enumerate() {
            let child = child.as_ref();
            if clip && i == 0 {
                let mut nodes = Vec::new();
                self.clip_nodes(child, &mut nodes, 0);
                mask = Some(ArtMask::Vector { nodes });
                continue;
            }
            kids.extend(self.convert(child, depth.saturating_add(1))?);
        }
        let mut art = ArtNode::group(name, kids);
        art.mask = mask;
        Ok(art)
    }

    /// A clipping object's shape as black fills.
    fn clip_nodes(&self, node: &vectorcraft_doc::Node, out: &mut Vec<Node>, depth: usize) {
        if depth > self.limits.max_depth {
            return;
        }
        let mut shape = |path: BezPath, rule: vectorcraft_geom::FillRule| {
            out.push(Node::Fill(Fill { transform: self.shift, path, rule: map_rule(rule), paint: Paint::Solid(Rgba8::BLACK) }));
        };
        match &node.kind {
            NodeKind::Path { path, rule, .. } => shape(path.to_bezpath(), *rule),
            NodeKind::Compound { children, rule } => {
                let mut combined = BezPath::new();
                collect_paths(children, &mut combined, 0);
                shape(combined, *rule);
            }
            NodeKind::Group { children, .. } | NodeKind::Layer { children, .. } => {
                for child in children {
                    self.clip_nodes(child, out, depth.saturating_add(1));
                }
            }
            _ => {}
        }
    }

    /// A path with its appearance: fills and strokes bottom to top.
    fn vector(&mut self, node: &vectorcraft_doc::Node, name: &str, path: &BezPath, rule: vectorcraft_geom::FillRule) -> ArtNode {
        let doc_bounds = path.bounding_box();
        let bounds = self.shift.transform_rect_bbox(doc_bounds);
        let rule = map_rule(rule);
        let mut nodes = Vec::new();
        for item in &node.appearance.items {
            let (paint, opacity, blend, stroke) = match item {
                AppearanceItem::Fill(f) if f.visible => (self.paint(&f.paint, doc_bounds, 1.0), f.opacity, f.blend, None),
                AppearanceItem::Stroke(s) if s.visible => (self.paint(&s.paint, doc_bounds, 1.0), s.opacity, s.blend, Some(s)),
                _ => continue,
            };
            let Some(paint) = paint else { continue };
            let drawn = match stroke {
                None => Node::Fill(Fill { transform: self.shift, path: path.clone(), rule, paint }),
                Some(s) => Node::Stroke(Stroke { transform: self.shift, path: path.clone(), style: self.stroke_style(s), paint }),
            };
            let blend = map_blend(blend);
            if opacity < 1.0 || blend != BlendMode::Normal {
                nodes.push(Node::Group(Group {
                    color: anim_core::ColorTransform::alpha(opacity.clamp(0.0, 1.0)),
                    blend,
                    children: vec![drawn],
                    ..Group::default()
                }));
            } else {
                nodes.push(drawn);
            }
        }
        ArtNode::vector(name, nodes, bounds)
    }

    fn stroke_style(&mut self, s: &StrokeLayer) -> StrokeStyle {
        if s.brush.is_some() {
            self.note("brush strokes are drawn as plain strokes");
        }
        StrokeStyle {
            width: s.width,
            cap: match s.cap {
                LineCap::Butt => Cap::None,
                LineCap::Round => Cap::Round,
                LineCap::Square => Cap::Square,
            },
            join: match s.join {
                LineJoin::Miter => Join::Miter,
                LineJoin::Round => Join::Round,
                LineJoin::Bevel => Join::Bevel,
            },
            miter_limit: s.miter_limit,
            scale: StrokeScale::Normal,
            dashes: s.dash.as_ref().filter(|d| d.is_dashed()).map(|d| d.pattern.clone()).unwrap_or_default(),
        }
    }

    fn paint(&mut self, paint: &vectorcraft_color::Paint, bounds: Rect, alpha: f32) -> Option<Paint> {
        match paint {
            vectorcraft_color::Paint::None => None,
            vectorcraft_color::Paint::Solid { color, .. } => Some(Paint::Solid(rgba(color, alpha))),
            vectorcraft_color::Paint::Gradient(g) => Some(self.gradient(g, bounds, alpha)),
            vectorcraft_color::Paint::Pattern { .. } => {
                self.note("pattern paints are not carried over");
                None
            }
        }
    }

    fn gradient(&mut self, paint: &GradientPaint, bounds: Rect, alpha: f32) -> Paint {
        let stops: Vec<GradientStop> =
            paint.gradient.expanded_stops().into_iter().map(|(offset, color, opacity)| GradientStop { offset, color: rgba(&color, opacity * alpha) }).collect();
        if paint.gradient.kind == GradientKind::Freeform {
            self.note("freeform gradients are drawn as a flat colour");
            return Paint::Solid(stops.first().map_or(Rgba8::BLACK, |s| s.color));
        }
        let geom = paint.resolve(bounds);
        let axis = geom.end - geom.start;
        let unit = axis / anim_display::GRADIENT_EXTENT;
        let (transform, focal, make): (Affine, f64, fn(Gradient) -> Paint) = if paint.gradient.kind == GradientKind::Radial {
            let squash = geom.aspect;
            let frame = Affine::new([unit.x, unit.y, -unit.y * squash, unit.x * squash, geom.start.x, geom.start.y]);
            let len2 = axis.hypot2();
            let focal = geom.focal.filter(|_| len2 > 0.0).map_or(0.0, |f| ((f - geom.start).dot(axis) / len2).clamp(-1.0, 1.0));
            (frame, focal, Paint::Radial)
        } else {
            let mid = geom.start.midpoint(geom.end);
            let half = axis / 2.0 / anim_display::GRADIENT_EXTENT;
            (Affine::new([half.x, half.y, -half.y, half.x, mid.x, mid.y]), 0.0, Paint::Linear)
        };
        make(Gradient { transform, stops, spread: Spread::Pad, interpolation: Interpolation::Rgb, focal })
    }

    fn image(&mut self, name: &str, object: &ImageObject) -> ArtNode {
        let empty = || ArtNode::vector(name, Vec::new(), Rect::ZERO);
        let Some(blob) = self.doc.images.get(&object.key) else {
            self.note("an image's data is missing");
            return empty();
        };
        let Ok(decoded) = image::load_from_memory(blob.bytes.as_slice()) else {
            self.note("images that could not be decoded were left out");
            return empty();
        };
        let decoded = decoded.to_rgba8();
        let (w, h) = decoded.dimensions();
        let budget = self.limits.max_layer_pixels.min(self.limits.max_total_pixels.saturating_sub(self.pixels));
        if u64::from(w).saturating_mul(u64::from(h)) > budget {
            self.note("images over the pixel limit were left out");
            return empty();
        }
        // The image's own pixel size can differ from what the object records; trust the data.
        let xf =
            self.shift * object.xf * Affine::scale_non_uniform(f64::from(object.width) / f64::from(w.max(1)), f64::from(object.height) / f64::from(h.max(1)));
        let Some((pixels, offset)) = resample(decoded.as_raw(), w, h, xf, budget) else {
            self.note("images that could not be placed were left out");
            return empty();
        };
        self.pixels = self.pixels.saturating_add(u64::from(pixels.width).saturating_mul(u64::from(pixels.height)));
        ArtNode::raster(name, pixels, offset)
    }
}

/// Appends the outlines of every path in `children` (compound paths hold paths, possibly nested).
fn collect_paths(children: &[impl AsRef<vectorcraft_doc::Node>], out: &mut BezPath, depth: usize) {
    if depth > 32 {
        return;
    }
    for child in children {
        match &child.as_ref().kind {
            NodeKind::Path { path, .. } => out.extend(path.to_bezpath().iter()),
            NodeKind::Compound { children, .. } | NodeKind::Group { children, .. } => collect_paths(children, out, depth.saturating_add(1)),
            _ => {}
        }
    }
}

fn rgba(color: &Color, alpha: f32) -> Rgba8 {
    let [r, g, b] = color.to_rgb_uncalibrated();
    Rgba8::new(num::unit_f32_to_u8(r), num::unit_f32_to_u8(g), num::unit_f32_to_u8(b), num::unit_f32_to_u8(alpha))
}

const fn map_rule(rule: vectorcraft_geom::FillRule) -> FillRule {
    match rule {
        vectorcraft_geom::FillRule::NonZero => FillRule::NonZero,
        vectorcraft_geom::FillRule::EvenOdd => FillRule::EvenOdd,
    }
}

const fn map_blend(mode: vectorcraft_color::BlendMode) -> BlendMode {
    use vectorcraft_color::BlendMode as V;
    match mode {
        V::Normal => BlendMode::Normal,
        V::Darken => BlendMode::Darken,
        V::Multiply => BlendMode::Multiply,
        V::ColorBurn => BlendMode::ColorBurn,
        V::Lighten => BlendMode::Lighten,
        V::Screen => BlendMode::Screen,
        V::ColorDodge => BlendMode::ColorDodge,
        V::Overlay => BlendMode::Overlay,
        V::SoftLight => BlendMode::SoftLight,
        V::HardLight => BlendMode::HardLight,
        V::Difference => BlendMode::Difference,
        V::Exclusion => BlendMode::Exclusion,
        V::Hue => BlendMode::Hue,
        V::Saturation => BlendMode::Saturation,
        V::Color => BlendMode::Color,
        V::Luminosity => BlendMode::Luminosity,
    }
}
