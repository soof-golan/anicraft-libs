//! The edge shape model and its XFL transport form.
//!
//! An XFL `<DOMShape>` holds `<fills>`, `<strokes>` and a flat list of `<Edge>` elements. Each edge is a polyline of
//! lines and quadratic curves that separates the fill on its left (`fillStyle0`) from the fill on its right
//! (`fillStyle1`) and may be stroked (`strokeStyle`). Newer files add elements with only a `cubics` attribute that
//! carry the exact cubic curves the quadratic edges approximate.
//!
//! [`XflEdge`] is the raw attribute text of one element, which the XFL reader fills and the writer consumes;
//! [`EdgeShape::from_xfl`] and [`EdgeShape::to_xfl_edges`] convert between that and the model. An edge that has not
//! been edited since it was read writes back its original text unchanged.

use std::collections::HashMap;

use anim_core::num::round_f64_to_i64;
use anim_core::{Affine, Point};
use serde::{Deserialize, Serialize};

use crate::codec::{CubicRun, encode_cubic, encode_edges, parse_cubics, parse_edges};
use crate::error::ShapeError;
use crate::segment::{Segment, TWIPS_PER_PX, fingerprint};
use crate::styles::{FillStyle, StrokeStyle};

/// Tolerance, in twips, when matching a cubic's quadratic approximation to the segments of an edge. Files round the
/// approximation to whole twips while the edge keeps 1/256.
const MATCH_EPS: f64 = 1.01;

/// The attributes of one `<Edge>` element as text. Style indices are 1-based and 0 means none (also when the
/// attribute is absent).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct XflEdge {
    pub fill_style0: u32,
    pub fill_style1: u32,
    pub stroke_style: u32,
    /// The `edges` attribute.
    pub edges: Option<String>,
    /// The `cubics` attribute.
    pub cubics: Option<String>,
}

/// What an edge looked like in the file, so that it can be written back byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Original {
    edges: Option<String>,
    cubics: Option<String>,
    /// `cubics`-only elements that followed this one in the file.
    following: Vec<String>,
    /// [`fingerprint`] of the segments when the edge was read.
    fingerprint: u64,
}

/// One edge: a run of segments with its fill and stroke styles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// Fill on the left of the direction of travel (1-based, 0 = none).
    pub fill_style0: u32,
    /// Fill on the right of the direction of travel (1-based, 0 = none).
    pub fill_style1: u32,
    /// Stroke style (1-based, 0 = none).
    pub stroke_style: u32,
    /// Coordinates in twips.
    pub segments: Vec<Segment>,
    original: Option<Original>,
}

impl Edge {
    /// A new edge without original text; it is encoded canonically.
    #[must_use]
    pub const fn new(fill_style0: u32, fill_style1: u32, stroke_style: u32, segments: Vec<Segment>) -> Self {
        Self { fill_style0, fill_style1, stroke_style, segments, original: None }
    }

    /// True when the edge came from a file and its segments have not changed since, so it writes back verbatim.
    #[must_use]
    pub fn is_unedited(&self) -> bool {
        self.unedited_original().is_some()
    }

    /// The `edges` attribute text as read, while the edge is unedited.
    #[must_use]
    pub fn original_edges_text(&self) -> Option<&str> {
        self.unedited_original().and_then(|o| o.edges.as_deref())
    }

    fn unedited_original(&self) -> Option<&Original> {
        self.original.as_ref().filter(|o| o.fingerprint == fingerprint(&self.segments))
    }

    /// Forgets the original text so the edge is encoded canonically.
    pub fn forget_original(&mut self) {
        self.original = None;
    }
}

/// An Animate vector shape: styles plus the planar set of edges that uses them.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EdgeShape {
    /// Fill styles; edge `fill_style0/1` index them 1-based.
    pub fills: Vec<FillStyle>,
    /// Stroke styles; edge `stroke_style` indexes them 1-based.
    pub strokes: Vec<StrokeStyle>,
    pub edges: Vec<Edge>,
}

