//! As-rigid-as-possible (ARAP) surface deformation with pinned vertices (Sorkine and Alexa, 2007).
//!
//! [`ArapSolver::new`] precomputes the edge weights, adjacency and the sparse system once per mesh and pin set;
//! [`ArapSolver::solve`] then alternates a local step (best-fit rotation per vertex) and a global step (a sparse
//! symmetric positive definite solve by preconditioned conjugate gradients) starting from a rigid fit of the pins,
//! so a rigid pin motion is reproduced exactly and unmoved pins give the rest pose back.
//!
//! Everything is deterministic (fixed iteration order, no threads). Non-finite pin targets fall back to the rest
//! position; mesh components that contain no pin stay where they are.

use std::collections::BTreeMap;

use anim_core::{Point, Vec2, num};
use serde::{Deserialize, Serialize};

use crate::mesh::Mesh;

/// How edge weights are computed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeightKind {
    /// Cotangent weights: independent of triangle shape, the standard choice.
    #[default]
    Cotangent,
    /// Every edge weighs the same; more stable on poor meshes, less faithful on irregular ones.
    Uniform,
}

/// Solver tuning.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArapOptions {
    /// Local/global alternations per solve.
    pub iterations: u32,
    /// Edge weight scheme.
    pub weights: WeightKind,
    /// 1.0 is true ARAP. Lower values scale every local rotation angle toward zero, which makes the shape follow the
    /// pins more like a pure Laplacian (stretchy) deformation; 0.0 never rotates the neighbourhoods.
    pub rigidity: f64,
    /// Stop alternating when no vertex moved more than this fraction of the mesh diameter.
    pub tolerance: f64,
    /// Conjugate gradient iteration cap per global solve.
    pub max_cg_iterations: u32,
}

impl Default for ArapOptions {
    fn default() -> Self {
        Self { iterations: 12, weights: WeightKind::Cotangent, rigidity: 1.0, tolerance: 1.0e-9, max_cg_iterations: 400 }
    }
}

/// Why a solver could not be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArapError {
    #[error("at least one pinned vertex is required")]
    NoPins,
    #[error("pinned vertex {0} is out of range")]
    PinOutOfRange(u32),
    #[error("the mesh has a triangle index out of range")]
    InvalidMesh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Free,
    Pin(usize),
    /// Not connected to any pin: held at its rest position.
    Anchor,
}

/// Precomputed ARAP system for one mesh and one set of pinned vertices.
#[derive(Clone, Debug)]
pub struct ArapSolver {
    rest: Vec<Point>,
    nbrs: Vec<Vec<(usize, f64)>>,
    roles: Vec<Role>,
    pinned: Vec<u32>,
    options: ArapOptions,
    diameter: f64,
    /// The free vertices, ascending: the unknowns of the global step.
    free: Vec<u32>,
    /// The system matrix of the free block (the Laplacian restricted to free vertices) in compressed rows: the
    /// diagonal of each free vertex, and the offsets of its off-diagonal entries in `cols` (positions among
    /// `free`) and `weights`, in vertex order.
    free_diag: Vec<f64>,
    rows: Vec<u32>,
    cols: Vec<u32>,
    weights: Vec<f64>,
}

/// Best rotation `(cos, sin)` for the 2x2 covariance `[[m00, m01], [m10, m11]]` (sum of `target * rest^T`).
fn best_rotation(m00: f64, m01: f64, m10: f64, m11: f64, rigidity: f64) -> (f64, f64) {
    let c = m00 + m11;
    let s = m10 - m01;
    let n = c.hypot(s);
    if !n.is_finite() || n < 1.0e-300 {
        return (1.0, 0.0);
    }
    if (rigidity - 1.0).abs() < f64::EPSILON {
        return (c / n, s / n);
    }
    let a = s.atan2(c) * rigidity.clamp(0.0, 1.0);
    (a.cos(), a.sin())
}

fn rot(r: (f64, f64), v: Vec2) -> Vec2 {
    Vec2::new(r.0 * v.x - r.1 * v.y, r.1 * v.x + r.0 * v.y)
}

impl ArapSolver {
    /// Builds a solver with default [`ArapOptions`].
    ///
    /// # Errors
    /// See [`ArapError`].
    pub fn new(mesh: &Mesh, pinned: &[u32]) -> Result<Self, ArapError> {
        Self::with_options(mesh, pinned, ArapOptions::default())
    }

