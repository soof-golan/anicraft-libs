//! Silhouette triangulation: outline polygons to a constrained Delaunay mesh with interior points.

use anim_core::{Point, Rect, num};
use serde::{Deserialize, Serialize};
use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

/// Largest number of vertices a mesh may get (guards against absurd `max_edge` values).
pub const MAX_VERTICES: usize = 200_000;
/// Coordinates beyond this magnitude are rejected.
const MAX_COORD: f64 = 1.0e9;

/// A triangle mesh. Triangles index into `vertices`; they are counter-clockwise in a y-up frame.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub vertices: Vec<Point>,
    pub triangles: Vec<[u32; 3]>,
}

/// A closed region: one outer ring and any number of holes. Rings are implicitly closed, either winding.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Outline {
    pub outer: Vec<Point>,
    pub holes: Vec<Vec<Point>>,
}

/// Meshing parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshOptions {
    /// Target maximum edge length. Boundary edges are subdivided to at most this length and interior points are
    /// placed on a hexagonal grid with this spacing. Zero or non-finite means "boundary vertices only".
    pub max_edge: f64,
}

impl Default for MeshOptions {
    fn default() -> Self {
        Self { max_edge: 0.0 }
    }
}

/// Why meshing failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MeshError {
    #[error("the outer ring needs at least 3 distinct points")]
    DegenerateOuter,
    #[error("a coordinate is not finite or exceeds the supported range")]
    BadCoordinate,
    #[error("the mesh would need more than {MAX_VERTICES} vertices; raise max_edge")]
    TooManyVertices,
    #[error("triangulation failed: {0}")]
    Triangulation(String),
}

impl Mesh {
    /// Triangulates `outline`.
    ///
    /// # Errors
    /// Fails for degenerate rings, non-finite or huge coordinates, or when `max_edge` implies too many vertices.
    pub fn from_outline(outline: &Outline, options: &MeshOptions) -> Result<Self, MeshError> {
        build(outline, *options)
    }

    /// Bounding box of all vertices, or `None` when empty.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        let mut it = self.vertices.iter();
        let first = it.next()?;
        let mut r = Rect::from_points(*first, *first);
        for p in it {
            r = r.union_pt(*p);
        }
        Some(r)
    }

    /// Unique undirected edges, sorted, as `(low, high)` vertex ids.
    #[must_use]
    pub fn edges(&self) -> Vec<(u32, u32)> {
        let mut e: Vec<(u32, u32)> = Vec::with_capacity(self.triangles.len().saturating_mul(3));
        for &[a, b, c] in &self.triangles {
            for (u, v) in [(a, b), (b, c), (c, a)] {
                e.push((u.min(v), u.max(v)));
            }
        }
        e.sort_unstable();
        e.dedup();
        e
    }

    /// Longest edge length (0 for an empty mesh).
    #[must_use]
    pub fn max_edge_length(&self) -> f64 {
        self.edges()
            .iter()
            .filter_map(|&(a, b)| {
                let pa = self.vertex(a)?;
                let pb = self.vertex(b)?;
                Some(pa.distance(pb))
            })
            .fold(0.0, f64::max)
    }

    /// Vertex by id.
    #[must_use]
    pub fn vertex(&self, id: u32) -> Option<Point> {
        self.vertices.get(num::u32_to_usize(id)).copied()
    }

    /// Triangle corners by triangle id.
    #[must_use]
    pub fn triangle_points(&self, tri: u32) -> Option<[Point; 3]> {
        let [a, b, c] = *self.triangles.get(num::u32_to_usize(tri))?;
        Some([self.vertex(a)?, self.vertex(b)?, self.vertex(c)?])
    }

    /// Whether every triangle index is in range.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let n = self.vertices.len();
        self.triangles.iter().all(|t| t.iter().all(|&i| num::u32_to_usize(i) < n))
    }

    /// Total (unsigned) area.
    #[must_use]
    pub fn area(&self) -> f64 {
        (0..self.triangles.len()).filter_map(|i| self.triangle_points(num::usize_to_u32_sat(i))).map(|[a, b, c]| tri_area2(a, b, c).abs() * 0.5).sum()
    }
}

pub(crate) fn tri_area2(a: Point, b: Point, c: Point) -> f64 {
    (b - a).cross(c - a)
}

fn finite_ok(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD
}

