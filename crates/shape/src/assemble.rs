//! From a planar set of edges to paintable paths.
//!
//! For fill style `k`, the edges with `fill_style1 == k` are traversed forward and the edges with `fill_style0 == k`
//! backwards, which orients every boundary of the region the same way. The pieces are chained into closed contours
//! by matching end points (within half a twip), whatever their order in the file. A contour that cannot be
//! continued is closed with a straight line. Strokes are chained too, without orientation, so that joins and caps
//! are drawn once per polyline instead of once per segment.

use std::collections::{BTreeMap, HashMap};

use anim_core::num::round_f64_to_i64;
use anim_core::{BezPath, Point};

use crate::edges::{Edge, EdgeShape};
use crate::segment::{Segment, TWIPS_PER_PX};
use crate::styles::StrokeStyle;

/// End points closer than this many twips are the same vertex.
pub const JOIN_EPS: f64 = 0.51;

/// The area of a style's fill: all closed contours in one path, to be filled with the even-odd rule.
#[derive(Debug, Clone, PartialEq)]
pub struct FillRegion {
    /// 1-based fill style index.
    pub style: u32,
    /// Pixel space.
    pub path: BezPath,
}

/// All polylines drawn with one stroke style.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeRun {
    /// 1-based stroke style index.
    pub style: u32,
    /// Stroke width in pixels (0 = hairline), copied from the style for hit testing.
    pub width: f64,
    /// Pixel space.
    pub path: BezPath,
}

/// The paintable result of [`EdgeShape::assemble`], in painter's order (fills by index, then strokes by index).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Assembled {
    pub fills: Vec<FillRegion>,
    pub strokes: Vec<StrokeRun>,
}

impl EdgeShape {
    /// Chains the edges into fill contours and stroke polylines in pixel space. Styles that no edge uses, and
    /// edges that name a style the shape does not have, produce nothing.
    #[must_use]
    pub fn assemble(&self) -> Assembled {
        let mut fill_pieces: BTreeMap<u32, Vec<Vec<Segment>>> = BTreeMap::new();
        let mut stroke_pieces: BTreeMap<u32, Vec<Vec<Segment>>> = BTreeMap::new();
        for edge in &self.edges {
            collect_pieces(edge, self.fills.len(), self.strokes.len(), &mut fill_pieces, &mut stroke_pieces);
        }
        let fills = fill_pieces
            .into_iter()
            .filter_map(|(style, pieces)| {
                let contours = chain_contours(pieces);
                (!contours.is_empty()).then(|| FillRegion { style, path: contours_to_path(&contours) })
            })
            .collect();
        let strokes = stroke_pieces
            .into_iter()
            .filter_map(|(style, pieces)| {
                let chains = chain_strokes(pieces);
                (!chains.is_empty()).then(|| StrokeRun { style, width: self.stroke(style).map_or(1.0, StrokeStyle::weight), path: chains_to_path(&chains) })
            })
            .collect();
        Assembled { fills, strokes }
    }
}

fn collect_pieces(
    edge: &Edge,
    fill_count: usize,
    stroke_count: usize,
    fills: &mut BTreeMap<u32, Vec<Vec<Segment>>>,
    strokes: &mut BTreeMap<u32, Vec<Vec<Segment>>>,
) {
    let valid = |index: u32, count: usize| index != 0 && usize::try_from(index).is_ok_and(|i| i <= count);
    let pieces = split_continuous(&edge.segments);
    // A fill on both sides is an interior edge: it separates nothing.
    if edge.fill_style0 != edge.fill_style1 {
        if valid(edge.fill_style1, fill_count) {
            fills.entry(edge.fill_style1).or_default().extend(pieces.iter().cloned());
        }
        if valid(edge.fill_style0, fill_count) {
            fills.entry(edge.fill_style0).or_default().extend(pieces.iter().map(|p| reverse_piece(p)));
        }
    }
    if valid(edge.stroke_style, stroke_count) {
        strokes.entry(edge.stroke_style).or_default().extend(pieces);
    }
}