impl EdgeShape {
    /// A shape without edges but with the given styles.
    #[must_use]
    pub const fn new(fills: Vec<FillStyle>, strokes: Vec<StrokeStyle>) -> Self {
        Self { fills, strokes, edges: Vec::new() }
    }

    /// True when there are no segments at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.edges.iter().all(|e| e.segments.is_empty())
    }

    /// The fill style with the 1-based `index`.
    #[must_use]
    pub fn fill(&self, index: u32) -> Option<&FillStyle> {
        usize::try_from(index).ok()?.checked_sub(1).and_then(|i| self.fills.get(i))
    }

    /// The stroke style with the 1-based `index`.
    #[must_use]
    pub fn stroke(&self, index: u32) -> Option<&StrokeStyle> {
        usize::try_from(index).ok()?.checked_sub(1).and_then(|i| self.strokes.get(i))
    }

    /// Returns the 1-based index of `style`, adding it when the shape does not have an equal one yet.
    pub fn intern_fill(&mut self, style: &FillStyle) -> u32 {
        let position = self.fills.iter().position(|f| f == style).unwrap_or_else(|| {
            self.fills.push(style.clone());
            self.fills.len().saturating_sub(1)
        });
        one_based(position)
    }

    /// Returns the 1-based index of `style`, adding it when the shape does not have an equal one yet.
    pub fn intern_stroke(&mut self, style: &StrokeStyle) -> u32 {
        let position = self.strokes.iter().position(|s| s == style).unwrap_or_else(|| {
            self.strokes.push(style.clone());
            self.strokes.len().saturating_sub(1)
        });
        one_based(position)
    }

    /// Builds a shape from the styles and `<Edge>` elements of a `<DOMShape>`, merging the exact cubic curves of
    /// `cubics` elements into the quadratic edges they approximate.
    ///
    /// # Errors
    /// [`ShapeError`] naming the first element whose `edges` or `cubics` text does not parse.
    pub fn from_xfl(fills: Vec<FillStyle>, strokes: Vec<StrokeStyle>, xfl: &[XflEdge]) -> Result<Self, ShapeError> {
        let mut edges: Vec<Edge> = Vec::with_capacity(xfl.len());
        let mut runs: Vec<CubicRun> = Vec::new();
        for (index, element) in xfl.iter().enumerate() {
            let segments = match &element.edges {
                Some(text) => parse_edges(text).map_err(|source| ShapeError::Edges { index, source })?,
                None => Vec::new(),
            };
            if let Some(text) = &element.cubics {
                runs.extend(parse_cubics(text).map_err(|source| ShapeError::Cubics { index, source })?);
            }
            let unstyled = element.fill_style0 == 0 && element.fill_style1 == 0 && element.stroke_style == 0;
            if let (true, None, Some(text)) = (unstyled, &element.edges, &element.cubics)
                && let Some(original) = edges.last_mut().and_then(|e| e.original.as_mut())
            {
                original.following.push(text.clone());
                continue;
            }
            edges.push(Edge {
                fill_style0: element.fill_style0,
                fill_style1: element.fill_style1,
                stroke_style: element.stroke_style,
                segments,
                original: Some(Original { edges: element.edges.clone(), cubics: element.cubics.clone(), following: Vec::new(), fingerprint: 0 }),
            });
        }
        attach_cubics(&mut edges, &runs);
        for edge in &mut edges {
            let print = fingerprint(&edge.segments);
            if let Some(original) = &mut edge.original {
                original.fingerprint = print;
            }
        }
        Ok(Self { fills, strokes, edges })
    }

    /// The `<Edge>` elements to write for this shape, in order. Unedited edges reproduce their original text and
    /// following `cubics` elements; new or edited edges get canonical text, with a `cubics` element for each exact
    /// cubic segment.
    #[must_use]
    pub fn to_xfl_edges(&self) -> Vec<XflEdge> {
        let mut out = Vec::with_capacity(self.edges.len());
        for edge in &self.edges {
            let styles = (edge.fill_style0, edge.fill_style1, edge.stroke_style);
            if let Some(original) = edge.unedited_original() {
                out.push(XflEdge {
                    fill_style0: styles.0,
                    fill_style1: styles.1,
                    stroke_style: styles.2,
                    edges: original.edges.clone(),
                    cubics: original.cubics.clone(),
                });
                out.extend(original.following.iter().map(|text| XflEdge { cubics: Some(text.clone()), ..XflEdge::default() }));
            } else if !edge.segments.is_empty() {
                out.push(XflEdge {
                    fill_style0: styles.0,
                    fill_style1: styles.1,
                    stroke_style: styles.2,
                    edges: Some(encode_edges(&edge.segments)),
                    cubics: None,
                });
                out.extend(edge.segments.iter().filter_map(encode_cubic).map(|text| XflEdge { cubics: Some(text), ..XflEdge::default() }));
            }
        }
        out
    }

    /// Transforms the geometry and the gradient and bitmap fills (fills move with the shape, as in Animate). `m`
    /// is in pixels; strokes keep their weight.
    pub fn transform(&mut self, m: Affine) {
        let [xx, yx, xy, yy, tx, ty] = m.as_coeffs();
        let twips = Affine::new([xx, yx, xy, yy, tx * TWIPS_PER_PX, ty * TWIPS_PER_PX]);
        for edge in &mut self.edges {
            for segment in &mut edge.segments {
                *segment = segment.map_points(|p| twips * p);
            }
        }
        for fill in &mut self.fills {
            transform_fill(fill, m);
        }
        for stroke in &mut self.strokes {
            transform_fill(&mut stroke.solid_mut().fill, m);
        }
    }
}

