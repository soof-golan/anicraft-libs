//! Binding user handles (arbitrary points) to a mesh and turning handle targets into vertex pins for the solver.

use std::collections::BTreeMap;

use anim_core::{Point, Vec2, num};
use serde::{Deserialize, Serialize};

use crate::mesh::{Mesh, tri_area2};

/// How a handle attaches to the mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingMode {
    /// The closest mesh vertex.
    #[default]
    NearestVertex,
    /// Barycentric coordinates in the containing (or closest) triangle.
    Barycentric,
}

/// A point attached to a mesh.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Binding {
    Vertex(u32),
    Barycentric { triangle: u32, weights: [f64; 3] },
}

impl Binding {
    /// Binds `p` to `mesh`; `None` for an empty mesh or a non-finite point.
    #[must_use]
    pub fn bind(mesh: &Mesh, p: Point, mode: BindingMode) -> Option<Self> {
        if !p.x.is_finite() || !p.y.is_finite() {
            return None;
        }
        match mode {
            BindingMode::NearestVertex => nearest_vertex(mesh, p).map(Self::Vertex),
            BindingMode::Barycentric => nearest_triangle(mesh, p)
                .map(|(triangle, weights)| Self::Barycentric { triangle, weights })
                .or_else(|| nearest_vertex(mesh, p).map(Self::Vertex)),
        }
    }

    /// The mesh vertices this binding depends on.
    #[must_use]
    pub fn vertices(&self, mesh: &Mesh) -> Vec<u32> {
        match *self {
            Self::Vertex(v) => vec![v],
            Self::Barycentric { triangle, .. } => mesh.triangles.get(num::u32_to_usize(triangle)).map(|t| t.to_vec()).unwrap_or_default(),
        }
    }

    /// Where the bound point is, given vertex `positions` (the rest pose gives the original point).
    #[must_use]
    pub fn evaluate(&self, mesh: &Mesh, positions: &[Point]) -> Option<Point> {
        match *self {
            Self::Vertex(v) => positions.get(num::u32_to_usize(v)).copied(),
            Self::Barycentric { triangle, weights } => {
                let t = mesh.triangles.get(num::u32_to_usize(triangle))?;
                let mut acc = Vec2::ZERO;
                for (&id, w) in t.iter().zip(weights) {
                    acc += positions.get(num::u32_to_usize(id))?.to_vec2() * w;
                }
                Some(acc.to_point())
            }
        }
    }
}

fn nearest_vertex(mesh: &Mesh, p: Point) -> Option<u32> {
    let mut best: Option<(f64, usize)> = None;
    for (i, v) in mesh.vertices.iter().enumerate() {
        let d = v.distance_squared(p);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| num::usize_to_u32_sat(i))
}

/// Triangle containing `p` (or, outside the mesh, the one whose clamped coordinates are closest) and weights.
fn nearest_triangle(mesh: &Mesh, p: Point) -> Option<(u32, [f64; 3])> {
    let mut best: Option<(f64, u32, [f64; 3])> = None;
    for i in 0..mesh.triangles.len() {
        let id = num::usize_to_u32_sat(i);
        let Some([a, b, c]) = mesh.triangle_points(id) else { continue };
        let area = tri_area2(a, b, c);
        if !area.is_finite() || area.abs() < 1.0e-18 {
            continue;
        }
        let w = [tri_area2(p, b, c) / area, tri_area2(a, p, c) / area, tri_area2(a, b, p) / area];
        let outside = w.iter().map(|x| (-x).max(0.0)).sum::<f64>();
        if best.is_none_or(|(bo, _, _)| outside < bo) {
            best = Some((outside, id, w));
            if outside == 0.0 {
                break;
            }
        }
    }
    best.map(|(outside, id, w)| {
        if outside == 0.0 {
            (id, w)
        } else {
            let clamped = w.map(|x| x.max(0.0));
            let s: f64 = clamped.iter().sum();
            (id, if s > 0.0 { clamped.map(|x| x / s) } else { [1.0 / 3.0; 3] })
        }
    })
}

/// A set of user handles bound to one mesh; produces the pin list for [`crate::ArapSolver`] and the matching targets.
#[derive(Clone, Debug)]
pub struct HandleSet {
    bindings: Vec<Binding>,
    rest_points: Vec<Point>,
    /// Sorted, deduplicated pinned vertices.
    pinned: Vec<u32>,
    /// For each handle, the indices into `pinned` it drives.
    slots: Vec<Vec<usize>>,
    rest_vertices: Vec<Point>,
}

impl HandleSet {
    /// Binds each rest-pose handle position. Returns `None` when any handle cannot be bound (empty mesh,
    /// non-finite point).
    #[must_use]
    pub fn new(mesh: &Mesh, handles: &[Point], mode: BindingMode) -> Option<Self> {
        let bindings: Vec<Binding> = handles.iter().map(|&p| Binding::bind(mesh, p, mode)).collect::<Option<_>>()?;
        let mut pinned: Vec<u32> = bindings.iter().flat_map(|b| b.vertices(mesh)).collect();
        pinned.sort_unstable();
        pinned.dedup();
        let slots = bindings.iter().map(|b| b.vertices(mesh).iter().filter_map(|v| pinned.binary_search(v).ok()).collect()).collect();
        Some(Self { bindings, rest_points: handles.to_vec(), pinned, slots, rest_vertices: mesh.vertices.clone() })
    }

