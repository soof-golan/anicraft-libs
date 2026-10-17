//! Path segments in twips.

use std::hash::Hasher;

use anim_core::hash::Fnv1a;
use anim_core::kurbo::{CubicBez, Line, PathSeg, QuadBez};
use anim_core::{Point, Vec2};
use serde::{Deserialize, Serialize};

/// Twips per pixel: XFL edge coordinates are in twips (1/20 px).
pub const TWIPS_PER_PX: f64 = 20.0;

/// One piece of an edge: a line, a quadratic curve or a cubic curve.
///
/// Every segment carries its own start point, so a break in an edge (an `!` move that does not continue from the
/// previous end) is simply a segment whose start differs from the previous end. Coordinates are twips. Files store lines and quadratic curves in `edges` and true cubic curves in `cubics`;
/// [`Segment::Cubic`] is what the two attributes resolve to when they are merged.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Line { from: Point, to: Point },
    Quad { from: Point, ctrl: Point, to: Point },
    Cubic { from: Point, ctrl1: Point, ctrl2: Point, to: Point },
}

impl Segment {
    /// First point of the segment.
    #[must_use]
    pub const fn start(&self) -> Point {
        match *self {
            Self::Line { from, .. } | Self::Quad { from, .. } | Self::Cubic { from, .. } => from,
        }
    }

    /// Last point of the segment.
    #[must_use]
    pub const fn end(&self) -> Point {
        match *self {
            Self::Line { to, .. } | Self::Quad { to, .. } | Self::Cubic { to, .. } => to,
        }
    }

    /// The same curve traversed the other way.
    #[must_use]
    pub const fn reversed(self) -> Self {
        match self {
            Self::Line { from, to } => Self::Line { from: to, to: from },
            Self::Quad { from, ctrl, to } => Self::Quad { from: to, ctrl, to: from },
            Self::Cubic { from, ctrl1, ctrl2, to } => Self::Cubic { from: to, ctrl1: ctrl2, ctrl2: ctrl1, to: from },
        }
    }

    /// Applies `f` to every point.
    #[must_use]
    pub fn map_points(self, f: impl Fn(Point) -> Point) -> Self {
        match self {
            Self::Line { from, to } => Self::Line { from: f(from), to: f(to) },
            Self::Quad { from, ctrl, to } => Self::Quad { from: f(from), ctrl: f(ctrl), to: f(to) },
            Self::Cubic { from, ctrl1, ctrl2, to } => Self::Cubic { from: f(from), ctrl1: f(ctrl1), ctrl2: f(ctrl2), to: f(to) },
        }
    }

    /// Direction of travel leaving the start point (zero for a fully degenerate segment).
    #[must_use]
    pub fn start_tangent(&self) -> Vec2 {
        let candidates: [Point; 3] = match *self {
            Self::Line { to, .. } => [to, to, to],
            Self::Quad { ctrl, to, .. } => [ctrl, to, to],
            Self::Cubic { ctrl1, ctrl2, to, .. } => [ctrl1, ctrl2, to],
        };
        first_nonzero(self.start(), candidates)
    }

    /// Direction of travel arriving at the end point (zero for a fully degenerate segment).
    #[must_use]
    pub fn end_tangent(&self) -> Vec2 {
        -self.reversed().start_tangent()
    }

    /// The segment as a kurbo path segment with every coordinate multiplied by `scale` (`1/20` converts twips to
    /// pixels).
    #[must_use]
    pub fn to_path_seg(&self, scale: f64) -> PathSeg {
        let s = |p: Point| Point::new(p.x * scale, p.y * scale);
        match *self {
            Self::Line { from, to } => PathSeg::Line(Line::new(s(from), s(to))),
            Self::Quad { from, ctrl, to } => PathSeg::Quad(QuadBez::new(s(from), s(ctrl), s(to))),
            Self::Cubic { from, ctrl1, ctrl2, to } => PathSeg::Cubic(CubicBez::new(s(from), s(ctrl1), s(ctrl2), s(to))),
        }
    }

    /// True when every point lies within `eps` of the start (a zero-length segment).
    #[must_use]
    pub fn is_degenerate(&self, eps: f64) -> bool {
        let start = self.start();
        match *self {
            Self::Line { to, .. } => start.distance(to) <= eps,
            Self::Quad { ctrl, to, .. } => start.distance(ctrl) <= eps && start.distance(to) <= eps,
            Self::Cubic { ctrl1, ctrl2, to, .. } => start.distance(ctrl1) <= eps && start.distance(ctrl2) <= eps && start.distance(to) <= eps,
        }
    }