fn reverse_piece(piece: &[Segment]) -> Vec<Segment> {
    piece.iter().rev().map(|s| s.reversed()).collect()
}

/// Splits a segment list where one segment does not start where the previous one ended.
fn split_continuous(segments: &[Segment]) -> Vec<Vec<Segment>> {
    let mut pieces: Vec<Vec<Segment>> = Vec::new();
    for segment in segments {
        let continues = pieces.last().and_then(|p| p.last()).is_some_and(|prev| prev.end().distance(segment.start()) <= JOIN_EPS);
        match pieces.last_mut() {
            Some(piece) if continues => piece.push(*segment),
            _ => pieces.push(vec![*segment]),
        }
    }
    pieces
}

/// A spatial hash over points with one-twip cells: everything within [`JOIN_EPS`] of a point is in the 3x3 cells
/// around it.
#[derive(Default)]
struct Grid {
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl Grid {
    const fn key(p: Point) -> (i64, i64) {
        (round_f64_to_i64(p.x.floor()), round_f64_to_i64(p.y.floor()))
    }

    fn insert(&mut self, p: Point, id: usize) {
        self.cells.entry(Self::key(p)).or_default().push(id);
    }

    /// Ids stored in the cells around `p` (a superset of those within [`JOIN_EPS`]).
    fn near(&self, p: Point) -> impl Iterator<Item = usize> + '_ {
        let (kx, ky) = Self::key(p);
        (-1..=1_i64)
            .flat_map(move |dx| (-1..=1_i64).map(move |dy| (kx.saturating_add(dx), ky.saturating_add(dy))))
            .filter_map(|key| self.cells.get(&key))
            .flatten()
            .copied()
    }
}

/// Chains oriented pieces head to tail into closed contours.
fn chain_contours(pieces: Vec<Vec<Segment>>) -> Vec<Vec<Segment>> {
    let starts: Vec<Option<Point>> = pieces.iter().map(|p| p.first().map(Segment::start)).collect();
    let mut grid = Grid::default();
    for (id, start) in starts.iter().enumerate() {
        if let Some(start) = start {
            grid.insert(*start, id);
        }
    }
    let mut pieces: Vec<Option<Vec<Segment>>> = pieces.into_iter().map(Some).collect();
    let mut contours = Vec::new();
    for id in 0..pieces.len() {
        let Some(mut contour) = pieces.get_mut(id).and_then(Option::take) else { continue };
        let Some(origin) = contour.first().map(Segment::start) else { continue };
        while let Some(last) = contour.last() {
            if last.end().distance(origin) <= JOIN_EPS {
                break;
            }
            let Some(next) = best_continuation(last, &pieces, &starts, &grid) else { break };
            match pieces.get_mut(next).and_then(Option::take) {
                Some(piece) => contour.extend(piece),
                None => break,
            }
        }
        if !is_degenerate_contour(&contour) {
            contours.push(contour);
        }
    }
    contours
}

/// Among unused pieces starting at the end of `last`, the one that turns sharpest towards the filled side. With
/// the fill on the right of travel (and y pointing down) that is the largest clockwise turn, which keeps two
/// regions that merely touch at a vertex apart.
fn best_continuation(last: &Segment, pieces: &[Option<Vec<Segment>>], starts: &[Option<Point>], grid: &Grid) -> Option<usize> {
    let end = last.end();
    let incoming = last.end_tangent();
    let mut best: Option<(f64, usize)> = None;
    for id in grid.near(end) {
        let Some(Some(piece)) = pieces.get(id) else { continue };
        let Some(Some(start)) = starts.get(id) else { continue };
        if start.distance(end) > JOIN_EPS {
            continue;
        }
        let outgoing = piece.first().map_or(incoming, Segment::start_tangent);
        let turn = incoming.cross(outgoing).atan2(incoming.dot(outgoing));
        let better = match best {
            None => true,
            Some((best_turn, best_id)) => turn > best_turn + 1e-9 || ((turn - best_turn).abs() <= 1e-9 && id < best_id),
        };
        if better {
            best = Some((turn, id));
        }
    }
    best.map(|(_, id)| id)
}

