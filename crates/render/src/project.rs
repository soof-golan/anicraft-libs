//! Groups under a perspective projection (an instance moved or rotated in 3D).
//!
//! The group is drawn flat into an offscreen layer ("the source"), exactly as an ordinary isolated group is, with
//! one uniform scale `k` chosen so that the source has about one texel per device pixel where the projection
//! magnifies most. The source is then warped onto the parent by **per-pixel inverse mapping**: every destination
//! pixel is sent through the inverse homography to a point of the source and filtered there.
//!
//! Accuracy:
//!
//! * Geometry is exact to floating point: each destination pixel centre is mapped through the true projective
//!   inverse, not through a mesh or an affine approximation, so edges and corners land where the projection puts them.
//! * Filtering is bilinear on premultiplied pixels. Where the projection shrinks the content (the far side), each
//!   pixel averages an `n x n` grid of bilinear taps with `n` up to 4 following the local minification, which keeps
//!   shrunk content smooth up to a 4x reduction; beyond that the result may alias slightly.
//! * The source has at most [`MAX_SOURCE_SIDE`] texels per side, so content stretched more than that (a plane seen
//!   almost edge-on, close to the viewer) is magnified by bilinear interpolation and looks soft.
//! * Mask, filters, colour transform and blend mode act on the flat source, before the warp, so a blur is blurred
//!   in the object's own plane (as Flash's filters on 3D objects are).
//! * Parts of the plane at or behind the viewer (`w <= 0`) are not drawn.

#![allow(clippy::suboptimal_flops, reason = "no fused multiply-add: results must be bit-identical on every CPU")]
#![allow(clippy::many_single_char_names, reason = "matrix entries and texel corners are named after their positions")]

use anim_core::num::{f64_to_f32, floor_f64_to_i32, u32_to_usize};
use anim_core::{Affine, Homography, Point, Rect};
use anim_display::Group;

use crate::composite::{self, Composite};
use crate::draw::{Ctx, bump};
use crate::group::finish_offscreen;
use crate::pool::Layer;
use crate::px::PxRect;
use crate::{bounds, filters};

/// Longest side of the flat source layer in texels.
pub const MAX_SOURCE_SIDE: f64 = 4096.0;
/// Most taps per axis for minified content.
const MAX_TAPS: u32 = 4;
/// Weights this small are clipped when bounding the projected rectangle (relative to a weight of 1 at z = 0).
const CLIP_W: f64 = 1.0e-3;

/// Draws `group` projected by `projection` (applied after `group.transform`) onto `layer`; `matrix` maps the
/// parent's space to device space.
pub fn draw(cx: &mut Ctx<'_>, layer: &mut Layer, group: &Group, projection: &Homography, matrix: Affine, depth: usize) {
    if depth >= bounds::MAX_DEPTH || !projection.is_finite() {
        bump(&mut cx.stats.invalid);
        return;
    }
    let next = depth.saturating_add(1);
    // Source space: the group's local space after its own transform.
    let Some(content) = source_bounds(cx, group, next) else { return };
    let to_device = projection.before_affine(matrix);
    let k = source_scale(&to_device, content);
    let pad = filters::total_reach(&group.filters, Affine::scale(k));
    let Some(wanted) = PxRect::enclosing(Rect::new(content.x0 * k, content.y0 * k, content.x1 * k, content.y1 * k)).map(|r| r.inflate(pad.saturating_add(1)))
    else {
        bump(&mut cx.stats.invalid);
        return;
    };
    let Some(mut source) = cx.pool.take(wanted) else {
        bump(&mut cx.stats.degraded_groups);
        return;
    };
    bump(&mut cx.stats.isolated_groups);
    let inner = Affine::scale(k) * group.transform;
    finish_offscreen(cx, &mut source, group, inner, wanted, next);
    // Source texel space to device space.
    let warp = to_device.after_affine(Affine::scale(1.0 / k));
    let corners = rect_corners(wanted);
    let Some(area) = projected_bounds(&warp, corners).and_then(PxRect::enclosing).and_then(|r| r.intersect(&layer.rect)) else {
        cx.pool.recycle(source);
        return;
    };
    let Some(mut target) = cx.pool.take(area) else {
        bump(&mut cx.stats.degraded_groups);
        cx.pool.recycle(source);
        return;
    };
    match warp.inverse() {
        Some(inverse) => warp_into(&mut target, &source, &warp, &inverse),
        None => bump(&mut cx.stats.invalid),
    }
    composite::composite(layer, &target, Composite::of(group.blend));
    cx.pool.recycle(target);
    cx.pool.recycle(source);
}