    /// True when `other` is the same curve within `eps`: endpoints must agree, and control points are compared when
    /// both are quadratic. A line matches a quadratic with the same endpoints (files round-trip lines either way).
    #[must_use]
    pub fn matches(&self, other: &Self, eps: f64) -> bool {
        if self.start().distance(other.start()) > eps || self.end().distance(other.end()) > eps {
            return false;
        }
        match (self, other) {
            (Self::Quad { ctrl: a, .. }, Self::Quad { ctrl: b, .. }) => a.distance(*b) <= eps,
            (Self::Cubic { ctrl1: a1, ctrl2: a2, .. }, Self::Cubic { ctrl1: b1, ctrl2: b2, .. }) => a1.distance(*b1) <= eps && a2.distance(*b2) <= eps,
            _ => true,
        }
    }

    /// Feeds the variant tag and every coordinate to `f`, in a fixed order.
    pub(crate) fn visit_coords(&self, mut f: impl FnMut(f64)) {
        let (tag, points): (u8, [Option<Point>; 4]) = match *self {
            Self::Line { from, to } => (1, [Some(from), Some(to), None, None]),
            Self::Quad { from, ctrl, to } => (2, [Some(from), Some(ctrl), Some(to), None]),
            Self::Cubic { from, ctrl1, ctrl2, to } => (3, [Some(from), Some(ctrl1), Some(ctrl2), Some(to)]),
        };
        f(f64::from(tag));
        for p in points.into_iter().flatten() {
            f(p.x);
            f(p.y);
        }
    }
}

fn first_nonzero(origin: Point, candidates: [Point; 3]) -> Vec2 {
    candidates.into_iter().map(|p| p - origin).find(|v| v.hypot2() > 1e-18).unwrap_or(Vec2::ZERO)
}

/// A 64-bit FNV-1a fingerprint of the exact coordinates, used to tell whether an edge changed since it was read.
#[must_use]
pub fn fingerprint(segments: &[Segment]) -> u64 {
    let mut hash = Fnv1a::new();
    let mut feed = |value: f64| hash.write(&value.to_bits().to_le_bytes());
    feed(f64::from(u32::try_from(segments.len()).unwrap_or(u32::MAX)));
    for segment in segments {
        segment.visit_coords(&mut feed);
    }
    hash.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn reversing_twice_is_identity() {
        let segs = [
            Segment::Line { from: p(0.0, 0.0), to: p(5.0, 1.0) },
            Segment::Quad { from: p(0.0, 0.0), ctrl: p(2.0, 3.0), to: p(5.0, 1.0) },
            Segment::Cubic { from: p(0.0, 0.0), ctrl1: p(1.0, 2.0), ctrl2: p(3.0, 4.0), to: p(5.0, 1.0) },
        ];
        for s in segs {
            assert_eq!(s.reversed().reversed(), s);
            assert_eq!(s.reversed().start(), s.end());
            assert_eq!(s.reversed().end(), s.start());
        }
    }

    #[test]
    fn tangents_follow_control_points_and_survive_degenerates() {
        let q = Segment::Quad { from: p(0.0, 0.0), ctrl: p(0.0, 4.0), to: p(4.0, 4.0) };
        assert_eq!(q.start_tangent(), Vec2::new(0.0, 4.0));
        assert_eq!(q.end_tangent(), Vec2::new(4.0, 0.0));
        let c = Segment::Cubic { from: p(1.0, 1.0), ctrl1: p(1.0, 1.0), ctrl2: p(1.0, 1.0), to: p(1.0, 1.0) };
        assert_eq!(c.start_tangent(), Vec2::ZERO);
        assert!(c.is_degenerate(0.1));
    }

    #[test]
    fn fingerprint_detects_any_coordinate_change() {
        let a = [Segment::Line { from: p(0.0, 0.0), to: p(5.0, 1.0) }];
        let b = [Segment::Line { from: p(0.0, 0.0), to: p(5.0, 1.00001) }];
        assert_eq!(fingerprint(&a), fingerprint(&a));
        assert_ne!(fingerprint(&a), fingerprint(&b));
        assert_ne!(fingerprint(&a), fingerprint(&[]));
    }

    #[test]
    fn matching_tolerates_rounding_and_line_versus_quad() {
        let quad = Segment::Quad { from: p(0.0, 0.0), ctrl: p(10.4, 5.0), to: p(20.0, 0.0) };
        let rounded = Segment::Quad { from: p(0.0, 0.0), ctrl: p(10.0, 5.0), to: p(20.0, 0.0) };
        let line = Segment::Line { from: p(0.0, 0.0), to: p(20.0, 0.0) };
        assert!(quad.matches(&rounded, 0.51));
        assert!(!quad.matches(&rounded, 0.1));
        assert!(quad.matches(&line, 0.51));
    }
}