/// A contour made only of zero-length pieces paints nothing.
fn is_degenerate_contour(contour: &[Segment]) -> bool {
    contour.iter().all(|s| s.is_degenerate(JOIN_EPS))
}

fn px(p: Point) -> Point {
    Point::new(p.x / TWIPS_PER_PX, p.y / TWIPS_PER_PX)
}

fn extend_path(path: &mut BezPath, segment: &Segment) {
    match *segment {
        Segment::Line { to, .. } => path.line_to(px(to)),
        Segment::Quad { ctrl, to, .. } => path.quad_to(px(ctrl), px(to)),
        Segment::Cubic { ctrl1, ctrl2, to, .. } => path.curve_to(px(ctrl1), px(ctrl2), px(to)),
    }
}

fn contours_to_path(contours: &[Vec<Segment>]) -> BezPath {
    let mut path = BezPath::new();
    for contour in contours {
        let Some(first) = contour.first() else { continue };
        path.move_to(px(first.start()));
        for segment in contour {
            extend_path(&mut path, segment);
        }
        path.close_path();
    }
    path
}

fn chains_to_path(chains: &[Chain]) -> BezPath {
    let mut path = BezPath::new();
    for chain in chains {
        let Some(first) = chain.segments.first() else { continue };
        path.move_to(px(first.start()));
        for segment in &chain.segments {
            extend_path(&mut path, segment);
        }
        if chain.closed {
            path.close_path();
        }
    }
    path
}

/// A stroke polyline.
struct Chain {
    segments: Vec<Segment>,
    closed: bool,
}

/// The pieces of one stroke style as a graph: nodes are vertices, pieces are edges between two of them.
struct StrokeGraph {
    pieces: Vec<Vec<Segment>>,
    /// Vertex at the start and at the end of each piece.
    ends: Vec<(usize, usize)>,
    /// Per vertex, the piece ends that meet there, as `(piece, is_end)`.
    incident: Vec<Vec<(usize, bool)>>,
    visited: Vec<bool>,
}

/// The id of the vertex at `p`, creating it when no existing vertex is within [`JOIN_EPS`].
fn vertex_at(p: Point, points: &mut Vec<Point>, grid: &mut Grid) -> usize {
    if let Some(found) = grid.near(p).find(|id| points.get(*id).is_some_and(|q| q.distance(p) <= JOIN_EPS)) {
        return found;
    }
    points.push(p);
    let id = points.len().saturating_sub(1);
    grid.insert(p, id);
    id
}

impl StrokeGraph {
    fn new(pieces: Vec<Vec<Segment>>) -> Self {
        let mut points: Vec<Point> = Vec::new();
        let mut grid = Grid::default();
        let mut ends = Vec::with_capacity(pieces.len());
        for piece in &pieces {
            let start = piece.first().map_or(Point::ZERO, Segment::start);
            let end = piece.last().map_or(start, Segment::end);
            let a = vertex_at(start, &mut points, &mut grid);
            let b = vertex_at(end, &mut points, &mut grid);
            ends.push((a, b));
        }
        let mut incident: Vec<Vec<(usize, bool)>> = vec![Vec::new(); points.len()];
        for (id, (a, b)) in ends.iter().enumerate() {
            if let Some(list) = incident.get_mut(*a) {
                list.push((id, false));
            }
            if let Some(list) = incident.get_mut(*b) {
                list.push((id, true));
            }
        }
        let visited = vec![false; pieces.len()];
        Self { pieces, ends, incident, visited }
    }