/// Drops consecutive duplicates and a repeated closing point.
fn clean_ring(ring: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(ring.len());
    for &p in ring {
        if out.last().is_none_or(|&l| l != p) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// Even-odd point-in-rings test.
fn inside(rings: &[Vec<Point>], p: Point) -> bool {
    let mut odd = false;
    for ring in rings {
        let Some(&last) = ring.last() else { continue };
        let mut prev = last;
        for &cur in ring {
            if (cur.y > p.y) != (prev.y > p.y) {
                let t = (p.y - cur.y) / (prev.y - cur.y);
                let x = cur.x + t * (prev.x - cur.x);
                if p.x < x {
                    odd = !odd;
                }
            }
            prev = cur;
        }
    }
    odd
}

fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.hypot2();
    if len2 <= 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn dist_to_rings(rings: &[Vec<Point>], p: Point) -> f64 {
    let mut best = f64::INFINITY;
    for ring in rings {
        let Some(&last) = ring.last() else { continue };
        let mut prev = last;
        for &cur in ring {
            best = best.min(dist_to_segment(p, prev, cur));
            prev = cur;
        }
    }
    best
}

fn ring_perimeter(ring: &[Point]) -> f64 {
    let Some(&last) = ring.last() else { return 0.0 };
    let mut prev = last;
    let mut sum = 0.0;
    for &cur in ring {
        sum += prev.distance(cur);
        prev = cur;
    }
    sum
}

fn build(outline: &Outline, options: MeshOptions) -> Result<Mesh, MeshError> {
    let outer = clean_ring(&outline.outer);
    if outer.len() < 3 {
        return Err(MeshError::DegenerateOuter);
    }
    let mut rings: Vec<Vec<Point>> = vec![outer];
    for h in &outline.holes {
        let h = clean_ring(h);
        if h.len() >= 3 {
            rings.push(h);
        }
    }
    if rings.iter().flatten().any(|&p| !finite_ok(p)) {
        return Err(MeshError::BadCoordinate);
    }

    let h = options.max_edge;
    let spacing = (h.is_finite() && h > 0.0).then_some(h);

    // Size guard before allocating anything.
    if let Some(h) = spacing {
        let perimeter: f64 = rings.iter().map(|r| ring_perimeter(r)).sum();
        let bb = rings.first().and_then(|r| bounds_of(r));
        let area = bb.map_or(0.0, |b| b.width() * b.height());
        let est = perimeter / h + area / (h * h * 0.8);
        if !est.is_finite() || est > num::usize_to_f64(MAX_VERTICES) {
            return Err(MeshError::TooManyVertices);
        }
    }

    let mut cdt = Cdt::new();
    add_boundary(&mut cdt, &rings, spacing)?;
    if let Some(h) = spacing {
        add_interior(&mut cdt, &rings, h)?;
        refine(&mut cdt, &rings, h)?;
    }

    let mut vertices = Vec::with_capacity(cdt.num_vertices());
    for v in cdt.vertices() {
        let p = v.position();
        vertices.push(Point::new(p.x, p.y));
    }
    let mut triangles = Vec::new();
    for f in cdt.inner_faces() {
        let [a, b, c] = f.vertices();
        let ids = [a.fix().index(), b.fix().index(), c.fix().index()].map(num::usize_to_u32_sat);
        let (Some(pa), Some(pb), Some(pc)) = (vertices.get(a.fix().index()), vertices.get(b.fix().index()), vertices.get(c.fix().index())) else {
            continue;
        };
        let centroid = Point::new((pa.x + pb.x + pc.x) / 3.0, (pa.y + pb.y + pc.y) / 3.0);
        if inside(&rings, centroid) {
            triangles.push(ids);
        }
    }
    let mut mesh = Mesh { vertices, triangles };
    compact(&mut mesh);
    Ok(mesh)
}

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

fn insert(cdt: &mut Cdt, p: Point) -> Result<FixedVertexHandle, MeshError> {
    cdt.insert(Point2::new(p.x, p.y)).map_err(|e| MeshError::Triangulation(format!("{e:?}")))
}

/// Subdivided rings with constraint edges between consecutive points.
fn add_boundary(cdt: &mut Cdt, rings: &[Vec<Point>], spacing: Option<f64>) -> Result<(), MeshError> {
    for ring in rings {
        let mut handles = Vec::new();
        let mut prev = ring.last().copied();
        for &cur in ring {
            let Some(p0) = prev else { break };
            let segs = spacing.map_or(1, |h| num::round_f64_to_u32((p0.distance(cur) / h).ceil()).max(1));
            // Points of the edge prev -> cur, excluding the start (it was added by the previous edge).
            for s in 1..=segs {
                let t = f64::from(s) / f64::from(segs);
                let p = if s == segs { cur } else { p0.lerp(cur, t) };
                handles.push(insert(cdt, p)?);
            }
            prev = Some(cur);
        }
        for (i, &a) in handles.iter().enumerate() {
            let Some(&b) = handles.get(i.saturating_add(1)).or_else(|| handles.first()) else { continue };
            if a != b && cdt.can_add_constraint(a, b) {
                cdt.add_constraint(a, b);
            }
        }
    }
    Ok(())
}

/// Interior points on a hexagonal grid, kept away from the boundary.
fn add_interior(cdt: &mut Cdt, rings: &[Vec<Point>], h: f64) -> Result<(), MeshError> {
    let Some(bb) = rings.first().and_then(|r| bounds_of(r)) else { return Ok(()) };
    let row_h = h * 0.866_025_403_784_438_6;
    let rows = num::round_f64_to_u32((bb.height() / row_h).ceil()).saturating_add(1);
    let cols = num::round_f64_to_u32((bb.width() / h).ceil()).saturating_add(1);
    let gap = h * 0.5;
    for r in 0..rows {
        let y = bb.y0 + f64::from(r) * row_h;
        let off = if r % 2 == 1 { h * 0.5 } else { 0.0 };
        for c in 0..cols {
            let p = Point::new(bb.x0 + f64::from(c) * h + off, y);
            if inside(rings, p) && dist_to_rings(rings, p) >= gap {
                insert(cdt, p)?;
            }
        }
    }
    Ok(())
}

/// Splits the remaining long edges (typically between boundary and interior points) at their midpoints.
fn refine(cdt: &mut Cdt, rings: &[Vec<Point>], h: f64) -> Result<(), MeshError> {
    let limit = h * 1.05;
    for _ in 0..64 {
        let mids: Vec<Point> = cdt
            .undirected_edges()
            .filter_map(|e| {
                let [a, b] = e.positions();
                let (a, b) = (Point::new(a.x, a.y), Point::new(b.x, b.y));
                let mid = a.midpoint(b);
                (a.distance(b) > limit && inside(rings, mid)).then_some(mid)
            })
            .collect();
        if mids.is_empty() {
            break;
        }
        if cdt.num_vertices().saturating_add(mids.len()) > MAX_VERTICES {
            return Err(MeshError::TooManyVertices);
        }
        for m in mids {
            insert(cdt, m)?;
        }
    }
    Ok(())
}

fn bounds_of(ring: &[Point]) -> Option<Rect> {
    let mut it = ring.iter();
    let first = *it.next()?;
    Some(it.fold(Rect::from_points(first, first), |r, &p| r.union_pt(p)))
}

/// Removes vertices no triangle uses (outside the region) and renumbers.
fn compact(mesh: &mut Mesh) {
    let n = mesh.vertices.len();
    let mut used = vec![false; n];
    for t in &mesh.triangles {
        for &i in t {
            if let Some(u) = used.get_mut(num::u32_to_usize(i)) {
                *u = true;
            }
        }
    }
    let mut remap = vec![u32::MAX; n];
    let mut kept = Vec::with_capacity(n);
    for (i, p) in mesh.vertices.iter().enumerate() {
        if used.get(i).copied().unwrap_or(false) {
            if let Some(r) = remap.get_mut(i) {
                *r = num::usize_to_u32_sat(kept.len());
            }
            kept.push(*p);
        }
    }
    for t in &mut mesh.triangles {
        *t = t.map(|i| remap.get(num::u32_to_usize(i)).copied().unwrap_or(0));
    }
    mesh.vertices = kept;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f64) -> Vec<Point> {
        vec![Point::new(0.0, 0.0), Point::new(s, 0.0), Point::new(s, s), Point::new(0.0, s)]
    }

    #[test]
    fn square_without_interior() {
        let m = Mesh::from_outline(&Outline { outer: square(10.0), holes: vec![] }, &MeshOptions::default()).unwrap();
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(m.triangles.len(), 2);
        assert!((m.area() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn max_edge_controls_density() {
        let o = Outline { outer: square(100.0), holes: vec![] };
        let coarse = Mesh::from_outline(&o, &MeshOptions { max_edge: 50.0 }).unwrap();
        let fine = Mesh::from_outline(&o, &MeshOptions { max_edge: 10.0 }).unwrap();
        assert!(fine.vertices.len() > coarse.vertices.len() * 4);
        assert!(fine.is_valid());
        assert!((fine.area() - 10_000.0).abs() < 1e-6);
        assert!(fine.max_edge_length() <= 10.0 * 1.05 + 1e-9, "{}", fine.max_edge_length());
        assert!(coarse.max_edge_length() <= 50.0 * 1.05 + 1e-9, "{}", coarse.max_edge_length());
    }

    #[test]
    fn hole_is_excluded() {
        let hole: Vec<Point> = square(20.0).into_iter().map(|p| Point::new(p.x + 40.0, p.y + 40.0)).collect();
        let o = Outline { outer: square(100.0), holes: vec![hole] };
        let m = Mesh::from_outline(&o, &MeshOptions { max_edge: 8.0 }).unwrap();
        assert!((m.area() - (10_000.0 - 400.0)).abs() < 1e-6, "{}", m.area());
        // No vertex strictly inside the hole.
        assert!(m.vertices.iter().all(|p| !(p.x > 40.0 + 1e-9 && p.x < 60.0 - 1e-9 && p.y > 40.0 + 1e-9 && p.y < 60.0 - 1e-9)));
    }

    #[test]
    fn concave_outline_keeps_notch_empty() {
        // L shape: area 3 * 100.
        let l =
            vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0), Point::new(20.0, 10.0), Point::new(10.0, 10.0), Point::new(10.0, 20.0), Point::new(0.0, 20.0)];
        let m = Mesh::from_outline(&Outline { outer: l, holes: vec![] }, &MeshOptions { max_edge: 4.0 }).unwrap();
        assert!((m.area() - 300.0).abs() < 1e-6, "{}", m.area());
    }

    #[test]
    fn hostile_input_is_rejected() {
        let bad = Outline { outer: vec![Point::new(f64::NAN, 0.0), Point::new(1.0, 0.0), Point::new(0.0, 1.0)], holes: vec![] };
        assert_eq!(Mesh::from_outline(&bad, &MeshOptions::default()), Err(MeshError::BadCoordinate));
        let two = Outline { outer: vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)], holes: vec![] };
        assert_eq!(Mesh::from_outline(&two, &MeshOptions::default()), Err(MeshError::DegenerateOuter));
        let huge = Outline { outer: square(1.0e8), holes: vec![] };
        assert_eq!(Mesh::from_outline(&huge, &MeshOptions { max_edge: 0.001 }), Err(MeshError::TooManyVertices));
    }

    #[test]
    fn self_intersecting_ring_does_not_panic() {
        let bow = Outline { outer: vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0), Point::new(10.0, 0.0), Point::new(0.0, 10.0)], holes: vec![] };
        let _ = Mesh::from_outline(&bow, &MeshOptions { max_edge: 3.0 });
    }

    proptest::proptest! {
        #[test]
        fn random_rings_never_panic(
            pts in proptest::collection::vec((-1.0e3f64..1.0e3, -1.0e3f64..1.0e3), 0..12),
            hole in proptest::collection::vec((-1.0e3f64..1.0e3, -1.0e3f64..1.0e3), 0..6),
            edge in 0.0f64..400.0,
        ) {
            let ring = |v: &[(f64, f64)]| v.iter().map(|&(x, y)| Point::new(x, y)).collect::<Vec<_>>();
            let o = Outline { outer: ring(&pts), holes: vec![ring(&hole)] };
            if let Ok(m) = Mesh::from_outline(&o, &MeshOptions { max_edge: edge }) {
                proptest::prop_assert!(m.is_valid());
            }
        }
    }

    #[test]
    fn deterministic() {
        let o = Outline { outer: square(37.0), holes: vec![] };
        let a = Mesh::from_outline(&o, &MeshOptions { max_edge: 5.0 }).unwrap();
        let b = Mesh::from_outline(&o, &MeshOptions { max_edge: 5.0 }).unwrap();
        assert_eq!(a, b);
    }
}