/// The group's content in source space (clipped by the mask), before filters.
fn source_bounds(cx: &Ctx<'_>, group: &Group, next: usize) -> Option<Rect> {
    let content = bounds::nodes(&group.children, group.transform, cx.images, next)?;
    match &group.mask {
        Some(mask) => bounds::intersect(content, bounds::nodes(mask, group.transform, cx.images, next)?),
        None => Some(content),
    }
}

/// Texels per source unit: the largest stretch the projection applies to the sides of `content`, limited so that the
/// source stays within [`MAX_SOURCE_SIDE`] and never smaller than a texel per 16 units.
fn source_scale(to_device: &Homography, content: Rect) -> f64 {
    let corners = [Point::new(content.x0, content.y0), Point::new(content.x1, content.y0), Point::new(content.x1, content.y1), Point::new(content.x0, content.y1)];
    let mapped: Vec<Option<Point>> = corners.iter().map(|c| to_device.apply(*c)).collect();
    let mut stretch = 0.0_f64;
    for (i, (a, b)) in corners.iter().zip(corners.iter().cycle().skip(1)).enumerate() {
        let (Some(Some(da)), Some(Some(db))) = (mapped.get(i), mapped.get(i.saturating_add(1) % 4)) else { continue };
        let length = a.distance(*b);
        if length > 1.0e-9 {
            stretch = stretch.max(da.distance(*db) / length);
        }
    }
    if stretch <= 0.0 || !stretch.is_finite() {
        // Part of the content is behind the viewer: use the local scale at the content's centre.
        stretch = to_device.affine_at(content.center()).map_or(1.0, |a| a.determinant().abs().sqrt());
    }
    let longest = content.width().max(content.height()).max(1.0e-9);
    let limit = MAX_SOURCE_SIDE / longest;
    let k = if stretch.is_finite() && stretch > 0.0 { stretch } else { 1.0 };
    k.min(limit).max(1.0 / 16.0)
}

fn rect_corners(rect: PxRect) -> [Point; 4] {
    let (x0, y0, x1, y1) = (f64::from(rect.x0), f64::from(rect.y0), f64::from(rect.x1), f64::from(rect.y1));
    [Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]
}

/// Device bounds of the image of the quadrilateral `quad` under `h`, with the part behind the viewer clipped away.
pub fn projected_bounds(h: &Homography, quad: [Point; 4]) -> Option<Rect> {
    let [a, b, c, d, e, f, g, hh, i] = h.0;
    // Homogeneous images (X, Y, W) of the corners.
    let homogeneous: Vec<[f64; 3]> = quad.iter().map(|p| [a * p.x + b * p.y + c, d * p.x + e * p.y + f, g * p.x + hh * p.y + i]).collect();
    let mut clipped: Vec<[f64; 3]> = Vec::with_capacity(8);
    for (index, current) in homogeneous.iter().enumerate() {
        let Some(next) = homogeneous.get(index.saturating_add(1) % 4) else { continue };
        let (wc, wn) = (current[2] - CLIP_W, next[2] - CLIP_W);
        if wc >= 0.0 {
            clipped.push(*current);
        }
        if (wc >= 0.0) != (wn >= 0.0) {
            let t = wc / (wc - wn);
            clipped.push([current[0] + (next[0] - current[0]) * t, current[1] + (next[1] - current[1]) * t, current[2] + (next[2] - current[2]) * t]);
        }
    }
    let mut rect: Option<Rect> = None;
    for [x, y, w] in clipped {
        let p = Point::new(x / w, y / w);
        if !(p.x.is_finite() && p.y.is_finite()) {
            continue;
        }
        let single = Rect::new(p.x, p.y, p.x, p.y);
        rect = Some(rect.map_or(single, |r| r.union(single)));
    }
    rect
}

/// Fills `target` by pulling each pixel from `source` through the inverse of `warp` (source texels to device).
fn warp_into(target: &mut Layer, source: &Layer, warp: &Homography, inverse: &Homography) {
    let width = u32_to_usize(target.rect.width());
    let (x0, y0) = (f64::from(target.rect.x0), f64::from(target.rect.y0));
    let sampler = Sampler { data: source.pm.data(), width: u32_to_usize(source.rect.width()), height: u32_to_usize(source.rect.height()), x0: f64::from(source.rect.x0), y0: f64::from(source.rect.y0) };
    crate::par::for_each_row_with(
        target.pm.data_mut(),
        width.saturating_mul(4),
        || (),
        |(), row, bytes| {
            let y = y0 + f64::from(u32::try_from(row).unwrap_or(u32::MAX)) + 0.5;
            for (column, px) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = x0 + f64::from(u32::try_from(column).unwrap_or(u32::MAX)) + 0.5;
                if let Some(rgba) = pixel(&sampler, warp, inverse, x, y) {
                    *px = rgba;
                }
            }
        },
    );
}

