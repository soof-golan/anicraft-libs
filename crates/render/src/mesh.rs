//! Textured triangle meshes: the content is rendered once into a texture, then every triangle is drawn with
//! an affine-mapped pattern.
//!
//! Two texture layouts:
//!
//! - A mesh whose targets equal its sources (an undeformed puppet, an identity warp) renders its content with
//!   exactly the matrix a plain group would use, into a texture aligned with the device grid, and copies it: the
//!   result equals the plain render pixel for pixel wherever the triangles cover. Its outline is not anti-aliased.
//! - A deformed mesh renders the content axis-aligned in source space, at the pixel density of the
//!   least-squares affine fit of source onto target, with the source bounds snapped to whole texels so that the
//!   outermost texels are full content (bilinear sampling at the mesh border then clamps to real colour instead
//!   of fading). Its silhouette is anti-aliased.
//!
//! Triangles are filled without anti-aliasing (one triangle owns each pixel centre, so abutting triangles leave
//! no seams and never double-blend). For a deformed mesh a first pass of slightly enlarged triangles gives the
//! silhouette texels to fade out from, and the union of the triangles, anti-aliased, becomes the alpha mask.

use anim_core::{Affine, Point, Rect, Vec2};
use anim_display::Mesh;
use tiny_skia::{BlendMode, FillRule, FilterQuality, Mask, Paint, PathBuilder, Pattern, SpreadMode, Transform};

use crate::composite::{self, Composite};
use crate::draw::{self, Ctx, bump};
use crate::pool::Layer;
use crate::px::PxRect;
use crate::{bounds, convert};

/// More triangles than this in one mesh are ignored.
const MAX_TRIANGLES: usize = 200_000;
/// Longest side of the texture a mesh's content is rendered into.
const MAX_TEXTURE_SIDE: u32 = 4096;
/// How far (device pixels) the first pass grows each triangle.
const FRINGE: f64 = 0.75;
/// Longest displacement of a vertex when growing a triangle, in multiples of [`FRINGE`] (thin triangles).
const MAX_MITER: f64 = 3.0;
/// Triangles with less area than this (device pixels squared) are skipped.
const MIN_AREA: f64 = 1e-6;

/// One valid triangle: source positions in content space, target positions in device space.
#[derive(Debug, Clone, Copy)]
struct Triangle {
    source: [Point; 3],
    target: [Point; 3],
}

/// Draws `mesh` (whose parent maps to device space with `matrix`) onto `layer`.
pub fn draw(cx: &mut Ctx<'_>, layer: &mut Layer, mesh: &Mesh, matrix: Affine, depth: usize) {
    if depth >= bounds::MAX_DEPTH {
        bump(&mut cx.stats.invalid);
        return;
    }
    let full = matrix * mesh.transform;
    let triangles = valid_triangles(mesh, full);
    if triangles.is_empty() {
        if !mesh.triangles.is_empty() {
            bump(&mut cx.stats.invalid);
        }
        return;
    }
    if !in_range(&triangles) {
        bump(&mut cx.stats.invalid);
        return;
    }
    let Some(dest) = destination(&triangles, layer) else {
        bump(&mut cx.stats.culled);
        return;
    };
    let Some(source_bounds) = bounding(triangles.iter().flat_map(|tri| tri.source)) else {
        bump(&mut cx.stats.invalid);
        return;
    };
    let Some(plan) = plan_texture(&triangles, full, source_bounds) else {
        bump(&mut cx.stats.invalid);
        return;
    };
    let Some(mut texture) = cx.pool.take(plan.rect) else {
        bump(&mut cx.stats.degraded_groups);
        return;
    };
    let Some(mut scratch) = cx.pool.take(dest) else {
        cx.pool.recycle(texture);
        bump(&mut cx.stats.degraded_groups);
        return;
    };
    draw::nodes(cx, &mut texture, &mesh.content, plan.matrix, depth.saturating_add(1));
    paint_triangles(cx.antialias && plan.soft_edges, &triangles, plan.matrix, &texture, &mut scratch);
    composite::composite(layer, &scratch, Composite::of(anim_core::BlendMode::Normal));
    cx.pool.recycle(scratch);
    cx.pool.recycle(texture);
    bump(&mut cx.stats.isolated_groups);
    bump(&mut cx.stats.drawn);
}

/// Triangles whose indices exist, whose points are finite and whose target area is not degenerate.
fn valid_triangles(mesh: &Mesh, full: Affine) -> Vec<Triangle> {
    let point = |list: &[Point], index: u32| -> Option<Point> {
        let found = *list.get(usize::try_from(index).ok()?)?;
        (found.x.is_finite() && found.y.is_finite()).then_some(found)
    };
    mesh.triangles
        .iter()
        .take(MAX_TRIANGLES)
        .filter_map(|&[first, second, third]| {
            let source = [point(&mesh.source, first)?, point(&mesh.source, second)?, point(&mesh.source, third)?];
            let local = [point(&mesh.target, first)?, point(&mesh.target, second)?, point(&mesh.target, third)?];
            let target = local.map(|p| full * p);
            (twice_area(&target).abs() > MIN_AREA && target.iter().all(|p| p.x.is_finite() && p.y.is_finite())).then_some(Triangle { source, target })
        })
        .collect()
}