    /// Takes an unvisited piece, reversed or not, and returns its segments with the vertex it arrives at.
    fn take(&mut self, id: usize, reversed: bool) -> Option<(Vec<Segment>, usize)> {
        let flag = self.visited.get_mut(id)?;
        if *flag {
            return None;
        }
        *flag = true;
        let piece = self.pieces.get(id)?;
        let (a, b) = *self.ends.get(id)?;
        if reversed { Some((reverse_piece(piece), a)) } else { Some((piece.clone(), b)) }
    }

    fn degree(&self, vertex: usize) -> usize {
        self.incident.get(vertex).map_or(0, Vec::len)
    }

    /// Follows a chain from `first`, continuing through vertices where exactly two piece ends meet.
    fn walk(&mut self, first: usize, reversed: bool) -> Option<Chain> {
        let origin = {
            let (a, b) = *self.ends.get(first)?;
            if reversed { b } else { a }
        };
        let (mut segments, mut vertex) = self.take(first, reversed)?;
        while self.degree(vertex) == 2 {
            let next = self.incident.get(vertex).and_then(|list| list.iter().find(|(id, _)| self.visited.get(*id).is_some_and(|v| !v)).copied());
            // A piece met at its end is walked backwards.
            let Some((id, met_at_end)) = next else { break };
            let Some((piece, arrived)) = self.take(id, met_at_end) else { break };
            segments.extend(piece);
            vertex = arrived;
        }
        Some(Chain { segments, closed: vertex == origin })
    }

    fn into_chains(mut self) -> Vec<Chain> {
        let mut chains = Vec::new();
        // Open ends and junctions first, so that open polylines start at their ends.
        for vertex in 0..self.incident.len() {
            if self.degree(vertex) == 2 {
                continue;
            }
            let list = self.incident.get(vertex).cloned().unwrap_or_default();
            for (id, is_end) in list {
                chains.extend(self.walk(id, is_end));
            }
        }
        // What is left are closed loops.
        for id in 0..self.pieces.len() {
            chains.extend(self.walk(id, false));
        }
        chains
    }
}

fn chain_strokes(pieces: Vec<Vec<Segment>>) -> Vec<Chain> {
    StrokeGraph::new(pieces).into_chains()
}

#[cfg(test)]
mod tests {
    use anim_core::Rgba8;
    use anim_core::kurbo::Shape;

    use super::*;
    use crate::codec::parse_edges;
    use crate::styles::{FillStyle, StrokeStyle};

    fn edge(f0: u32, f1: u32, s: u32, text: &str) -> Edge {
        Edge::new(f0, f1, s, parse_edges(text).unwrap())
    }

    fn shape(fills: usize, strokes: usize, edges: Vec<Edge>) -> EdgeShape {
        let fills = (0..fills).map(|i| FillStyle::Solid(Rgba8::opaque(u8::try_from(i).unwrap().saturating_mul(40), 0, 0))).collect();
        let strokes = (0..strokes).map(|_| StrokeStyle::plain(1.0, Rgba8::BLACK)).collect();
        EdgeShape { fills, strokes, edges }
    }

    fn area(path: &BezPath) -> f64 {
        path.area().abs()
    }