/// One destination pixel centred at `(x, y)`.
fn pixel(sampler: &Sampler<'_>, warp: &Homography, inverse: &Homography, x: f64, y: f64) -> Option<[u8; 4]> {
    let centre = inverse.apply_unchecked(Point::new(x, y))?;
    if warp.weight(centre) <= MIN_FRONT {
        return None;
    }
    // How many source texels one device pixel spans.
    let step = |dx: f64, dy: f64| inverse.apply_unchecked(Point::new(x + dx, y + dy)).map_or(1.0, |p| p.distance(centre));
    let spread = step(1.0, 0.0).max(step(0.0, 1.0));
    let taps = if spread > 1.0 && spread.is_finite() { anim_core::num::ceil_f64_to_i32(spread).clamp(1, i32::try_from(MAX_TAPS).unwrap_or(4)) } else { 1 };
    let taps = u32::try_from(taps).unwrap_or(1);
    let mut sum = [0.0_f32; 4];
    let mut count = 0.0_f32;
    for iy in 0..taps {
        for ix in 0..taps {
            let ox = (f64::from(ix) + 0.5) / f64::from(taps) - 0.5;
            let oy = (f64::from(iy) + 0.5) / f64::from(taps) - 0.5;
            let at = if taps == 1 { Some(centre) } else { inverse.apply_unchecked(Point::new(x + ox, y + oy)) };
            count += 1.0;
            let Some(p) = at else { continue };
            for (total, v) in sum.iter_mut().zip(sampler.bilinear(p)) {
                *total += v;
            }
        }
    }
    if count <= 0.0 {
        return None;
    }
    let out = sum.map(|v| (v / count).round().clamp(0.0, 255.0));
    let alpha = unit_byte(out[3]);
    Some([unit_byte(out[0]).min(alpha), unit_byte(out[1]).min(alpha), unit_byte(out[2]).min(alpha), alpha])
}

fn unit_byte(v: f32) -> u8 {
    anim_core::num::unit_f32_to_u8(v / 255.0)
}

/// Weight below which a source point counts as behind the viewer when mapped forward.
pub const MIN_FRONT: f64 = 1.0e-6;

struct Sampler<'a> {
    data: &'a [u8],
    width: usize,
    height: usize,
    x0: f64,
    y0: f64,
}

impl Sampler<'_> {
    /// The premultiplied texel at integer layer coordinates; transparent outside the layer.
    fn texel(&self, x: i32, y: i32) -> [f32; 4] {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else { return [0.0; 4] };
        if x >= self.width || y >= self.height {
            return [0.0; 4];
        }
        let start = y.saturating_mul(self.width).saturating_add(x).saturating_mul(4);
        self.data.get(start..start.saturating_add(4)).map_or([0.0; 4], |px| {
            let mut out = [0.0; 4];
            for (o, v) in out.iter_mut().zip(px) {
                *o = f32::from(*v);
            }
            out
        })
    }

    /// Bilinear sample at source position `p` (texel centres at half integers), premultiplied 0..=255 floats.
    fn bilinear(&self, p: Point) -> [f32; 4] {
        let (fx, fy) = (p.x - self.x0 - 0.5, p.y - self.y0 - 0.5);
        let (ix, iy) = (floor_f64_to_i32(fx), floor_f64_to_i32(fy));
        let (tx, ty) = (f64_to_f32(fx - f64::from(ix)), f64_to_f32(fy - f64::from(iy)));
        let (a, b) = (self.texel(ix, iy), self.texel(ix.saturating_add(1), iy));
        let (c, d) = (self.texel(ix, iy.saturating_add(1)), self.texel(ix.saturating_add(1), iy.saturating_add(1)));
        let mut out = [0.0; 4];
        for (i, slot) in out.iter_mut().enumerate() {
            let at = |t: [f32; 4]| t.get(i).copied().unwrap_or(0.0);
            let top = at(a) * (1.0 - tx) + at(b) * tx;
            let bottom = at(c) * (1.0 - tx) + at(d) * tx;
            *slot = top * (1.0 - ty) + bottom * ty;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_projection_clips_to_the_same_rectangle() {
        let quad = [Point::new(2.0, 3.0), Point::new(12.0, 3.0), Point::new(12.0, 9.0), Point::new(2.0, 9.0)];
        let r = projected_bounds(&Homography::IDENTITY, quad).unwrap();
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (2.0, 3.0, 12.0, 9.0));
    }

    #[test]
    fn points_behind_the_viewer_are_clipped() {
        // w = 1 + x/10: the half x < -10 is behind.
        let h = Homography([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.1, 0.0, 1.0]);
        let quad = [Point::new(-30.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(-30.0, 10.0)];
        let r = projected_bounds(&h, quad).unwrap();
        assert!(r.x1 < 10.0 && r.x1 > 4.0, "{r:?}");
        assert!(r.x0.is_finite());
    }
}
