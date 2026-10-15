//! Building edge shapes from plain paths, for the drawing tools.
//!
//! Each subpath of an input path becomes one [`Edge`]. A closed subpath with a fill puts the fill on the side of
//! its interior (or exterior, for a hole), so that [`EdgeShape::assemble`] gives the region back. Paths are *not*
//! cut against each other: overlapping inputs stay overlapping, which is what Animate's object drawing mode
//! (one shape per object) wants. Merge-mode cutting is a job for the tool that calls this.

use anim_core::kurbo::{ParamCurve, ParamCurveArea, PathEl, Shape};
use anim_core::{BezPath, Point};

use crate::edges::{Edge, EdgeShape};
use crate::segment::{Segment, TWIPS_PER_PX};
use crate::styles::{FillStyle, StrokeStyle};

/// Subpaths per input path for which hole detection (which compares every pair) is done; beyond it every subpath
/// is treated as an outer boundary.
const MAX_HOLE_ANALYSIS: usize = 512;

/// A path in pixels with the styles to give it.
#[derive(Debug, Clone, PartialEq)]
pub struct PathInput {
    pub path: BezPath,
    pub fill: Option<FillStyle>,
    pub stroke: Option<StrokeStyle>,
}

impl PathInput {
    #[must_use]
    pub const fn new(path: BezPath, fill: Option<FillStyle>, stroke: Option<StrokeStyle>) -> Self {
        Self { path, fill, stroke }
    }
}

struct Subpath {
    segments: Vec<Segment>,
    closed: bool,
}

fn twips(p: Point) -> Point {
    Point::new(p.x * TWIPS_PER_PX, p.y * TWIPS_PER_PX)
}

const fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Appends `segment` to the last subpath (starting one if there is none), unless it is non-finite or zero-length,
/// and moves the pen to its end either way.
fn add(out: &mut Vec<Subpath>, segment: Segment, pen: &mut Point) {
    let points_ok = match segment {
        Segment::Line { from, to } => finite(from) && finite(to),
        Segment::Quad { from, ctrl, to } => finite(from) && finite(ctrl) && finite(to),
        Segment::Cubic { from, ctrl1, ctrl2, to } => finite(from) && finite(ctrl1) && finite(ctrl2) && finite(to),
    };
    *pen = segment.end();
    if points_ok && !segment.is_degenerate(1.0e-9) {
        if out.is_empty() {
            out.push(Subpath { segments: Vec::new(), closed: false });
        }
        if let Some(sub) = out.last_mut() {
            sub.segments.push(segment);
        }
    }
}

/// Splits a path into subpaths of segments in twips. Non-finite and zero-length segments are dropped.
fn subpaths(path: &BezPath) -> Vec<Subpath> {
    let mut out: Vec<Subpath> = Vec::new();
    let mut start = Point::ZERO;
    let mut pen = Point::ZERO;
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(p) => {
                start = twips(p);
                pen = start;
                out.push(Subpath { segments: Vec::new(), closed: false });
            }
            PathEl::LineTo(p) => add(&mut out, Segment::Line { from: pen, to: twips(p) }, &mut pen),
            PathEl::QuadTo(c, p) => add(&mut out, Segment::Quad { from: pen, ctrl: twips(c), to: twips(p) }, &mut pen),
            PathEl::CurveTo(c1, c2, p) => add(&mut out, Segment::Cubic { from: pen, ctrl1: twips(c1), ctrl2: twips(c2), to: twips(p) }, &mut pen),
            PathEl::ClosePath => {
                if pen.distance(start) > 1.0e-9 {
                    add(&mut out, Segment::Line { from: pen, to: start }, &mut pen);
                }
                if let Some(sub) = out.last_mut() {
                    sub.closed = true;
                }
                pen = start;
            }
        }
    }
    out.retain(|s| !s.segments.is_empty());
    out
}

/// The signed area of a contour in square twips. Positive when the contour runs clockwise on a y-down screen, i.e.
/// when its interior is on the right of the direction of travel.
fn signed_area(segments: &[Segment]) -> f64 {
    segments.iter().map(|s| s.to_path_seg(1.0).signed_area()).sum()
}

/// Which subpaths of one input are holes: those inside an odd number of the others.
fn hole_flags(subpaths: &[Subpath]) -> Vec<bool> {
    if subpaths.len() < 2 || subpaths.len() > MAX_HOLE_ANALYSIS {
        return vec![false; subpaths.len()];
    }
    let shapes: Vec<BezPath> = subpaths
        .iter()
        .map(|sub| {
            let mut path = BezPath::new();
            if let Some(first) = sub.segments.first() {
                path.move_to(first.start());
                for segment in &sub.segments {
                    match *segment {
                        Segment::Line { to, .. } => path.line_to(to),
                        Segment::Quad { ctrl, to, .. } => path.quad_to(ctrl, to),
                        Segment::Cubic { ctrl1, ctrl2, to, .. } => path.curve_to(ctrl1, ctrl2, to),
                    }
                }
                path.close_path();
            }
            path
        })
        .collect();
    subpaths
        .iter()
        .enumerate()
        .map(|(i, sub)| {
            let Some(sample) = sub.segments.first().map(|s| s.to_path_seg(1.0).eval(0.5)) else { return false };
            let depth = shapes.iter().enumerate().filter(|(j, shape)| *j != i && shape.winding(sample) != 0).count();
            depth % 2 == 1
        })
        .collect()
}

