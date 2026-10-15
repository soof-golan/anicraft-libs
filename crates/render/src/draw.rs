//! Drawing display-list nodes onto a layer.
//!
//! Matrices: every function receives the matrix that maps the node's *parent* space to *device* space; a layer
//! knows which device rectangle it covers (`layer.rect`), and [`to_layer`] shifts a device matrix to layer pixels
//! just before it reaches tiny-skia.

use anim_core::{Affine, Rect};
use anim_display::{Bitmap, Fill, FillRule, ImageStore, Node, Stroke};
use tiny_skia::{FilterQuality, Paint as SkPaint, PathBuilder, Pattern, SpreadMode, Transform};

use crate::mip::MipCache;
use crate::paint::PaintIssue;
use crate::pool::{Layer, Pool};
use crate::px::PxRect;
use crate::stats::RenderStats;
use crate::{convert, group, mesh, mip, paint, stroke};

/// Everything a draw call needs besides the layer and the matrix.
pub struct Ctx<'a> {
    pub images: &'a ImageStore,
    pub antialias: bool,
    pub pool: &'a mut Pool,
    pub stats: &'a mut RenderStats,
    /// Mip levels of the bitmaps drawn small in this render.
    pub mips: MipCache,
}

/// Counts one more event, saturating.
pub const fn bump(counter: &mut usize) {
    *counter = counter.saturating_add(1);
}

/// Maps device matrices into the pixel grid of `layer`.
pub fn to_layer(layer: &Layer, matrix: Affine) -> Affine {
    Affine::translate((-f64::from(layer.rect.x0), -f64::from(layer.rect.y0))) * matrix
}

/// Whether a rectangle in layer pixels touches the layer.
pub fn visible(layer: &Layer, rect: Rect) -> bool {
    PxRect::enclosing(rect).and_then(|r| r.intersect(&PxRect::from_size(layer.pm.width(), layer.pm.height()))).is_some()
}

/// Draws `list` bottom to top.
pub fn nodes(cx: &mut Ctx<'_>, layer: &mut Layer, list: &[Node], matrix: Affine, depth: usize) {
    for node in list {
        match node {
            Node::Fill(item) => fill(cx, layer, item, matrix),
            Node::Stroke(item) => outline(cx, layer, item, matrix),
            Node::Bitmap(item) => bitmap(cx, layer, item, matrix),
            Node::Group(item) => group::draw(cx, layer, item, matrix, depth),
            Node::Mesh(item) => mesh::draw(cx, layer, item, matrix, depth),
        }
    }
}

const fn sk_fill_rule(rule: FillRule) -> tiny_skia::FillRule {
    match rule {
        FillRule::NonZero => tiny_skia::FillRule::Winding,
        FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
    }
}

/// Notes why a paint produced no shader.
const fn paint_failed(cx: &mut Ctx<'_>, issue: PaintIssue) {
    match issue {
        PaintIssue::MissingImage => bump(&mut cx.stats.missing_images),
        PaintIssue::Degenerate => bump(&mut cx.stats.invalid),
    }
}

fn fill(cx: &mut Ctx<'_>, layer: &mut Layer, item: &Fill, matrix: Affine) {
    if item.path.elements().is_empty() {
        return;
    }
    let local = to_layer(layer, matrix * item.transform);
    let Some(bounds) = convert::path_bounds(&item.path, local).filter(|b| convert::in_range(*b)) else {
        bump(&mut cx.stats.invalid);
        return;
    };
    if !visible(layer, bounds) {
        bump(&mut cx.stats.culled);
        return;
    }
    let Some(path) = convert::path(&item.path, local) else {
        bump(&mut cx.stats.invalid);
        return;
    };
    let images = cx.images;
    prepare_mips(cx, &item.paint, local);
    let shader = match paint::shader(&item.paint, local, local, images, &cx.mips) {
        Ok(shader) => shader,
        Err(issue) => return paint_failed(cx, issue),
    };
    let sk = SkPaint { shader, anti_alias: cx.antialias, ..SkPaint::default() };
    layer.pm.fill_path(&path, &sk, sk_fill_rule(item.rule), Transform::identity(), None);
    bump(&mut cx.stats.drawn);
}