    #[test]
    fn a_square_becomes_one_closed_contour_in_pixels() {
        let s = shape(1, 0, vec![edge(0, 1, 0, "!0 0|200 0|200 200|0 200|0 0")]);
        let a = s.assemble();
        assert_eq!(a.fills.len(), 1);
        assert_eq!(a.strokes.len(), 0);
        let region = a.fills.first().unwrap();
        assert_eq!(region.style, 1);
        assert!((area(&region.path) - 100.0).abs() < 1e-9);
        let bounds = region.path.bounding_box();
        assert!((bounds.width() - 10.0).abs() < 1e-9 && (bounds.height() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn fill_on_the_left_is_reversed_but_encloses_the_same_area() {
        let s = shape(1, 0, vec![edge(1, 0, 0, "!0 0|0 200|200 200|200 0|0 0")]);
        let a = s.assemble();
        assert!((area(&a.fills.first().unwrap().path) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn edge_order_and_split_edges_do_not_matter() {
        let pieces = vec![edge(0, 1, 0, "!200 200|0 200"), edge(0, 1, 0, "!0 0|200 0"), edge(0, 1, 0, "!0 200|0 0"), edge(0, 1, 0, "!200 0|200 200")];
        let s = shape(1, 0, pieces);
        let a = s.assemble();
        assert_eq!(a.fills.len(), 1);
        assert_eq!(a.fills.first().unwrap().path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 1);
        assert!((area(&a.fills.first().unwrap().path) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn missing_closure_is_closed_with_a_line() {
        let s = shape(1, 0, vec![edge(0, 1, 0, "!0 0|200 0|200 200|0 200")]);
        let a = s.assemble();
        assert!((area(&a.fills.first().unwrap().path) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn tolerates_sub_twip_gaps() {
        let s = shape(1, 0, vec![edge(0, 1, 0, "!0 0|200 0!200.25 0.25|200 200|0 200|0.25 0.25")]);
        let a = s.assemble();
        assert_eq!(a.fills.first().unwrap().path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 1);
    }

    #[test]
    fn a_donut_has_an_outer_and_an_inner_contour() {
        // Fill 1 is outside the hole; its outer boundary has the fill inside (right), the hole boundary has it outside.
        let outer = edge(0, 1, 0, "!0 0|400 0|400 400|0 400|0 0");
        let hole = edge(0, 1, 0, "!100 100|100 300|300 300|300 100|100 100");
        let s = shape(1, 0, vec![outer, hole]);
        let a = s.assemble();
        let region = a.fills.first().unwrap();
        assert_eq!(region.path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 2);
        // Even-odd: outer 20x20 px minus inner 10x10 px.
        assert_eq!(region.path.winding(Point::new(2.0, 2.0)) & 1, 1);
        assert_eq!(region.path.winding(Point::new(10.0, 10.0)) & 1, 0);
    }

    #[test]
    fn two_squares_sharing_an_edge_give_each_style_its_own_region() {
        // Left square (style 1) and right square (style 2) share the vertical edge x = 200. Walking it downwards
        // the left square is on the right-hand side (west) and the right square on the left-hand side (east).
        let edges = vec![
            edge(0, 1, 0, "!0 0|200 0"),
            edge(0, 1, 0, "!200 200|0 200|0 0"),
            edge(2, 1, 0, "!200 0|200 200"),
            edge(2, 0, 0, "!200 200|400 200|400 0|200 0"),
        ];
        let s = shape(2, 0, edges);
        let a = s.assemble();
        assert_eq!(a.fills.len(), 2);
        for region in &a.fills {
            assert!((area(&region.path) - 100.0).abs() < 1e-9, "style {} area {}", region.style, area(&region.path));
        }
        let left = a.fills.iter().find(|r| r.style == 1).unwrap();
        let right = a.fills.iter().find(|r| r.style == 2).unwrap();
        assert_eq!(left.path.winding(Point::new(5.0, 5.0)) & 1, 1);
        assert_eq!(left.path.winding(Point::new(15.0, 5.0)) & 1, 0);
        assert_eq!(right.path.winding(Point::new(15.0, 5.0)) & 1, 1);
    }

    #[test]
    fn regions_touching_at_one_vertex_stay_separate() {
        // Two squares of the same style meeting at (0,0): upper-left and lower-right, both clockwise on screen and
        // both split at that vertex, so that chaining reaches it with two candidates to continue on.
        let a1 = edge(0, 1, 0, "!-200 0|-200 -200|0 -200|0 0");
        let a2 = edge(0, 1, 0, "!0 0|-200 0");
        let b1 = edge(0, 1, 0, "!0 0|200 0|200 200|0 200");
        let b2 = edge(0, 1, 0, "!0 200|0 0");
        let s = shape(1, 0, vec![a1, a2, b1, b2]);
        let assembled = s.assemble();
        let region = assembled.fills.first().unwrap();
        assert_eq!(region.path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 2);
        assert!((area(&region.path) - 200.0).abs() < 1e-9);
    }

    #[test]
    fn interior_edges_with_the_same_fill_on_both_sides_are_ignored() {
        let s = shape(1, 0, vec![edge(1, 1, 0, "!0 0|200 0|200 200|0 200|0 0")]);
        assert_eq!(s.assemble().fills.len(), 0);
    }

    #[test]
    fn unknown_styles_are_skipped() {
        let s = shape(1, 1, vec![edge(0, 5, 7, "!0 0|200 0|200 200|0 200|0 0")]);
        let a = s.assemble();
        assert!(a.fills.is_empty() && a.strokes.is_empty());
    }

    #[test]
    fn curves_are_kept_as_curves() {
        let s = shape(1, 0, vec![edge(0, 1, 0, "!0 0[200 -200 400 0|0 0")]);
        let region = s.assemble().fills.into_iter().next().unwrap();
        assert!(region.path.elements().iter().any(|e| matches!(e, anim_core::kurbo::PathEl::QuadTo(..))));
    }

    #[test]
    fn strokes_chain_into_polylines_with_joins() {
        // Three pieces in scrambled order and direction forming an open polyline.
        let s = shape(0, 1, vec![edge(0, 0, 1, "!200 0|200 200"), edge(0, 0, 1, "!200 0|0 0"), edge(0, 0, 1, "!200 200|400 200")]);
        let a = s.assemble();
        assert_eq!(a.strokes.len(), 1);
        let run = a.strokes.first().unwrap();
        assert_eq!(run.path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 1);
        assert_eq!(run.path.segments().count(), 3);
        assert!(!run.path.elements().iter().any(|e| matches!(e, anim_core::kurbo::PathEl::ClosePath)));
    }

    #[test]
    fn closed_stroke_loops_are_closed() {
        let s = shape(0, 1, vec![edge(0, 0, 1, "!0 0|200 0|200 200"), edge(0, 0, 1, "!0 0|0 200|200 200")]);
        let a = s.assemble();
        let run = a.strokes.first().unwrap();
        assert_eq!(run.path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 1);
        assert!(run.path.elements().iter().any(|e| matches!(e, anim_core::kurbo::PathEl::ClosePath)));
        assert_eq!(run.path.segments().filter(|s| !matches!(s, anim_core::kurbo::PathSeg::Line(l) if l.p0 == l.p1)).count(), 4);
    }

    #[test]
    fn t_junctions_split_strokes() {
        // A "T": the stem ends on the middle of the bar, which is two pieces meeting at the junction.
        let s = shape(0, 1, vec![edge(0, 0, 1, "!0 0|200 0"), edge(0, 0, 1, "!200 0|400 0"), edge(0, 0, 1, "!200 0|200 200")]);
        let a = s.assemble();
        let run = a.strokes.first().unwrap();
        assert_eq!(run.path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 3);
    }

    #[test]
    fn stroke_width_comes_from_the_style() {
        let mut s = shape(0, 1, vec![edge(0, 0, 1, "!0 0|200 0")]);
        s.strokes = vec![StrokeStyle::plain(2.5, Rgba8::BLACK)];
        assert!((s.assemble().strokes.first().unwrap().width - 2.5).abs() < 1e-12);
    }

    #[test]
    fn disconnected_pieces_inside_one_edge_are_separate_polylines() {
        let s = shape(0, 1, vec![edge(0, 0, 1, "!0 0|100 0!500 500|600 500")]);
        let a = s.assemble();
        assert_eq!(a.strokes.first().unwrap().path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 2);
    }
}