/// Signed area times two (positive for clockwise on a y-down screen).
fn twice_area(tri: &[Point; 3]) -> f64 {
    (tri[1].x - tri[0].x).mul_add(tri[2].y - tri[0].y, -((tri[2].x - tri[0].x) * (tri[1].y - tri[0].y)))
}

fn bounding(points: impl Iterator<Item = Point>) -> Option<Rect> {
    points.fold(None, |acc: Option<Rect>, p| {
        Some(acc.map_or_else(|| Rect::new(p.x, p.y, p.x, p.y), |r| Rect::new(r.x0.min(p.x), r.y0.min(p.y), r.x1.max(p.x), r.y1.max(p.y))))
    })
}

/// The part of `layer` the deformed mesh can touch.
fn destination(triangles: &[Triangle], layer: &Layer) -> Option<PxRect> {
    let rect = bounding(triangles.iter().flat_map(|tri| tri.target))?;
    PxRect::enclosing(rect)?.inflate(2).intersect(&layer.rect)
}

/// Whether the deformed triangles stay within [`convert::MAX_COORDINATE`].
fn in_range(triangles: &[Triangle]) -> bool {
    bounding(triangles.iter().flat_map(|tri| tri.target)).is_some_and(convert::in_range)
}

/// How the content is rendered into the texture.
#[derive(Debug, Clone, Copy)]
struct TexturePlan {
    /// Maps content space to the texture layer's coordinates (its device rectangle is `rect`).
    matrix: Affine,
    rect: PxRect,
    /// Whether the mesh outline is anti-aliased (deformed meshes) or hard-edged (exact copies).
    soft_edges: bool,
}

fn plan_texture(triangles: &[Triangle], full: Affine, source: Rect) -> Option<TexturePlan> {
    let undeformed = triangles.iter().all(|tri| tri.source.iter().zip(&tri.target).all(|(from, to)| (full * *from - *to).hypot() < 1e-6));
    if undeformed {
        let (matrix, rect) = texture_frame(full, source)?;
        return Some(TexturePlan { matrix, rect, soft_edges: false });
    }
    let pairs: Vec<(Point, Point)> = triangles.iter().flat_map(|tri| tri.source.into_iter().zip(tri.target)).collect();
    let fit = fit_affine(&pairs).unwrap_or(full);
    let (scale_x, scale_y) = convert::axis_scales(fit);
    let (width, height) = (source.width(), source.height());
    if width <= 0.0 || height <= 0.0 || !scale_x.is_finite() || !scale_y.is_finite() {
        return None;
    }
    // Whole texels, at least one, and not beyond the texture limit (scaling both axes down together).
    let wanted = (width * scale_x.max(1e-3), height * scale_y.max(1e-3));
    let shrink = (f64::from(MAX_TEXTURE_SIDE) / wanted.0.max(wanted.1)).min(1.0);
    let texels = |extent: f64| anim_core::num::ceil_f64_to_i32((extent * shrink).max(1.0)).max(1);
    let (columns, rows) = (texels(wanted.0), texels(wanted.1));
    let matrix = Affine::scale_non_uniform(f64::from(columns) / width, f64::from(rows) / height) * Affine::translate((-source.x0, -source.y0));
    Some(TexturePlan { matrix, rect: PxRect::new(0, 0, columns, rows), soft_edges: true })
}

/// Second moments of centred source and target positions (the normal equations of the affine fit).
#[derive(Debug, Default)]
struct Moments {
    sxx: f64,
    sxy: f64,
    syy: f64,
    cxx: f64,
    cyx: f64,
    cxy: f64,
    cyy: f64,
}

impl Moments {
    const fn add(&mut self, source: Vec2, target: Vec2) {
        self.sxx = source.x.mul_add(source.x, self.sxx);
        self.sxy = source.x.mul_add(source.y, self.sxy);
        self.syy = source.y.mul_add(source.y, self.syy);
        self.cxx = source.x.mul_add(target.x, self.cxx);
        self.cyx = source.y.mul_add(target.x, self.cyx);
        self.cxy = source.x.mul_add(target.y, self.cxy);
        self.cyy = source.y.mul_add(target.y, self.cyy);
    }
}