fn transform_fill(fill: &mut FillStyle, m: Affine) {
    match fill {
        FillStyle::Solid(_) => {}
        FillStyle::LinearGradient(g) | FillStyle::RadialGradient(g) => g.matrix = m * g.matrix,
        FillStyle::Bitmap(b) => b.matrix = m * b.matrix,
    }
}

fn one_based(position: usize) -> u32 {
    u32::try_from(position.saturating_add(1)).unwrap_or(u32::MAX)
}

const fn grid_key(p: Point) -> (i64, i64) {
    (round_f64_to_i64(p.x), round_f64_to_i64(p.y))
}

/// Replaces runs of quadratic segments by the exact cubic whose approximation they are.
fn attach_cubics(edges: &mut [Edge], runs: &[CubicRun]) {
    if runs.is_empty() {
        return;
    }
    // A cubic may be listed in the direction of the edge or against it, so it is indexed by both of its ends.
    let mut by_start: RunIndex = HashMap::new();
    for (i, run) in runs.iter().enumerate() {
        let (forward, backward) =
            run.approx.first().zip(run.approx.last()).map_or_else(|| (run.cubic.start(), run.cubic.end()), |(first, last)| (first.start(), last.end()));
        by_start.entry(grid_key(forward)).or_default().push((i, false));
        by_start.entry(grid_key(backward)).or_default().push((i, true));
    }
    let mut used = vec![false; runs.len()];
    for edge in edges {
        if edge.segments.is_empty() {
            continue;
        }
        let mut merged = Vec::with_capacity(edge.segments.len());
        let mut position = 0;
        while let Some(tail) = edge.segments.get(position..).filter(|t| !t.is_empty()) {
            if let Some(found) = find_run(tail, runs, &used, &by_start) {
                if let Some(flag) = used.get_mut(found.run) {
                    *flag = true;
                }
                // The cubic keeps its control points but takes the exact end points of the edge it replaces:
                // files round the cubic's end points to whole twips and the edge's neighbours must still meet it.
                if let (Some(Segment::Cubic { ctrl1, ctrl2, .. }), Some(first), Some(last)) =
                    (runs.get(found.run).map(|r| r.cubic), tail.first(), tail.get(found.consumed.saturating_sub(1)))
                {
                    let (ctrl1, ctrl2) = if found.reversed { (ctrl2, ctrl1) } else { (ctrl1, ctrl2) };
                    merged.push(Segment::Cubic { from: first.start(), ctrl1, ctrl2, to: last.end() });
                }
                position = position.saturating_add(found.consumed);
            } else {
                merged.extend(tail.first().copied());
                position = position.saturating_add(1);
            }
        }
        edge.segments = merged;
    }
}