impl EdgeShape {
    /// Builds a shape from plain paths. Fills and strokes are interned, so equal styles share an index.
    #[must_use]
    pub fn from_paths(inputs: &[PathInput]) -> Self {
        let mut shape = Self::default();
        for input in inputs {
            let fill = input.fill.as_ref().map(|f| shape.intern_fill(f));
            let stroke = input.stroke.as_ref().map_or(0, |s| shape.intern_stroke(s));
            let subs = subpaths(&input.path);
            let holes = if fill.is_some() { hole_flags(&subs) } else { vec![false; subs.len()] };
            for (sub, is_hole) in subs.into_iter().zip(holes) {
                shape.edges.extend(edges_for_subpath(sub, fill, stroke, is_hole));
            }
        }
        shape
    }
}

fn edges_for_subpath(sub: Subpath, fill: Option<u32>, stroke: u32, is_hole: bool) -> Vec<Edge> {
    let Subpath { mut segments, closed } = sub;
    let Some(fill) = fill else { return vec![Edge::new(0, 0, stroke, segments)] };
    // Fills close their contours; the closing line is only stroked when the path itself was closed.
    let closing = match (segments.first(), segments.last()) {
        (Some(first), Some(last)) if !closed && last.end().distance(first.start()) > 1.0e-9 => Some(Segment::Line { from: last.end(), to: first.start() }),
        _ => None,
    };
    let mut contour = segments.clone();
    contour.extend(closing);
    // Interior on the right of travel for clockwise contours; holes want the exterior instead.
    let fill_on_right = (signed_area(&contour) >= 0.0) != is_hole;
    let (fill_style0, fill_style1) = if fill_on_right { (0, fill) } else { (fill, 0) };
    match closing {
        Some(line) if stroke != 0 => vec![Edge::new(fill_style0, fill_style1, stroke, segments), Edge::new(fill_style0, fill_style1, 0, vec![line])],
        Some(line) => {
            segments.push(line);
            vec![Edge::new(fill_style0, fill_style1, stroke, segments)]
        }
        None => vec![Edge::new(fill_style0, fill_style1, stroke, segments)],
    }
}

#[cfg(test)]
mod tests {
    use anim_core::Rgba8;
    use anim_core::kurbo::{Circle, Rect};

    use super::*;

    fn red() -> FillStyle {
        FillStyle::Solid(Rgba8::opaque(255, 0, 0))
    }

    fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        Rect::new(x0, y0, x1, y1).to_path(0.1)
    }

    #[test]
    fn signed_area_sign_matches_the_screen_orientation() {
        let clockwise = subpaths(&rect_path(0.0, 0.0, 10.0, 10.0));
        let ccw: Vec<Segment> = clockwise.first().unwrap().segments.iter().rev().map(|s| s.reversed()).collect();
        assert!(signed_area(&clockwise.first().unwrap().segments) > 0.0);
        assert!(signed_area(&ccw) < 0.0);
    }

    #[test]
    fn a_rectangle_round_trips_through_assembly() {
        let shape = EdgeShape::from_paths(&[PathInput::new(rect_path(5.0, 5.0, 25.0, 15.0), Some(red()), None)]);
        assert_eq!(shape.fills.len(), 1);
        assert_eq!(shape.strokes.len(), 0);
        let assembled = shape.assemble();
        assert_eq!(assembled.fills.len(), 1);
        let region = assembled.fills.first().unwrap();
        assert!((region.path.area().abs() - 200.0).abs() < 1e-9);
        let b = region.path.bounding_box();
        assert_eq!((b.x0, b.y0, b.x1, b.y1), (5.0, 5.0, 25.0, 15.0));
        assert_eq!(shape.fill_at(Point::new(10.0, 10.0)), Some(1));
        assert_eq!(shape.fill_at(Point::new(2.0, 10.0)), None);
    }

    #[test]
    fn orientation_does_not_matter() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((0.0, 10.0));
        path.line_to((10.0, 10.0));
        path.line_to((10.0, 0.0));
        path.close_path();
        let shape = EdgeShape::from_paths(&[PathInput::new(path, Some(red()), None)]);
        assert_eq!(shape.fill_at(Point::new(5.0, 5.0)), Some(1));
        assert_eq!(shape.fill_at(Point::new(15.0, 5.0)), None);
        let edge = shape.edges.first().unwrap();
        assert!(edge.fill_style0 == 1 && edge.fill_style1 == 0, "counter-clockwise contours keep the fill on the left");
    }

    #[test]
    fn a_donut_from_two_subpaths() {
        let mut path = rect_path(0.0, 0.0, 40.0, 40.0);
        // The hole drawn counter-clockwise, as designers often do.
        path.move_to((10.0, 10.0));
        path.line_to((10.0, 30.0));
        path.line_to((30.0, 30.0));
        path.line_to((30.0, 10.0));
        path.close_path();
        let shape = EdgeShape::from_paths(&[PathInput::new(path, Some(red()), None)]);
        assert_eq!(shape.edges.len(), 2);
        assert_eq!(shape.fill_at(Point::new(5.0, 5.0)), Some(1));
        assert_eq!(shape.fill_at(Point::new(20.0, 20.0)), None);
        let area = shape.assemble().fills.first().unwrap().path.area().abs();
        assert!((area - (1600.0 - 400.0)).abs() < 1e-6, "ring area, got {area}");
    }

    #[test]
    fn a_donut_with_a_same_direction_hole_is_still_a_hole() {
        let mut path = rect_path(0.0, 0.0, 40.0, 40.0);
        path.extend(rect_path(10.0, 10.0, 30.0, 30.0));
        let shape = EdgeShape::from_paths(&[PathInput::new(path, Some(red()), None)]);
        assert_eq!(shape.fill_at(Point::new(5.0, 5.0)), Some(1));
        assert_eq!(shape.fill_at(Point::new(20.0, 20.0)), None);
    }

    #[test]
    fn strokes_and_fills_share_one_edge_and_styles_are_interned() {
        let stroke = StrokeStyle::plain(2.0, Rgba8::BLACK);
        let a = PathInput::new(rect_path(0.0, 0.0, 10.0, 10.0), Some(red()), Some(stroke.clone()));
        let b = PathInput::new(rect_path(20.0, 0.0, 30.0, 10.0), Some(red()), Some(stroke));
        let shape = EdgeShape::from_paths(&[a, b]);
        assert_eq!((shape.fills.len(), shape.strokes.len(), shape.edges.len()), (1, 1, 2));
        assert!(shape.edges.iter().all(|e| e.stroke_style == 1 && e.fill_style1 == 1));
        assert_eq!(shape.assemble().strokes.len(), 1);
    }

    #[test]
    fn open_strokes_have_no_fill_and_open_filled_paths_close_without_a_stroked_closing_line() {
        let mut open = BezPath::new();
        open.move_to((0.0, 0.0));
        open.line_to((10.0, 0.0));
        open.line_to((10.0, 10.0));
        let stroke = StrokeStyle::plain(1.0, Rgba8::BLACK);
        let line_only = EdgeShape::from_paths(&[PathInput::new(open.clone(), None, Some(stroke.clone()))]);
        assert_eq!(line_only.edges.len(), 1);
        assert_eq!((line_only.edges.first().unwrap().fill_style0, line_only.edges.first().unwrap().fill_style1), (0, 0));
        assert_eq!(line_only.fills.len(), 0);

        let both = EdgeShape::from_paths(&[PathInput::new(open, Some(red()), Some(stroke))]);
        assert_eq!(both.edges.len(), 2);
        assert_eq!(both.edges.first().unwrap().stroke_style, 1);
        assert_eq!(both.edges.get(1).unwrap().stroke_style, 0);
        assert_eq!(both.fill_at(Point::new(8.0, 2.0)), Some(1));
        // The stroke follows the two drawn sides only.
        assert_eq!(both.assemble().strokes.first().unwrap().path.segments().count(), 2);
    }

    #[test]
    fn curves_survive_as_cubics_and_write_exact_cubics() {
        let circle = Circle::new((50.0, 50.0), 25.0).to_path(0.1);
        let shape = EdgeShape::from_paths(&[PathInput::new(circle, Some(red()), None)]);
        let cubics = shape.edges.iter().flat_map(|e| e.segments.iter()).filter(|s| matches!(s, Segment::Cubic { .. })).count();
        assert!(cubics >= 4);
        let written = shape.to_xfl_edges();
        assert!(written.iter().filter(|e| e.cubics.is_some()).count() >= cubics);
        let back = EdgeShape::from_xfl(shape.fills.clone(), shape.strokes.clone(), &written).unwrap();
        assert_eq!(back.edges.first().unwrap().segments.len(), shape.edges.first().unwrap().segments.len());
        assert!(back.edges.iter().flat_map(|e| e.segments.iter()).any(|s| matches!(s, Segment::Cubic { .. })));
        assert_eq!(back.fill_at(Point::new(50.0, 50.0)), Some(1));
        assert_eq!(back.fill_at(Point::new(5.0, 5.0)), None);
    }

    #[test]
    fn hostile_paths_do_not_panic() {
        // NaN, infinity, a double close and a dangling move.
        let path = BezPath::from_vec(vec![
            PathEl::MoveTo(Point::new(f64::NAN, 0.0)),
            PathEl::LineTo(Point::new(f64::INFINITY, 1.0)),
            PathEl::ClosePath,
            PathEl::ClosePath,
            PathEl::MoveTo(Point::new(0.0, 0.0)),
        ]);
        let shape = EdgeShape::from_paths(&[PathInput::new(path, Some(red()), None)]);
        let _ = shape.assemble();
        let _ = shape.to_xfl_edges();
    }
}