/// Least-squares affine map taking each `.0` as close as possible to its `.1`.
fn fit_affine(pairs: &[(Point, Point)]) -> Option<Affine> {
    let count = anim_core::num::usize_to_f64(pairs.len());
    if pairs.is_empty() {
        return None;
    }
    let (mean_source, mean_target) = pairs.iter().fold((Vec2::ZERO, Vec2::ZERO), |(ms, mt), (s, t)| (ms + s.to_vec2(), mt + t.to_vec2()));
    let (mean_source, mean_target) = (mean_source / count, mean_target / count);
    let mut moments = Moments::default();
    for (source, target) in pairs {
        moments.add(source.to_vec2() - mean_source, target.to_vec2() - mean_target);
    }
    let Moments { sxx, sxy, syy, cxx, cyx, cxy, cyy } = moments;
    let det = sxx.mul_add(syy, -(sxy * sxy));
    if !det.is_finite() || det.abs() <= 1e-12 * (sxx + syy) * (sxx + syy) {
        return None;
    }
    let xx = syy.mul_add(cxx, -(sxy * cyx)) / det;
    let yx = sxx.mul_add(cyx, -(sxy * cxx)) / det;
    let xy = syy.mul_add(cxy, -(sxy * cyy)) / det;
    let yy = sxx.mul_add(cyy, -(sxy * cxy)) / det;
    let tx = mean_target.x - xx.mul_add(mean_source.x, yx * mean_source.y);
    let ty = mean_target.y - xy.mul_add(mean_source.x, yy * mean_source.y);
    let fit = Affine::new([xx, xy, yx, yy, tx, ty]);
    (fit.is_finite() && fit.determinant().abs() > 1e-12).then_some(fit)
}

/// The reference matrix (shrunk if the texture would be too big) and the device rectangle of the texture.
fn texture_frame(mut reference: Affine, source: Rect) -> Option<(Affine, PxRect)> {
    for _ in 0..3 {
        // No padding: texels beyond the source area would be transparent, and bilinear sampling at the mesh
        // border must clamp to the content's own edge instead of fading into them.
        let rect = PxRect::enclosing(reference.transform_rect_bbox(source))?;
        let longest = rect.width().max(rect.height());
        if longest == 0 {
            return None;
        }
        if longest <= MAX_TEXTURE_SIDE {
            return Some((reference, rect));
        }
        reference = Affine::scale(f64::from(MAX_TEXTURE_SIDE) / f64::from(longest) * 0.98) * reference;
    }
    None
}

/// The affine map that sends triangle `from` onto triangle `to`.
fn triangle_map(from: &[Point; 3], to: &[Point; 3]) -> Option<Affine> {
    let basis = |tri: &[Point; 3]| Affine::new([tri[1].x - tri[0].x, tri[1].y - tri[0].y, tri[2].x - tri[0].x, tri[2].y - tri[0].y, tri[0].x, tri[0].y]);
    let source = basis(from);
    if source.determinant().abs() < 1e-12 {
        return None;
    }
    let map = basis(to) * source.inverse();
    map.is_finite().then_some(map)
}

/// The triangle grown by `distance` on every side (vertices pushed out along their bisectors).
fn grow(tri: &[Point; 3], distance: f64) -> [Point; 3] {
    let corner = |at: usize| -> Point {
        let (Some(&vertex), Some(&before), Some(&after)) = (tri.get(at), tri.get(at.wrapping_add(2) % 3), tri.get(at.wrapping_add(1) % 3)) else {
            return Point::ZERO;
        };
        let (to_before, to_after) = ((before - vertex).normalize(), (after - vertex).normalize());
        let bisector = -(to_before + to_after);
        if !bisector.is_finite() || bisector.hypot() < 1e-9 {
            return vertex;
        }
        let sin_half = ((1.0 - to_before.dot(to_after)) / 2.0).max(0.0).sqrt().max(1.0 / MAX_MITER);
        vertex + bisector.normalize() * (distance / sin_half)
    };
    [corner(0), corner(1), corner(2)]
}

fn sk_point(point: Point, origin: (i32, i32)) -> tiny_skia::Point {
    tiny_skia::Point::from_xy(anim_core::num::f64_to_f32(point.x - f64::from(origin.0)), anim_core::num::f64_to_f32(point.y - f64::from(origin.1)))
}

fn triangle_path(tri: &[Point; 3], origin: (i32, i32)) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    let [first, second, third] = tri.map(|p| sk_point(p, origin));
    builder.move_to(first.x, first.y);
    builder.line_to(second.x, second.y);
    builder.line_to(third.x, third.y);
    builder.close();
    builder.finish()
}

/// One triangle ready to draw: its pattern matrix and the paths of the grown and the exact triangle.
struct Prepared {
    to_scratch: Transform,
    fringe: tiny_skia::Path,
    exact: tiny_skia::Path,
    /// The exact triangle wound consistently, for the union mask.
    wound: [Point; 3],
}