/// Cubic runs by the grid cell of one of their ends: `(run index, listed against the edge's direction)`.
type RunIndex = HashMap<(i64, i64), Vec<(usize, bool)>>;

/// A cubic run found at the start of a tail of segments.
struct Found {
    run: usize,
    /// How many segments of the tail it replaces.
    consumed: usize,
    /// The run is listed in the opposite direction to the edge.
    reversed: bool,
}

fn find_run(tail: &[Segment], runs: &[CubicRun], used: &[bool], by_start: &RunIndex) -> Option<Found> {
    let first = tail.first()?;
    let (kx, ky) = grid_key(first.start());
    for dx in -1..=1_i64 {
        for dy in -1..=1_i64 {
            let key = (kx.saturating_add(dx), ky.saturating_add(dy));
            for &(candidate, reversed) in by_start.get(&key).into_iter().flatten() {
                if used.get(candidate).copied().unwrap_or(true) {
                    continue;
                }
                if let Some(run) = runs.get(candidate)
                    && let Some(consumed) = match_run(tail, run, reversed)
                {
                    return Some(Found { run: candidate, consumed, reversed });
                }
            }
        }
    }
    None
}

/// How many segments at the start of `tail` the run replaces, if it matches them (read against their direction
/// when `reversed`).
fn match_run(tail: &[Segment], run: &CubicRun, reversed: bool) -> Option<usize> {
    if run.approx.is_empty() {
        let first = tail.first()?;
        let (from, to) = if reversed { (run.cubic.end(), run.cubic.start()) } else { (run.cubic.start(), run.cubic.end()) };
        return (first.start().distance(from) <= MATCH_EPS && first.end().distance(to) <= MATCH_EPS).then_some(1);
    }
    if tail.len() < run.approx.len() {
        return None;
    }
    let matches = if reversed {
        tail.iter().zip(run.approx.iter().rev()).all(|(segment, approx)| segment.matches(&approx.reversed(), MATCH_EPS))
    } else {
        tail.iter().zip(&run.approx).all(|(segment, approx)| segment.matches(approx, MATCH_EPS))
    };
    matches.then_some(run.approx.len())
}

#[cfg(test)]
mod tests {
    use anim_core::Rgba8;

    use super::*;

    fn xfl(f0: u32, f1: u32, s: u32, edges: &str) -> XflEdge {
        XflEdge { fill_style0: f0, fill_style1: f1, stroke_style: s, edges: Some(edges.to_owned()), cubics: None }
    }

    fn cubics_only(text: &str) -> XflEdge {
        XflEdge { cubics: Some(text.to_owned()), ..XflEdge::default() }
    }

    const BR_EDGE: &str = "!848 67S1[835 165 1052 184!1052 184[1134 190 1175 174!1175 174[1220 153 1177 116!1177 116[1085 42 987 12!987 12[858 -29 848 67";
    const BR_CUBIC_A: &str = "!848 67(;829,206 1302,223 1177,116q848 67Q835 165q1052 184Q1134 190q1175 174Q1220 153q1177 116);";
    const BR_CUBIC_B: &str = "!1177 116(;1091,47 864,-78 848,67q1177 116Q1085 42q987 12Q858 -29q848 67);";

    #[test]
    fn plain_edges_round_trip_verbatim_when_unedited() {
        let input = vec![xfl(0, 1, 1, BR_EDGE), xfl(1, 0, 0, "\r\n!1 2|3 4")];
        let shape = EdgeShape::from_xfl(vec![FillStyle::Solid(Rgba8::BLACK)], vec![StrokeStyle::default()], &input).unwrap();
        assert_eq!(shape.edges.len(), 2);
        assert!(shape.edges.iter().all(Edge::is_unedited));
        assert_eq!(shape.to_xfl_edges(), input);
    }

