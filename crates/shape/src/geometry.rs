//! Bounds and hit testing.
//!
//! All points are in the shape's pixel space (edges divided by 20), like the paths [`EdgeShape::assemble`] returns.
//! Hit testing works on an [`Assembled`] shape so that tools can assemble once and test many points; the
//! [`EdgeShape`] methods assemble on every call and are meant for one-off queries.

use anim_core::kurbo::{ParamCurve, ParamCurveExtrema, ParamCurveNearest, Shape};
use anim_core::{Point, Rect};

use crate::assemble::Assembled;
use crate::edges::EdgeShape;
use crate::segment::{Segment, TWIPS_PER_PX};

/// Accuracy of nearest-point searches on curves, in pixels.
const NEAREST_ACCURACY: f64 = 1.0e-3;

/// Tight bounding box of a segment in pixels.
fn pixel_bounds(segment: &Segment) -> Rect {
    ParamCurveExtrema::bounding_box(&segment.to_path_seg(1.0 / TWIPS_PER_PX))
}

/// What a point hits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// On a stroke: its 1-based style and the distance from the stroke's centre line in pixels.
    Stroke { style: u32, distance: f64 },
    /// Inside a filled region: its 1-based style.
    Fill { style: u32 },
}

impl Assembled {
    /// The 1-based fill style covering `p`, if any. Fills are tested with the even-odd rule, topmost first.
    #[must_use]
    pub fn fill_at(&self, p: Point) -> Option<u32> {
        self.fills.iter().rev().find(|region| region.path.winding(p) & 1 != 0).map(|region| region.style)
    }

    /// The stroke nearest to `p` whose half width plus `tolerance` reaches it, as (style, distance to its centre
    /// line), all in pixels.
    #[must_use]
    pub fn stroke_at(&self, p: Point, tolerance: f64) -> Option<(u32, f64)> {
        let mut best: Option<(f64, u32, f64)> = None;
        for run in &self.strokes {
            let reach = run.width.max(0.0) / 2.0 + tolerance;
            let distance = run.path.segments().map(|segment| segment.nearest(p, NEAREST_ACCURACY).distance_sq.sqrt()).fold(f64::INFINITY, f64::min);
            if distance <= reach && best.is_none_or(|(margin, ..)| distance - reach < margin) {
                best = Some((distance - reach, run.style, distance));
            }
        }
        best.map(|(_, style, distance)| (style, distance))
    }

    /// Strokes win over fills, as they are painted on top.
    #[must_use]
    pub fn hit_test(&self, p: Point, tolerance: f64) -> Option<Hit> {
        if let Some((style, distance)) = self.stroke_at(p, tolerance) {
            return Some(Hit::Stroke { style, distance });
        }
        self.fill_at(p).map(|style| Hit::Fill { style })
    }
}

impl EdgeShape {
    /// Bounding box of the geometry in pixels (stroke width not included), or `None` for an empty shape.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        self.edges.iter().flat_map(|edge| edge.segments.iter()).map(pixel_bounds).reduce(|a, b| a.union(b))
    }

    /// Like [`bounds`](Self::bounds) but grown by half the weight of the stroke each edge is drawn with.
    #[must_use]
    pub fn bounds_with_strokes(&self) -> Option<Rect> {
        self.edges
            .iter()
            .flat_map(|edge| {
                let half = self.stroke(edge.stroke_style).map_or(0.0, |s| s.weight().max(0.0) / 2.0);
                edge.segments.iter().map(move |segment| pixel_bounds(segment).inflate(half, half))
            })
            .reduce(|a, b| a.union(b))
    }

    /// The 1-based fill style at `p` (assembles the shape; use [`Assembled::fill_at`] for many queries).
    #[must_use]
    pub fn fill_at(&self, p: Point) -> Option<u32> {
        self.assemble().fill_at(p)
    }

    /// What `p` hits, with `tolerance` pixels of slack around strokes (assembles the shape).
    #[must_use]
    pub fn hit_test(&self, p: Point, tolerance: f64) -> Option<Hit> {
        self.assemble().hit_test(p, tolerance)
    }

    /// The segment of any edge that is nearest to `p` and within `max_distance` pixels, for selection and curve
    /// editing tools. Ties go to the earlier edge and segment.
    #[must_use]
    pub fn nearest_segment(&self, p: Point, max_distance: f64) -> Option<SegmentHit> {
        let mut best: Option<SegmentHit> = None;
        for (edge, e) in self.edges.iter().enumerate() {
            for (segment, s) in e.segments.iter().enumerate() {
                let seg = s.to_path_seg(1.0 / TWIPS_PER_PX);
                let nearest = seg.nearest(p, NEAREST_ACCURACY);
                let distance = nearest.distance_sq.sqrt();
                if distance <= max_distance && best.is_none_or(|b| distance < b.distance) {
                    best = Some(SegmentHit { edge, segment, t: nearest.t, distance, point: ParamCurve::eval(&seg, nearest.t) });
                }
            }
        }
        best
    }
}

