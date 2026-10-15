//! Conversions between the engine's `f64` geometry and tiny-skia's `f32` types.

use anim_core::kurbo::PathEl;
use anim_core::num::f64_to_f32;
use anim_core::{Affine, BezPath, Point, Rect, Rgba8};
use tiny_skia::{Color, Path, PathBuilder, Transform};

/// `f64` affine to tiny-skia's `f32` transform, `None` when a coefficient is not finite after narrowing.
pub fn transform(matrix: Affine) -> Option<Transform> {
    let [sx, ky, kx, sy, tx, ty] = matrix.as_coeffs().map(f64_to_f32);
    let ts = Transform::from_row(sx, ky, kx, sy, tx, ty);
    ts.is_finite().then_some(ts)
}

/// Straight-alpha 8-bit colour to tiny-skia's straight-alpha float colour.
pub fn color(rgba: Rgba8) -> Color {
    Color::from_rgba8(rgba.r, rgba.g, rgba.b, rgba.a)
}

/// Maps every point of `path` through `matrix` and builds a tiny-skia path. `None` for empty or non-finite input.
pub fn path(path: &BezPath, matrix: Affine) -> Option<Path> {
    let mut builder = PathBuilder::new();
    let map = |pt: Point| {
        let mapped = matrix * pt;
        (f64_to_f32(mapped.x), f64_to_f32(mapped.y))
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(to) => {
                let (px, py) = map(to);
                builder.move_to(px, py);
            }
            PathEl::LineTo(to) => {
                let (px, py) = map(to);
                builder.line_to(px, py);
            }
            PathEl::QuadTo(ctrl, to) => {
                let ((cx, cy), (px, py)) = (map(ctrl), map(to));
                builder.quad_to(cx, cy, px, py);
            }
            PathEl::CurveTo(ctrl1, ctrl2, to) => {
                let ((c1x, c1y), (c2x, c2y), (px, py)) = (map(ctrl1), map(ctrl2), map(to));
                builder.cubic_to(c1x, c1y, c2x, c2y, px, py);
            }
            PathEl::ClosePath => builder.close(),
        }
    }
    builder.finish()
}

/// Bounding box of the control points of `path` mapped through `matrix`: tight enough for culling and always
/// conservative (a Bezier curve stays inside the hull of its control points). `None` for an empty or
/// non-finite path.
pub fn path_bounds(path: &BezPath, matrix: Affine) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    let mut finite = true;
    let mut add = |pt: Point| {
        let mapped = matrix * pt;
        finite &= mapped.x.is_finite() && mapped.y.is_finite();
        bounds = Some(bounds.map_or_else(
            || Rect::new(mapped.x, mapped.y, mapped.x, mapped.y),
            |rect| Rect::new(rect.x0.min(mapped.x), rect.y0.min(mapped.y), rect.x1.max(mapped.x), rect.y1.max(mapped.y)),
        ));
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(to) | PathEl::LineTo(to) => add(to),
            PathEl::QuadTo(ctrl, to) => {
                add(ctrl);
                add(to);
            }
            PathEl::CurveTo(ctrl1, ctrl2, to) => {
                add(ctrl1);
                add(ctrl2);
                add(to);
            }
            PathEl::ClosePath => {}
        }
    }
    bounds.filter(|rect| finite && rect.x0.is_finite() && rect.y0.is_finite() && rect.x1.is_finite() && rect.y1.is_finite())
}

/// Largest coordinate, in device pixels, that geometry may have and still be drawn. tiny-skia's scan converter
/// uses fixed-point arithmetic and misbehaves (wrong coverage, then debug assertions) from about 1e9, and `f32`
/// cannot resolve a pixel past 1.6e7 anyway; real content stays orders of magnitude below this (Animate's stage
/// is at most 16 000 px, its zoom at most 80x).
pub const MAX_COORDINATE: f64 = 1.0e7;

/// Whether every edge of `rect` is within [`MAX_COORDINATE`] of the origin.
pub fn in_range(rect: Rect) -> bool {
    [rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.abs() <= MAX_COORDINATE)
}

/// Lengths of the images of the x and y unit vectors under `matrix`: how much it stretches each axis.
pub fn axis_scales(matrix: Affine) -> (f64, f64) {
    let [xx, xy, yx, yy, _, _] = matrix.as_coeffs();
    (xx.hypot(xy), yx.hypot(yy))
}

/// The union of two optional rectangles.
pub fn union(first: Option<Rect>, second: Option<Rect>) -> Option<Rect> {
    match (first, second) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_conversion_applies_the_transform() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 0.0));
        p.line_to((10.0, 10.0));
        p.close_path();
        let out = path(&p, Affine::translate((5.0, 7.0)) * Affine::scale(2.0)).unwrap();
        let b = out.bounds();
        assert!((b.left() - 5.0).abs() < 1e-6 && (b.right() - 25.0).abs() < 1e-6 && (b.bottom() - 27.0).abs() < 1e-6);
    }

    #[test]
    fn non_finite_input_is_rejected() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((f64::NAN, 1.0));
        assert!(path(&p, Affine::IDENTITY).is_none());
        assert!(path_bounds(&p, Affine::IDENTITY).is_none());
        assert!(transform(Affine::new([f64::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0])).is_none());
        assert!(path(&BezPath::new(), Affine::IDENTITY).is_none());
    }

    #[test]
    fn bounds_cover_curve_control_points() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.curve_to((0.0, 10.0), (10.0, 10.0), (10.0, 0.0));
        let b = path_bounds(&p, Affine::IDENTITY).unwrap();
        assert_eq!((b.x0, b.y0, b.x1, b.y1), (0.0, 0.0, 10.0, 10.0));
        let (sx, sy) = axis_scales(Affine::scale_non_uniform(2.0, 3.0));
        assert!((sx - 2.0).abs() < 1e-12 && (sy - 3.0).abs() < 1e-12);
    }

    #[test]
    fn coordinate_range_is_checked_on_every_edge() {
        assert!(in_range(Rect::new(-1.0e7, 0.0, 5.0, 1.0e7)));
        assert!(!in_range(Rect::new(0.0, 0.0, 1.0e7 + 1.0, 1.0)));
        assert!(!in_range(Rect::new(-1.0e9, 0.0, 1.0, 1.0)));
        assert!(!in_range(Rect::new(0.0, 0.0, f64::NAN, 1.0)));
    }

    #[test]
    fn union_of_options() {
        let a = Some(Rect::new(0.0, 0.0, 1.0, 1.0));
        let b = Some(Rect::new(2.0, 2.0, 3.0, 3.0));
        assert_eq!(union(a, b), Some(Rect::new(0.0, 0.0, 3.0, 3.0)));
        assert_eq!(union(None, b), b);
        assert_eq!(union(None, None), None);
    }
}