    /// Builds a solver. Duplicate pin ids are dropped (first occurrence wins); [`ArapSolver::pinned`] is the order
    /// `solve` expects targets in.
    ///
    /// # Errors
    /// See [`ArapError`].
    pub fn with_options(mesh: &Mesh, pinned: &[u32], options: ArapOptions) -> Result<Self, ArapError> {
        if !mesh.is_valid() {
            return Err(ArapError::InvalidMesh);
        }
        let n = mesh.vertices.len();
        let mut roles = vec![Role::Free; n];
        let mut pins: Vec<u32> = Vec::with_capacity(pinned.len());
        for &id in pinned {
            let idx = num::u32_to_usize(id);
            let role = roles.get_mut(idx).ok_or(ArapError::PinOutOfRange(id))?;
            if matches!(role, Role::Free) {
                *role = Role::Pin(pins.len());
                pins.push(id);
            }
        }
        if pins.is_empty() {
            return Err(ArapError::NoPins);
        }

        let weights = edge_weights(mesh, options.weights);
        let mut nbrs: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        for (&(a, b), &w) in &weights {
            if let Some(l) = nbrs.get_mut(a) {
                l.push((b, w));
            }
            if let Some(l) = nbrs.get_mut(b) {
                l.push((a, w));
            }
        }
        for l in &mut nbrs {
            l.sort_by_key(|&(j, _)| j);
        }

        // Anchor every vertex whose connected component has no pin.
        let mut reached = vec![false; n];
        let mut stack: Vec<usize> = pins.iter().map(|&p| num::u32_to_usize(p)).collect();
        while let Some(v) = stack.pop() {
            if reached.get(v).copied().unwrap_or(true) {
                continue;
            }
            if let Some(r) = reached.get_mut(v) {
                *r = true;
            }
            if let Some(l) = nbrs.get(v) {
                stack.extend(l.iter().map(|&(j, _)| j));
            }
        }
        for (role, &r) in roles.iter_mut().zip(&reached) {
            if !r && matches!(role, Role::Free) {
                *role = Role::Anchor;
            }
        }

        let free: Vec<u32> = roles.iter().enumerate().filter(|(_, r)| matches!(r, Role::Free)).map(|(i, _)| num::usize_to_u32_sat(i)).collect();
        let mut position = vec![0_u32; n];
        for (k, &id) in free.iter().enumerate() {
            if let Some(p) = position.get_mut(num::u32_to_usize(id)) {
                *p = num::usize_to_u32_sat(k);
            }
        }
        let (mut free_diag, mut rows, mut cols, mut weights) = (Vec::with_capacity(free.len()), vec![0_u32], Vec::new(), Vec::new());
        for &id in &free {
            let list = nbrs.get(num::u32_to_usize(id)).map_or(&[][..], Vec::as_slice);
            free_diag.push(list.iter().map(|&(_, w)| w).sum());
            for &(j, w) in list {
                if matches!(roles.get(j), Some(Role::Free)) {
                    cols.push(position.get(j).copied().unwrap_or(0));
                    weights.push(w);
                }
            }
            rows.push(num::usize_to_u32_sat(cols.len()));
        }
        let diameter = mesh.bounds().map_or(1.0, |b| b.width().hypot(b.height())).max(f64::MIN_POSITIVE);
        Ok(Self { rest: mesh.vertices.clone(), nbrs, roles, pinned: pins, options, diameter, free, free_diag, rows, cols, weights })
    }

    /// The pinned vertex ids, in the order `solve` expects targets.
    #[must_use]
    pub fn pinned(&self) -> &[u32] {
        &self.pinned
    }

    /// The rest positions.
    #[must_use]
    pub fn rest(&self) -> &[Point] {
        &self.rest
    }

    /// Current options.
    #[must_use]
    pub const fn options(&self) -> &ArapOptions {
        &self.options
    }

    /// Deforms the mesh so the pinned vertices land on `targets` (aligned with [`ArapSolver::pinned`]). Missing or
    /// non-finite targets keep the pin at its rest position. Returns one position per mesh vertex.
    #[must_use]
    pub fn solve(&self, targets: &[Point]) -> Vec<Point> {
        let pin_pos: Vec<Point> = self
            .pinned
            .iter()
            .enumerate()
            .map(|(k, &id)| {
                let rest = self.rest.get(num::u32_to_usize(id)).copied().unwrap_or(Point::ZERO);
                targets.get(k).copied().filter(|t| t.x.is_finite() && t.y.is_finite()).unwrap_or(rest)
            })
            .collect();

        let mut q = self.initial_guess(&pin_pos);
        let mut rotations = vec![(1.0, 0.0); self.rest.len()];
        let tol = self.options.tolerance * self.diameter;
        for _ in 0..self.options.iterations {
            self.local_step(&q, &mut rotations);
            let next = self.global_step(&q, &rotations);
            let moved = q.iter().zip(&next).map(|(a, b)| a.distance(*b)).fold(0.0, f64::max);
            q = next;
            if moved <= tol {
                break;
            }
        }
        q
    }