    #[test]
    fn editing_an_edge_switches_it_to_canonical_text() {
        let mut shape = EdgeShape::from_xfl(vec![], vec![], &[xfl(0, 1, 0, "\r\n!0 0|10 0")]).unwrap();
        let edge = shape.edges.first_mut().unwrap();
        edge.segments = vec![Segment::Line { from: Point::new(0.0, 0.0), to: Point::new(20.0, 0.0) }];
        assert!(!edge.is_unedited());
        assert_eq!(shape.to_xfl_edges(), vec![xfl(0, 1, 0, "!0 0|20 0")]);
    }

    #[test]
    fn style_changes_do_not_force_canonical_text() {
        let mut shape = EdgeShape::from_xfl(vec![], vec![], &[xfl(0, 1, 0, "\r\n!0 0|10 0")]).unwrap();
        shape.edges.first_mut().unwrap().fill_style1 = 2;
        assert_eq!(shape.to_xfl_edges(), vec![xfl(0, 2, 0, "\r\n!0 0|10 0")]);
    }

    #[test]
    fn cubics_replace_the_quads_they_approximate_and_round_trip() {
        let input = vec![xfl(1, 0, 0, BR_EDGE), cubics_only(BR_CUBIC_A), cubics_only(BR_CUBIC_B)];
        let shape = EdgeShape::from_xfl(vec![], vec![], &input).unwrap();
        assert_eq!(shape.edges.len(), 1);
        let segments = &shape.edges.first().unwrap().segments;
        assert_eq!(segments.len(), 2);
        assert!(segments.iter().all(|s| matches!(s, Segment::Cubic { .. })));
        assert_eq!(shape.to_xfl_edges(), input);
    }

    #[test]
    fn cubics_listed_against_the_edge_direction_merge_reversed() {
        let input = vec![xfl(0, 1, 0, "!0 0[100 50 200 0"), cubics_only("!200 0(;150,60 50,60 0,0q200 0Q100 50q0 0);")];
        let shape = EdgeShape::from_xfl(vec![], vec![], &input).unwrap();
        let segments = &shape.edges.first().unwrap().segments;
        assert_eq!(
            segments,
            &vec![Segment::Cubic { from: Point::new(0.0, 0.0), ctrl1: Point::new(50.0, 60.0), ctrl2: Point::new(150.0, 60.0), to: Point::new(200.0, 0.0) }]
        );
        assert_eq!(shape.to_xfl_edges(), input);
    }

    #[test]
    fn merged_cubics_take_the_exact_vertices_of_their_edge() {
        // The file rounds the cubic's end points to whole twips; the edge has half twips.
        let input = vec![xfl(0, 1, 0, "!0.5 0.5[100 50 200.5 0.5"), cubics_only("!1 1(;50,60 150,60 201,1q1 1Q100 50q201 1);")];
        let shape = EdgeShape::from_xfl(vec![], vec![], &input).unwrap();
        let segment = shape.edges.first().unwrap().segments.first().copied().unwrap();
        assert_eq!(segment.start(), Point::new(0.5, 0.5));
        assert_eq!(segment.end(), Point::new(200.5, 0.5));
        assert!(matches!(segment, Segment::Cubic { .. }));
    }

    #[test]
    fn edited_cubic_edges_write_exact_cubics_that_read_back() {
        let cubic = Segment::Cubic { from: Point::new(0.0, 0.0), ctrl1: Point::new(0.0, 400.0), ctrl2: Point::new(400.0, 400.0), to: Point::new(400.0, 0.0) };
        let line = Segment::Line { from: Point::new(400.0, 0.0), to: Point::new(0.0, 0.0) };
        let mut shape = EdgeShape::default();
        shape.edges.push(Edge::new(0, 1, 0, vec![cubic, line]));
        let written = shape.to_xfl_edges();
        assert_eq!(written.len(), 2);
        let back = EdgeShape::from_xfl(vec![], vec![], &written).unwrap();
        assert_eq!(back.edges.len(), 1);
        let segments = &back.edges.first().unwrap().segments;
        assert_eq!(segments.len(), 2);
        assert_eq!(segments.first(), Some(&cubic));
        assert_eq!(segments.get(1), Some(&line));
        assert_eq!(back.to_xfl_edges(), written);
    }