/// Builds the mip level a bitmap paint will sample from, if it is drawn small enough to want one.
fn prepare_mips(cx: &mut Ctx<'_>, paint: &anim_display::Paint, to_layer: Affine) {
    if let anim_display::Paint::Bitmap { image, transform, smoothing: true, .. } = paint
        && let Some(found) = cx.images.get(*image)
    {
        let level = paint::wanted_level(found, to_layer * *transform, true);
        if level > 0 {
            cx.mips.ensure(cx.images, *image, level);
        }
    }
}

fn outline(cx: &mut Ctx<'_>, layer: &mut Layer, item: &Stroke, matrix: Affine) {
    if item.path.elements().is_empty() {
        return;
    }
    let local = to_layer(layer, matrix * item.transform);
    let Some(plan) = stroke::plan(&item.style, local) else {
        bump(&mut cx.stats.invalid);
        return;
    };
    let Some(bounds) = convert::path_bounds(&item.path, local).filter(|b| convert::in_range(*b)) else {
        bump(&mut cx.stats.invalid);
        return;
    };
    if !visible(layer, bounds.inflate(plan.reach, plan.reach)) {
        bump(&mut cx.stats.culled);
        return;
    }
    let (Some(path), Some(post)) = (convert::path(&item.path, plan.pre), convert::transform(plan.post)) else {
        bump(&mut cx.stats.invalid);
        return;
    };
    let images = cx.images;
    prepare_mips(cx, &item.paint, local);
    let shader = match paint::shader(&item.paint, plan.pre, local, images, &cx.mips) {
        Ok(shader) => shader,
        Err(issue) => return paint_failed(cx, issue),
    };
    let sk = SkPaint { shader, anti_alias: cx.antialias, ..SkPaint::default() };
    layer.pm.stroke_path(&path, &sk, &plan.stroke, post, None);
    bump(&mut cx.stats.drawn);
}

fn bitmap(cx: &mut Ctx<'_>, layer: &mut Layer, item: &Bitmap, matrix: Affine) {
    let images = cx.images;
    let Some(image) = images.get(item.image).filter(|found| paint::is_usable(found)) else {
        bump(&mut cx.stats.missing_images);
        return;
    };
    let local = to_layer(layer, matrix * item.transform);
    let size = Rect::new(0.0, 0.0, f64::from(image.width), f64::from(image.height));
    let bounds = local.transform_rect_bbox(size);
    if !convert::in_range(bounds) {
        bump(&mut cx.stats.invalid);
        return;
    }
    if !visible(layer, bounds) {
        bump(&mut cx.stats.culled);
        return;
    }
    let level = paint::wanted_level(image, local, item.smoothing);
    if level > 0 {
        cx.mips.ensure(images, item.image, level);
    }
    let Ok(sampled) = paint::sample(item.image, local, item.smoothing, images, &cx.mips) else {
        bump(&mut cx.stats.missing_images);
        return;
    };
    // The rectangle always covers the whole image; at mip level n the image is 1/2^n as many pixels across.
    let (width, height) = (mip::level_extent(image.width, sampled.level), mip::level_extent(image.height, sampled.level));
    let (Some(ts), Some(rect)) = (convert::transform(local * sampled.level_to_full()), tiny_skia::Rect::from_xywh(0.0, 0.0, width, height)) else {
        bump(&mut cx.stats.invalid);
        return;
    };
    let quality = if item.smoothing { FilterQuality::Bilinear } else { FilterQuality::Nearest };
    let shader = Pattern::new(sampled.pixmap, SpreadMode::Pad, quality, 1.0, Transform::identity());
    let sk = SkPaint { shader, anti_alias: cx.antialias, ..SkPaint::default() };
    layer.pm.fill_path(&PathBuilder::from_rect(rect), &sk, tiny_skia::FillRule::Winding, ts, None);
    bump(&mut cx.stats.drawn);
}