    /// The bindings, one per handle.
    #[must_use]
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// The pinned vertex ids (sorted); pass to `ArapSolver::new`.
    #[must_use]
    pub fn pinned(&self) -> &[u32] {
        &self.pinned
    }

    /// Pin targets aligned with [`HandleSet::pinned`] for the given handle targets (one per handle). Every vertex of
    /// a handle moves by that handle's displacement, so the bound point lands exactly on its target when the
    /// triangle stays rigid. Vertices shared by several handles take the mean displacement. Missing or non-finite
    /// handle targets leave their handle at rest.
    #[must_use]
    pub fn targets(&self, handle_targets: &[Point]) -> Vec<Point> {
        let mut sum: BTreeMap<usize, (Vec2, f64)> = BTreeMap::new();
        for (h, slots) in self.slots.iter().enumerate() {
            let rest = self.rest_points.get(h).copied().unwrap_or(Point::ZERO);
            let d = handle_targets.get(h).filter(|t| t.x.is_finite() && t.y.is_finite()).map_or(Vec2::ZERO, |&t| t - rest);
            for &s in slots {
                let e = sum.entry(s).or_insert((Vec2::ZERO, 0.0));
                e.0 += d;
                e.1 += 1.0;
            }
        }
        self.pinned
            .iter()
            .enumerate()
            .map(|(s, &v)| {
                let rest = self.rest_vertices.get(num::u32_to_usize(v)).copied().unwrap_or(Point::ZERO);
                match sum.get(&s) {
                    Some(&(d, n)) if n > 0.0 => rest + d / n,
                    _ => rest,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArapSolver, MeshOptions, Outline};

    fn mesh() -> Mesh {
        let o = Outline { outer: vec![Point::new(0.0, 0.0), Point::new(60.0, 0.0), Point::new(60.0, 60.0), Point::new(0.0, 60.0)], holes: vec![] };
        Mesh::from_outline(&o, &MeshOptions { max_edge: 10.0 }).unwrap()
    }

    #[test]
    fn nearest_vertex_binding() {
        let m = mesh();
        let b = Binding::bind(&m, Point::new(0.4, -0.3), BindingMode::NearestVertex).unwrap();
        let p = b.evaluate(&m, &m.vertices).unwrap();
        assert!(p.distance(Point::new(0.0, 0.0)) < 1e-9);
    }

    #[test]
    fn barycentric_roundtrip_inside_and_clamped_outside() {
        let m = mesh();
        let q = Point::new(23.3, 31.7);
        let b = Binding::bind(&m, q, BindingMode::Barycentric).unwrap();
        assert!(b.evaluate(&m, &m.vertices).unwrap().distance(q) < 1e-9);
        let out = Binding::bind(&m, Point::new(-50.0, 30.0), BindingMode::Barycentric).unwrap();
        let p = out.evaluate(&m, &m.vertices).unwrap();
        assert!(p.x >= -1e-9 && p.x < 10.0);
    }

    #[test]
    fn non_finite_and_empty() {
        let m = mesh();
        assert!(Binding::bind(&m, Point::new(f64::NAN, 0.0), BindingMode::Barycentric).is_none());
        assert!(Binding::bind(&Mesh::default(), Point::ZERO, BindingMode::NearestVertex).is_none());
    }

    #[test]
    fn handle_set_drives_solver() {
        let m = mesh();
        let hs = HandleSet::new(&m, &[Point::new(11.0, 12.0), Point::new(48.0, 41.0)], BindingMode::Barycentric).unwrap();
        let solver = ArapSolver::new(&m, hs.pinned()).unwrap();
        let moved = [Point::new(11.0, 12.0) + Vec2::new(4.0, 4.0), Point::new(48.0, 41.0) + Vec2::new(4.0, 4.0)];
        let out = solver.solve(&hs.targets(&moved));
        // Both handles shifted by the same vector: the whole mesh translates.
        assert!(out.iter().zip(&m.vertices).all(|(o, r)| o.distance(*r + Vec2::new(4.0, 4.0)) < 1e-6));
        for (b, t) in hs.bindings().iter().zip(moved) {
            assert!(b.evaluate(&m, &out).unwrap().distance(t) < 1e-6);
        }
    }

    #[test]
    fn unmoved_handles_give_rest_pose() {
        let m = mesh();
        let pts = [Point::new(5.0, 5.0), Point::new(50.0, 20.0)];
        let hs = HandleSet::new(&m, &pts, BindingMode::NearestVertex).unwrap();
        let out = ArapSolver::new(&m, hs.pinned()).unwrap().solve(&hs.targets(&pts));
        assert!(out.iter().zip(&m.vertices).all(|(o, r)| o.distance(*r) < 1e-9));
    }
}