    /// Rigid (rotation plus translation) fit of the pins applied to every free vertex.
    fn initial_guess(&self, pin_pos: &[Point]) -> Vec<Point> {
        let count = num::usize_to_f64(pin_pos.len());
        let rest_pins: Vec<Point> = self.pinned.iter().map(|&id| self.rest.get(num::u32_to_usize(id)).copied().unwrap_or(Point::ZERO)).collect();
        let centroid = |pts: &[Point]| {
            let s = pts.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2());
            (s / count).to_point()
        };
        let cp = centroid(&rest_pins);
        let ct = centroid(pin_pos);
        let (mut m00, mut m01, mut m10, mut m11) = (0.0, 0.0, 0.0, 0.0);
        for (p, t) in rest_pins.iter().zip(pin_pos) {
            let a = *p - cp;
            let b = *t - ct;
            m00 += b.x * a.x;
            m01 += b.x * a.y;
            m10 += b.y * a.x;
            m11 += b.y * a.y;
        }
        let r = best_rotation(m00, m01, m10, m11, 1.0);
        self.rest
            .iter()
            .zip(&self.roles)
            .map(|(&p, role)| match *role {
                Role::Pin(k) => pin_pos.get(k).copied().unwrap_or(p),
                Role::Anchor => p,
                Role::Free => ct + rot(r, p - cp),
            })
            .collect()
    }

    /// Best-fit rotation per vertex between rest and current edge vectors.
    fn local_step(&self, q: &[Point], out: &mut [(f64, f64)]) {
        for (i, (l, slot)) in self.nbrs.iter().zip(out.iter_mut()).enumerate() {
            let (Some(&pi), Some(&qi)) = (self.rest.get(i), q.get(i)) else { continue };
            let (mut m00, mut m01, mut m10, mut m11) = (0.0, 0.0, 0.0, 0.0);
            for &(j, w) in l {
                let (Some(&pj), Some(&qj)) = (self.rest.get(j), q.get(j)) else { continue };
                let a = pi - pj;
                let b = qi - qj;
                m00 += w * b.x * a.x;
                m01 += w * b.x * a.y;
                m10 += w * b.y * a.x;
                m11 += w * b.y * a.y;
            }
            *slot = best_rotation(m00, m01, m10, m11, self.options.rigidity);
        }
    }

    /// Solves for the free vertices given the rotations; fixed vertices keep their `q`.
    fn global_step(&self, q: &[Point], rotations: &[(f64, f64)]) -> Vec<Point> {
        let nf = self.free.len();
        let (mut bx, mut by) = (Vec::with_capacity(nf), Vec::with_capacity(nf));
        let (mut xx, mut xy) = (Vec::with_capacity(nf), Vec::with_capacity(nf));
        for &id in &self.free {
            let i = num::u32_to_usize(id);
            let (Some(&pi), Some(l), Some(&ri), Some(&qi)) = (self.rest.get(i), self.nbrs.get(i), rotations.get(i), q.get(i)) else {
                for v in [&mut bx, &mut by, &mut xx, &mut xy] {
                    v.push(0.0);
                }
                continue;
            };
            let mut b = Vec2::ZERO;
            for &(j, w) in l {
                let (Some(&pj), Some(&rj)) = (self.rest.get(j), rotations.get(j)) else { continue };
                let e = pi - pj;
                let ra = rot(ri, e);
                let rb = rot(rj, e);
                b += (ra + rb) * (0.5 * w);
                if let (false, Some(&qj)) = (matches!(self.roles.get(j), Some(Role::Free)), q.get(j)) {
                    b += qj.to_vec2() * w;
                }
            }
            bx.push(b.x);
            by.push(b.y);
            xx.push(qi.x);
            xy.push(qi.y);
        }
        self.cg(&bx, &mut xx);
        self.cg(&by, &mut xy);
        let mut out = q.to_vec();
        for ((&id, &x), &y) in self.free.iter().zip(&xx).zip(&xy) {
            if x.is_finite()
                && y.is_finite()
                && let Some(slot) = out.get_mut(num::u32_to_usize(id))
            {
                *slot = Point::new(x, y);
            }
        }
        out
    }

    /// `out = A u` where `A` is the Laplacian restricted to free vertices (indexed by position among them); returns
    /// `u . out`.
    #[allow(clippy::indexing_slicing, reason = "rows, cols and weights are built together with the free list they index")]
    fn matvec(&self, u: &[f64], out: &mut [f64]) -> f64 {
        let mut dot = 0.0;
        let mut start = 0;
        for (((o, &uk), &d), &end) in out.iter_mut().zip(u).zip(&self.free_diag).zip(self.rows.iter().skip(1)) {
            let end = num::u32_to_usize(end);
            let mut s = d * uk;
            for (&c, &w) in self.cols[start..end].iter().zip(&self.weights[start..end]) {
                s -= w * u[num::u32_to_usize(c)];
            }
            start = end;
            *o = s;
            dot += uk * s;
        }
        dot
    }

    /// Preconditioned CG for the free block. `b` holds the right-hand side with fixed-neighbour terms already
    /// moved over, `x` the starting guess on entry and the solution on exit (both indexed by position among the free
    /// vertices).
    fn cg(&self, b: &[f64], x: &mut [f64]) {
        let precondition = |d: f64, v: f64| if d > 0.0 { v / d } else { v };
        let mut au = vec![0.0; x.len()];
        self.matvec(x, &mut au);
        let mut r: Vec<f64> = b.iter().zip(&au).map(|(b, a)| b - a).collect();
        let bnorm = b.iter().map(|v| v * v).sum::<f64>().sqrt();
        let target = (1.0e-12 * bnorm).max(1.0e-14 * self.diameter);
        let mut z: Vec<f64> = r.iter().zip(&self.free_diag).map(|(&v, &d)| precondition(d, v)).collect();
        let mut p = z.clone();
        let mut rz: f64 = r.iter().zip(&z).map(|(a, c)| a * c).sum();
        let mut res2: f64 = r.iter().map(|v| v * v).sum();
        for _ in 0..self.options.max_cg_iterations {
            let rn = res2.sqrt();
            if rn <= target || !rn.is_finite() {
                break;
            }
            let pap = self.matvec(&p, &mut au);
            if pap <= 0.0 || !pap.is_finite() {
                break;
            }
            let alpha = rz / pap;
            let (mut rz_new, mut res2_new) = (0.0, 0.0);
            for ((((xi, ri), zi), (pi, api)), &d) in x.iter_mut().zip(r.iter_mut()).zip(z.iter_mut()).zip(p.iter().zip(&au)).zip(&self.free_diag) {
                *xi += alpha * pi;
                *ri -= alpha * api;
                *zi = precondition(d, *ri);
                rz_new += *ri * *zi;
                res2_new += *ri * *ri;
            }
            res2 = res2_new;
            let beta = if rz > 0.0 { rz_new / rz } else { 0.0 };
            rz = rz_new;
            for (pi, zi) in p.iter_mut().zip(&z) {
                *pi = zi + beta * *pi;
            }
        }
    }
}