/// The result of [`EdgeShape::nearest_segment`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentHit {
    /// Index into [`EdgeShape::edges`].
    pub edge: usize,
    /// Index into that edge's segments.
    pub segment: usize,
    /// Position along the segment, 0 (start) to 1 (end).
    pub t: f64,
    /// Distance from the query point, in pixels.
    pub distance: f64,
    /// The nearest point on the segment, in pixels.
    pub point: Point,
}

#[cfg(test)]
mod tests {
    use anim_core::{Affine, Rgba8};

    use super::*;
    use crate::codec::parse_edges;
    use crate::edges::Edge;
    use crate::styles::{FillStyle, StrokeStyle};

    fn framed_square() -> EdgeShape {
        let outline = parse_edges("!0 0|400 0|400 400|0 400|0 0").unwrap();
        EdgeShape {
            fills: vec![FillStyle::Solid(Rgba8::WHITE)],
            strokes: vec![StrokeStyle::plain(2.0, Rgba8::BLACK)],
            edges: vec![Edge::new(0, 1, 1, outline)],
        }
    }

    #[test]
    fn bounds_are_in_pixels() {
        let shape = framed_square();
        let b = shape.bounds().unwrap();
        assert_eq!((b.x0, b.y0, b.x1, b.y1), (0.0, 0.0, 20.0, 20.0));
        let with = shape.bounds_with_strokes().unwrap();
        assert_eq!((with.x0, with.y0, with.x1, with.y1), (-1.0, -1.0, 21.0, 21.0));
        assert_eq!(EdgeShape::default().bounds(), None);
    }

    #[test]
    fn bounds_include_curve_extrema() {
        let curve = parse_edges("!0 0[200 -400 400 0").unwrap();
        let shape = EdgeShape { fills: vec![], strokes: vec![], edges: vec![Edge::new(0, 0, 0, curve)] };
        let b = shape.bounds().unwrap();
        assert!((b.y0 + 10.0).abs() < 1e-9, "peak of the parabola is at -10 px, got {}", b.y0);
    }

    #[test]
    fn fill_hit_testing() {
        let shape = framed_square();
        assert_eq!(shape.fill_at(Point::new(10.0, 10.0)), Some(1));
        assert_eq!(shape.fill_at(Point::new(30.0, 10.0)), None);
        assert_eq!(shape.fill_at(Point::new(-0.5, 10.0)), None);
    }

    #[test]
    fn stroke_hits_win_over_fills_and_respect_the_width() {
        let shape = framed_square();
        let assembled = shape.assemble();
        // On the outline (weight 2 px: half width 1 px).
        match assembled.hit_test(Point::new(0.5, 10.0), 0.0) {
            Some(Hit::Stroke { style: 1, distance }) => assert!((distance - 0.5).abs() < 1e-6),
            other => panic!("expected a stroke hit, got {other:?}"),
        }
        assert_eq!(assembled.hit_test(Point::new(3.0, 10.0), 0.0), Some(Hit::Fill { style: 1 }));
        // Outside the stroke but within tolerance.
        assert!(matches!(assembled.hit_test(Point::new(-1.5, 10.0), 1.0), Some(Hit::Stroke { .. })));
        assert_eq!(assembled.hit_test(Point::new(-1.5, 10.0), 0.0), None);
        assert_eq!(assembled.stroke_at(Point::new(10.0, 10.0), 0.0), None);
    }

    #[test]
    fn stroke_distance_follows_curves() {
        let curve = parse_edges("!0 0[200 -400 400 0").unwrap();
        let shape = EdgeShape { fills: vec![], strokes: vec![StrokeStyle::plain(1.0, Rgba8::BLACK)], edges: vec![Edge::new(0, 0, 1, curve)] };
        let assembled = shape.assemble();
        // The apex of the parabola is (10, -10) px.
        let (style, distance) = assembled.stroke_at(Point::new(10.0, -10.2), 0.0).unwrap();
        assert_eq!(style, 1);
        assert!((distance - 0.2).abs() < 1e-3);
        assert_eq!(assembled.stroke_at(Point::new(10.0, 5.0), 0.0), None);
    }

    #[test]
    fn transforming_moves_hits() {
        let mut shape = framed_square();
        shape.transform(Affine::translate((100.0, 0.0)));
        assert_eq!(shape.fill_at(Point::new(110.0, 10.0)), Some(1));
        assert_eq!(shape.fill_at(Point::new(10.0, 10.0)), None);
        let b = shape.bounds().unwrap();
        assert_eq!((b.x0, b.x1), (100.0, 120.0));
    }

    #[test]
    fn nearest_segment_finds_the_closest_side() {
        let shape = framed_square();
        // The outline is one edge of four lines: top, right, bottom, left.
        let near_right = shape.nearest_segment(Point::new(21.5, 8.0), 5.0).unwrap();
        assert_eq!((near_right.edge, near_right.segment), (0, 1));
        assert!((near_right.distance - 1.5).abs() < 1e-6);
        assert!((near_right.point.x - 20.0).abs() < 1e-6 && (near_right.point.y - 8.0).abs() < 1e-6);
        assert!((near_right.t - 0.4).abs() < 1e-3);
        assert!(shape.nearest_segment(Point::new(100.0, 100.0), 5.0).is_none());
        assert!(EdgeShape::default().nearest_segment(Point::ZERO, 100.0).is_none());
    }
}