/// Fills the triangles into `scratch` from `texture`. With `soft` edges the silhouette is anti-aliased.
fn paint_triangles(soft: bool, triangles: &[Triangle], reference: Affine, texture: &Layer, scratch: &mut Layer) {
    let (texture_origin, scratch_origin) = (texture.origin(), scratch.origin());
    let into_texture = Affine::translate(Vec2::new(f64::from(texture_origin.0), f64::from(texture_origin.1)));
    let out_of_device = Affine::translate(Vec2::new(-f64::from(scratch_origin.0), -f64::from(scratch_origin.1)));
    let prepared: Vec<Prepared> = triangles
        .iter()
        .filter_map(|tri| {
            let in_texture = tri.source.map(|p| reference * p);
            let map = triangle_map(&in_texture, &tri.target)?;
            let wound = if twice_area(&tri.target) >= 0.0 { tri.target } else { [tri.target[0], tri.target[2], tri.target[1]] };
            Some(Prepared {
                to_scratch: convert::transform(out_of_device * map * into_texture)?,
                fringe: triangle_path(&grow(&tri.target, FRINGE), scratch_origin)?,
                exact: triangle_path(&tri.target, scratch_origin)?,
                wound,
            })
        })
        .collect();
    let passes: &[bool] = if soft { &[true, false] } else { &[false] };
    for &fringe in passes {
        for item in &prepared {
            let shader = Pattern::new(texture.pm.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, item.to_scratch);
            let paint = Paint { shader, blend_mode: BlendMode::Source, anti_alias: false, ..Paint::default() };
            let path = if fringe { &item.fringe } else { &item.exact };
            scratch.pm.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
    if soft {
        let mut union = PathBuilder::new();
        for item in &prepared {
            let [first, second, third] = item.wound.map(|p| sk_point(p, scratch_origin));
            union.move_to(first.x, first.y);
            union.line_to(second.x, second.y);
            union.line_to(third.x, third.y);
            union.close();
        }
        if let (Some(path), Some(mut mask)) = (union.finish(), Mask::new(scratch.pm.width(), scratch.pm.height())) {
            mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
            scratch.pm.apply_mask(&mask);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn affine_fit_recovers_an_exact_transform() {
        let truth = Affine::translate((5.0, -3.0)) * Affine::rotate(0.4) * Affine::scale_non_uniform(2.0, 0.5);
        let sources = [pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 7.0), pt(10.0, 7.0), pt(4.0, 3.0)];
        let pairs: Vec<_> = sources.iter().map(|&s| (s, truth * s)).collect();
        let fit = fit_affine(&pairs).unwrap();
        for (got, want) in fit.as_coeffs().iter().zip(truth.as_coeffs()) {
            assert!((got - want).abs() < 1e-9, "{got} vs {want}");
        }
        assert!(fit_affine(&[(pt(0.0, 0.0), pt(1.0, 1.0)), (pt(1.0, 1.0), pt(2.0, 2.0))]).is_none(), "collinear points have no fit");
    }

    #[test]
    fn triangle_map_sends_vertices_to_vertices() {
        let from = [pt(0.0, 0.0), pt(4.0, 0.0), pt(0.0, 4.0)];
        let to = [pt(10.0, 10.0), pt(10.0, 18.0), pt(2.0, 10.0)];
        let map = triangle_map(&from, &to).unwrap();
        for (source, target) in from.iter().zip(&to) {
            assert!((map * *source - *target).hypot() < 1e-9);
        }
        assert!(triangle_map(&[pt(0.0, 0.0), pt(1.0, 1.0), pt(2.0, 2.0)], &to).is_none());
    }

    #[test]
    fn grown_triangles_contain_the_original() {
        let tri = [pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0)];
        let grown = grow(&tri, 1.0);
        let area = |t: &[Point; 3]| twice_area(t).abs();
        assert!(area(&grown) > area(&tri));
        let reversed = grow(&[tri[0], tri[2], tri[1]], 1.0);
        assert!(area(&reversed) > area(&tri), "orientation does not matter");
    }

    #[test]
    fn oversized_textures_are_scaled_down() {
        let (reference, rect) = texture_frame(Affine::scale(100.0), Rect::new(0.0, 0.0, 100.0, 50.0)).unwrap();
        assert!(rect.width() <= MAX_TEXTURE_SIDE && rect.height() <= MAX_TEXTURE_SIDE);
        assert!(reference.as_coeffs()[0] < 100.0);
        assert!(texture_frame(Affine::scale(1.0), Rect::new(0.0, 0.0, 0.0, 0.0)).is_none(), "an empty source area has no texture");
        let (_, rect) = texture_frame(Affine::translate((0.5, 0.5)), Rect::new(0.0, 0.0, 10.0, 10.0)).unwrap();
        assert_eq!((rect.x0, rect.x1), (0, 11), "the texture covers the source area exactly, rounded outwards");
    }
}