/// Undirected edge weights keyed by `(low, high)` vertex index.
fn edge_weights(mesh: &Mesh, kind: WeightKind) -> BTreeMap<(usize, usize), f64> {
    const FLOOR: f64 = 1.0e-3;
    let mut map: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    let key = |a: u32, b: u32| {
        let (a, b) = (num::u32_to_usize(a), num::u32_to_usize(b));
        (a.min(b), a.max(b))
    };
    for &[a, b, c] in &mesh.triangles {
        let (Some(pa), Some(pb), Some(pc)) = (mesh.vertex(a), mesh.vertex(b), mesh.vertex(c)) else { continue };
        if a == b || b == c || a == c {
            continue;
        }
        match kind {
            WeightKind::Uniform => {
                for (u, v) in [(a, b), (b, c), (c, a)] {
                    map.entry(key(u, v)).or_insert(1.0);
                }
            }
            WeightKind::Cotangent => {
                let area2 = (pb - pa).cross(pc - pa).abs();
                let degenerate = area2.is_nan() || area2 <= 1.0e-12 * (pb - pa).hypot2().max((pc - pa).hypot2());
                // The cotangent of the corner at each vertex weighs the edge opposite to it.
                for (corner, pu, pv, u, v) in [(pa, pb, pc, b, c), (pb, pc, pa, c, a), (pc, pa, pb, a, b)] {
                    let e1 = pu - corner;
                    let e2 = pv - corner;
                    let cot = if degenerate { 0.0 } else { e1.dot(e2) / e1.cross(e2).abs() };
                    *map.entry(key(u, v)).or_insert(0.0) += 0.5 * cot;
                }
            }
        }
    }
    for w in map.values_mut() {
        if !w.is_finite() || *w < FLOOR {
            *w = FLOOR;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{MeshOptions, Outline};

    fn rect_mesh(w: f64, h: f64, edge: f64) -> Mesh {
        let o = Outline { outer: vec![Point::new(0.0, 0.0), Point::new(w, 0.0), Point::new(w, h), Point::new(0.0, h)], holes: vec![] };
        Mesh::from_outline(&o, &MeshOptions { max_edge: edge }).unwrap()
    }

    fn ids_where(mesh: &Mesh, f: impl Fn(Point) -> bool) -> Vec<u32> {
        mesh.vertices.iter().enumerate().filter(|(_, p)| f(**p)).map(|(i, _)| u32::try_from(i).unwrap()).collect()
    }

    fn max_err(a: &[Point], b: &[Point]) -> f64 {
        a.iter().zip(b).map(|(x, y)| x.distance(*y)).fold(0.0, f64::max)
    }

    #[test]
    fn identity_when_pins_unmoved() {
        let m = rect_mesh(100.0, 40.0, 8.0);
        let pins = ids_where(&m, |p| p.x < 1.0);
        let s = ArapSolver::new(&m, &pins).unwrap();
        let targets: Vec<Point> = s.pinned().iter().map(|&i| m.vertices[i as usize]).collect();
        let out = s.solve(&targets);
        assert!(max_err(&out, &m.vertices) < 1e-9, "{}", max_err(&out, &m.vertices));
    }

    #[test]
    fn translation_of_all_pins_is_rigid() {
        let m = rect_mesh(100.0, 40.0, 8.0);
        let pins = ids_where(&m, |p| p.x < 1.0 || p.x > 99.0);
        let s = ArapSolver::new(&m, &pins).unwrap();
        let d = Vec2::new(13.5, -7.25);
        let targets: Vec<Point> = s.pinned().iter().map(|&i| m.vertices[i as usize] + d).collect();
        let out = s.solve(&targets);
        let want: Vec<Point> = m.vertices.iter().map(|&p| p + d).collect();
        assert!(max_err(&out, &want) < 1e-6, "{}", max_err(&out, &want));
    }

    #[test]
    fn rotation_of_all_pins_is_rigid() {
        let m = rect_mesh(100.0, 40.0, 8.0);
        let pins = ids_where(&m, |p| p.x < 1.0 || p.x > 99.0);
        let s = ArapSolver::new(&m, &pins).unwrap();
        let (sn, cs) = 0.7_f64.sin_cos();
        let c = Point::new(30.0, 10.0);
        let rotate = |p: Point| {
            let v = p - c;
            c + Vec2::new(cs * v.x - sn * v.y, sn * v.x + cs * v.y)
        };
        let targets: Vec<Point> = s.pinned().iter().map(|&i| rotate(m.vertices[i as usize])).collect();
        let out = s.solve(&targets);
        let want: Vec<Point> = m.vertices.iter().map(|&p| rotate(p)).collect();
        assert!(max_err(&out, &want) < 1e-6, "{}", max_err(&out, &want));
    }

    #[test]
    fn bending_preserves_edge_lengths_approximately() {
        let m = rect_mesh(100.0, 20.0, 5.0);
        let left = ids_where(&m, |p| p.x < 1.0);
        let right = ids_where(&m, |p| p.x > 99.0);
        let pins: Vec<u32> = left.iter().chain(&right).copied().collect();
        let s = ArapSolver::new(&m, &pins).unwrap();
        // Right end rotated 40 degrees about the origin: the bar has to bend.
        let (sn, cs) = 0.7_f64.sin_cos();
        let targets: Vec<Point> = s
            .pinned()
            .iter()
            .map(|&i| {
                let p = m.vertices[i as usize];
                if p.x > 99.0 { Point::new(cs * p.x - sn * p.y, sn * p.x + cs * p.y) } else { p }
            })
            .collect();
        let out = s.solve(&targets);
        assert!(out.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        let mut worst: f64 = 0.0;
        let mut total = 0.0;
        let edges = m.edges();
        for &(a, b) in &edges {
            let r = m.vertices[a as usize].distance(m.vertices[b as usize]);
            let d = out[a as usize].distance(out[b as usize]);
            let change = (d / r - 1.0).abs();
            worst = worst.max(change);
            total += change;
        }
        let mean = total / f64::from(u32::try_from(edges.len()).unwrap());
        assert!(worst < 0.6, "worst relative edge change {worst}");
        assert!(mean < 0.08, "mean relative edge change {mean}");
        // The bar actually moved in the middle.
        let mid = ids_where(&m, |p| (p.x - 50.0).abs() < 2.0 && (p.y - 10.0).abs() < 3.0);
        assert!(mid.iter().any(|&i| out[i as usize].distance(m.vertices[i as usize]) > 5.0));
    }

    #[test]
    fn single_pin_drag_translates() {
        let m = rect_mesh(60.0, 60.0, 10.0);
        let s = ArapSolver::new(&m, &[0]).unwrap();
        let d = Vec2::new(5.0, 9.0);
        let out = s.solve(&[m.vertices[0] + d]);
        let want: Vec<Point> = m.vertices.iter().map(|&p| p + d).collect();
        assert!(max_err(&out, &want) < 1e-6);
    }

    #[test]
    fn extreme_pins_stay_finite_and_deterministic() {
        let m = rect_mesh(100.0, 40.0, 6.0);
        let pins = ids_where(&m, |p| p.x < 1.0 || p.x > 99.0);
        let s = ArapSolver::new(&m, &pins).unwrap();
        let targets: Vec<Point> = s
            .pinned()
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                let p = m.vertices[i as usize];
                if k % 3 == 0 { Point::new(p.x * 1.0e6, -p.y * 1.0e5) } else { Point::new(p.x, p.y + 1.0e4) }
            })
            .collect();
        let a = s.solve(&targets);
        let b = s.solve(&targets);
        assert_eq!(a, b);
        assert!(a.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        let nan = vec![Point::new(f64::NAN, f64::INFINITY); targets.len()];
        let c = s.solve(&nan);
        assert!(c.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        assert!(s.solve(&[]).iter().zip(&m.vertices).all(|(p, q)| p.distance(*q) < 1e-9));
    }

    #[test]
    fn unpinned_component_stays_put() {
        let mut m = rect_mesh(20.0, 20.0, 10.0);
        let n = u32::try_from(m.vertices.len()).unwrap();
        m.vertices.extend([Point::new(100.0, 100.0), Point::new(110.0, 100.0), Point::new(100.0, 110.0)]);
        m.triangles.push([n, n + 1, n + 2]);
        let s = ArapSolver::new(&m, &[0]).unwrap();
        let out = s.solve(&[Point::new(50.0, 50.0)]);
        assert_eq!(out[n as usize], Point::new(100.0, 100.0));
    }

    #[test]
    fn errors() {
        let m = rect_mesh(10.0, 10.0, 0.0);
        assert_eq!(ArapSolver::new(&m, &[]).unwrap_err(), ArapError::NoPins);
        assert_eq!(ArapSolver::new(&m, &[99]).unwrap_err(), ArapError::PinOutOfRange(99));
        let bad = Mesh { vertices: vec![Point::ZERO], triangles: vec![[0, 1, 2]] };
        assert_eq!(ArapSolver::new(&bad, &[0]).unwrap_err(), ArapError::InvalidMesh);
    }

    #[test]
    fn uniform_weights_and_rigidity_option_work() {
        let m = rect_mesh(60.0, 20.0, 5.0);
        let pins = ids_where(&m, |p| p.x < 1.0 || p.x > 59.0);
        for opts in [
            ArapOptions { weights: WeightKind::Uniform, ..ArapOptions::default() },
            ArapOptions { rigidity: 0.0, ..ArapOptions::default() },
            ArapOptions { rigidity: 0.5, ..ArapOptions::default() },
        ] {
            let s = ArapSolver::with_options(&m, &pins, opts).unwrap();
            let targets: Vec<Point> = s
                .pinned()
                .iter()
                .map(|&i| {
                    let p = m.vertices[i as usize];
                    if p.x > 59.0 { Point::new(p.x, p.y + 15.0) } else { p }
                })
                .collect();
            let out = s.solve(&targets);
            assert!(out.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        }
    }
}