    #[test]
    fn unmatched_cubics_are_kept_in_the_original_text() {
        let stray = "!5000 5000(;5000,5000 5100,5000 5100,5100q5000 5000 5100 5100);";
        let input = vec![xfl(1, 0, 0, "!0 0|10 0"), cubics_only(stray)];
        let shape = EdgeShape::from_xfl(vec![], vec![], &input).unwrap();
        assert_eq!(shape.edges.first().unwrap().segments.len(), 1);
        assert_eq!(shape.to_xfl_edges(), input);
    }

    #[test]
    fn leading_cubics_element_is_kept() {
        let input = vec![cubics_only("!0 0(;1,1 2,2 3,3q0 0 3 3);"), xfl(1, 0, 0, "!0 0|10 0")];
        let shape = EdgeShape::from_xfl(vec![], vec![], &input).unwrap();
        assert_eq!(shape.edges.len(), 2);
        assert_eq!(shape.to_xfl_edges(), input);
    }

    #[test]
    fn errors_name_the_element() {
        let input = vec![xfl(0, 1, 0, "!0 0|1 1"), xfl(0, 1, 0, "!0 0|zz")];
        assert!(matches!(EdgeShape::from_xfl(vec![], vec![], &input), Err(ShapeError::Edges { index: 1, .. })));
        let input = vec![cubics_only("!0 0(;1,1")];
        assert!(matches!(EdgeShape::from_xfl(vec![], vec![], &input), Err(ShapeError::Cubics { index: 0, .. })));
    }

    #[test]
    fn serde_keeps_the_original_text() {
        let input = vec![xfl(0, 1, 1, "\r\n!1 2|3 4"), xfl(1, 0, 0, BR_EDGE), cubics_only(BR_CUBIC_A)];
        let shape = EdgeShape::from_xfl(vec![FillStyle::default()], vec![StrokeStyle::default()], &input).unwrap();
        let json = serde_json::to_string(&shape).unwrap();
        let back: EdgeShape = serde_json::from_str(&json).unwrap();
        assert_eq!(back, shape);
        assert_eq!(back.to_xfl_edges(), input);
    }

    #[test]
    fn interning_reuses_equal_styles() {
        let mut shape = EdgeShape::default();
        let red = FillStyle::Solid(Rgba8::opaque(255, 0, 0));
        let blue = FillStyle::Solid(Rgba8::opaque(0, 0, 255));
        assert_eq!(shape.intern_fill(&red), 1);
        assert_eq!(shape.intern_fill(&blue), 2);
        assert_eq!(shape.intern_fill(&red), 1);
        assert_eq!(shape.fill(2), Some(&blue));
        assert_eq!(shape.fill(0), None);
        assert_eq!(shape.fill(3), None);
        let stroke = StrokeStyle::plain(2.0, Rgba8::BLACK);
        assert_eq!(shape.intern_stroke(&stroke), 1);
        assert_eq!(shape.intern_stroke(&stroke), 1);
        assert_eq!(shape.stroke(1), Some(&stroke));
    }

    #[test]
    fn transform_moves_geometry_and_fills() {
        let mut shape = EdgeShape::from_xfl(vec![FillStyle::LinearGradient(crate::GradientFill::default())], vec![], &[xfl(0, 1, 0, "!0 0|20 0")]).unwrap();
        shape.transform(Affine::translate((1.0, 2.0)));
        let segment = shape.edges.first().unwrap().segments.first().copied().unwrap();
        assert_eq!(segment, Segment::Line { from: Point::new(20.0, 40.0), to: Point::new(40.0, 40.0) });
        let Some(FillStyle::LinearGradient(g)) = shape.fills.first() else { panic!("gradient expected") };
        assert_eq!(g.matrix, Affine::translate((1.0, 2.0)));
        assert!(!shape.edges.first().unwrap().is_unedited());
        shape.transform(Affine::IDENTITY);
    }
}
