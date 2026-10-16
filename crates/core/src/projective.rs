//! Flash-style 3D: [`Matrix3D`], the document's [`Perspective`] and the [`Homography`] that a display object lying in
//! its own z = 0 plane projects to.
//!
//! # Conventions
//!
//! * Stage space is y down, and **z grows away from the viewer** (as in Flash: a positive `z` makes an object
//!   smaller, a negative one bigger).
//! * [`Matrix3D`] stores the sixteen values of XFL's `<Matrix3D m11 .. m44>` and `flash.geom.Matrix3D`: row-major,
//!   points are row vectors (`p' = p * M`), so the translation is the fourth row (`m41 m42 m43`) and
//!   `a.then(&b)` applies `a` first.
//! * The Euler angles of [`Matrix3D::decompose`] compose as scale, then rotation about X, then Y, then Z, then
//!   translation, with the standard right-handed formulas for the rotation matrices of a y-down frame
//!   (`Rz` is the 2D matrix `[cos sin; -sin cos]`, positive angles turn clockwise on screen).
//! * Perspective is the Flash `PerspectiveProjection`: with field of view `fov` over the stage width `w` the
//!   focal length is `f = (w / 2) / tan(fov / 2)`, and a point `(x, y, z)` appears at
//!   `vanishing_point + ((x, y) - vanishing_point) * f / (f + z)`. A point with `z <= -f` is behind the viewer
//!   and is not drawn.
//!
//! Because a display object is flat, the whole projection of its plane is one projective map of the plane to the
//! screen, a [`Homography`]; that is what renderers consume.

#![allow(clippy::suboptimal_flops, reason = "no fused multiply-add: results must be bit-identical on every CPU")]
#![allow(clippy::many_single_char_names, reason = "matrix entries are named after their positions")]

use serde::{Deserialize, Serialize};

use crate::{Affine, Point};

/// Homogeneous weights below this count as "behind the viewer".
pub const MIN_W: f64 = 1.0e-9;

/// Flash's default perspective angle (field of view) in degrees.
pub const DEFAULT_PERSPECTIVE_ANGLE: f64 = 55.0;

/// A 4x4 transform in the row-vector convention of `flash.geom.Matrix3D`; see the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Matrix3D {
    /// Row `i`, column `j` is `m(i+1)(j+1)`.
    pub m: [[f64; 4]; 4],
}

impl Default for Matrix3D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A [`Matrix3D`] split into the properties the 3D tools show: position (including `z`), rotation about each axis in
/// degrees, and scale per axis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform3D {
    pub translation: [f64; 3],
    /// Degrees about X, Y and Z.
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
}

impl Default for Transform3D {
    fn default() -> Self {
        Self { translation: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] }
    }
}

fn dot(a: [f64; 4], b: [f64; 4]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn column(m: &[[f64; 4]; 4], c: usize) -> [f64; 4] {
    m.map(|row| row.get(c).copied().unwrap_or(0.0))
}

impl Matrix3D {
    pub const IDENTITY: Self = Self { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] };

    /// From the sixteen XFL values in order `m11 m12 m13 m14 m21 ... m44`.
    #[must_use]
    pub const fn from_values(v: [f64; 16]) -> Self {
        let [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p] = v;
        Self { m: [[a, b, c, d], [e, f, g, h], [i, j, k, l], [m, n, o, p]] }
    }

    /// The sixteen values in XFL order.
    #[must_use]
    pub const fn values(&self) -> [f64; 16] {
        let [[a, b, c, d], [e, f, g, h], [i, j, k, l], [m, n, o, p]] = self.m;
        [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p]
    }

    /// A 2D matrix placed in the z = 0 plane.
    #[must_use]
    pub const fn from_affine(a: Affine) -> Self {
        let [xa, xb, yc, yd, tx, ty] = a.as_coeffs();
        Self { m: [[xa, xb, 0.0, 0.0], [yc, yd, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [tx, ty, 0.0, 1.0]] }
    }

    #[must_use]
    pub const fn translation(x: f64, y: f64, z: f64) -> Self {
        Self { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, z, 1.0]] }
    }

    #[must_use]
    pub const fn scaling(x: f64, y: f64, z: f64) -> Self {
        Self { m: [[x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, z, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }

    /// Rotation about the X axis (the horizontal axis), degrees.
    #[must_use]
    pub fn rotation_x(degrees: f64) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        Self { m: [[1.0, 0.0, 0.0, 0.0], [0.0, c, s, 0.0], [0.0, -s, c, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }

    /// Rotation about the Y axis (the vertical axis), degrees.
    #[must_use]
    pub fn rotation_y(degrees: f64) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        Self { m: [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }

    /// Rotation about the Z axis (in the screen plane), degrees.
    #[must_use]
    pub fn rotation_z(degrees: f64) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        Self { m: [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }

    /// `self` first, then `next`.
    #[must_use]
    pub fn then(&self, next: &Self) -> Self {
        let cols: [[f64; 4]; 4] = std::array::from_fn(|c| column(&next.m, c));
        Self { m: self.m.map(|row| cols.map(|col| dot(row, col))) }
    }

    /// Where the point `p` goes (the fourth column is ignored: no w divide).
    #[must_use]
    pub fn transform_point(&self, p: [f64; 3]) -> [f64; 3] {
        let v = [p[0], p[1], p[2], 1.0];
        let out: [f64; 3] = std::array::from_fn(|c| dot(v, column(&self.m, c)));
        out
    }

    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.m.iter().flatten().all(|v| v.is_finite())
    }

    /// True when the matrix is a 2D transform in disguise: it keeps the z = 0 plane and does not couple z with x
    /// or y.
    #[must_use]
    pub fn is_planar(&self) -> bool {
        let [[_, _, m13, m14], [_, _, m23, m24], [m31, m32, m33, m34], [_, _, m43, m44]] = self.m;
        [m13, m14, m23, m24, m31, m32, m34, m43].iter().all(|v| v.abs() < 1.0e-12) && (m33 - 1.0).abs() < 1.0e-12 && (m44 - 1.0).abs() < 1.0e-12
    }

    /// The 2D matrix of the z = 0 plane ignoring depth (what the matrix is when there is no perspective).
    #[must_use]
    pub const fn plane_affine(&self) -> Affine {
        let [[a, b, _, _], [c, d, _, _], _, [tx, ty, _, _]] = self.m;
        Affine::new([a, b, c, d, tx, ty])
    }

    /// Splits into position, rotation and scale; `None` when the matrix is not finite or has a zero axis.
    #[must_use]
    pub fn decompose(&self) -> Option<Transform3D> {
        if !self.is_finite() {
            return None;
        }
        let [[r00, r01, r02, _], [r10, r11, r12, _], [r20, r21, r22, _], [tx, ty, tz, _]] = self.m;
        let len = |a: f64, b: f64, c: f64| (a * a + b * b + c * c).sqrt();
        let (mut sx, sy, sz) = (len(r00, r01, r02), len(r10, r11, r12), len(r20, r21, r22));
        if sx < 1.0e-12 || sy < 1.0e-12 || sz < 1.0e-12 {
            return None;
        }
        let det = r00 * (r11 * r22 - r12 * r21) - r01 * (r10 * r22 - r12 * r20) + r02 * (r10 * r21 - r11 * r20);
        let sign = if det < 0.0 { -1.0 } else { 1.0 };
        sx *= sign;
        // Normalised rotation rows.
        let n = |v: f64, s: f64| v / s;
        let (a00, a01, a02) = (n(r00, sx), n(r01, sx), n(r02, sx));
        let (a10, a11, a12) = (n(r10, sy), n(r11, sy), n(r12, sy));
        let a22 = n(r22, sz);
        let _ = (r20, r21);
        let ry = (-a02).clamp(-1.0, 1.0).asin();
        let (rx, rz) = if ry.cos().abs() > 1.0e-9 { (a12.atan2(a22), a01.atan2(a00)) } else { ((a10 / ry.sin()).atan2(a11), 0.0) };
        Some(Transform3D { translation: [tx, ty, tz], rotation: [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()], scale: [sx, sy, sz] })
    }

    /// The matrix of `t`; inverse of [`Matrix3D::decompose`].
    #[must_use]
    pub fn compose(t: &Transform3D) -> Self {
        let [sx, sy, sz] = t.scale;
        let [rx, ry, rz] = t.rotation;
        let [x, y, z] = t.translation;
        Self::scaling(sx, sy, sz)
            .then(&Self::rotation_x(rx))
            .then(&Self::rotation_y(ry))
            .then(&Self::rotation_z(rz))
            .then(&Self::translation(x, y, z))
    }

    /// Interpolates between two matrices through their decompositions (angles take the short way round). Falls back
    /// to the nearer endpoint when either cannot be decomposed.
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f64) -> Self {
        let (Some(a), Some(b)) = (self.decompose(), other.decompose()) else {
            return if t < 0.5 { *self } else { *other };
        };
        let mix = |x: f64, y: f64| (y - x).mul_add(t, x);
        let angle = |x: f64, y: f64| {
            let delta = (y - x + 180.0).rem_euclid(360.0) - 180.0;
            delta.mul_add(t, x)
        };
        Self::compose(&Transform3D {
            translation: std::array::from_fn(|i| mix(a.translation.get(i).copied().unwrap_or(0.0), b.translation.get(i).copied().unwrap_or(0.0))),
            rotation: std::array::from_fn(|i| angle(a.rotation.get(i).copied().unwrap_or(0.0), b.rotation.get(i).copied().unwrap_or(0.0))),
            scale: std::array::from_fn(|i| mix(a.scale.get(i).copied().unwrap_or(1.0), b.scale.get(i).copied().unwrap_or(1.0))),
        })
    }

    /// The projective map from the object's own plane `(x, y)` to the screen under `perspective`.
    #[must_use]
    pub fn homography(&self, perspective: &Perspective) -> Homography {
        let f = perspective.focal_length();
        let (vx, vy) = (perspective.vanishing_point.x, perspective.vanishing_point.y);
        let [[m11, m12, m13, _], [m21, m22, m23, _], _, [m41, m42, m43, _]] = self.m;
        Homography([
            m11 + vx * m13 / f,
            m21 + vx * m23 / f,
            m41 + vx * m43 / f,
            m12 + vy * m13 / f,
            m22 + vy * m23 / f,
            m42 + vy * m43 / f,
            m13 / f,
            m23 / f,
            1.0 + m43 / f,
        ])
    }
}

/// The document's perspective: field of view, the stage width it refers to and the vanishing point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Perspective {
    /// Field of view in degrees, 1 ..= 179 (Animate's "Perspective Angle").
    pub angle: f64,
    /// Stage width in pixels.
    pub stage_width: f64,
    /// Where parallel lines through the stage meet, in stage pixels (Animate's "Vanishing Point").
    pub vanishing_point: Point,
}

impl Perspective {
    /// The default view of a stage: 55 degrees, vanishing point at the centre.
    #[must_use]
    pub fn for_stage(width: f64, height: f64) -> Self {
        Self { angle: DEFAULT_PERSPECTIVE_ANGLE, stage_width: width, vanishing_point: Point::new(width / 2.0, height / 2.0) }
    }

    /// Distance from the viewer to the stage plane, in pixels. Always positive and finite.
    #[must_use]
    pub fn focal_length(&self) -> f64 {
        let angle = if self.angle.is_finite() { self.angle.clamp(1.0, 179.0) } else { DEFAULT_PERSPECTIVE_ANGLE };
        let width = if self.stage_width.is_finite() && self.stage_width > 0.0 { self.stage_width } else { 550.0 };
        (width / 2.0) / (angle.to_radians() / 2.0).tan()
    }
}

/// A projective map of the plane: `(x, y)` goes to `((h11 x + h12 y + h13) / w, (h21 x + h22 y + h23) / w)` with
/// `w = h31 x + h32 y + h33`. Row-major, acting on column vectors.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Homography(pub [f64; 9]);

impl Default for Homography {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Homography {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);

    /// An affine map as a homography.
    #[must_use]
    pub const fn from_affine(a: Affine) -> Self {
        let [xa, xb, yc, yd, tx, ty] = a.as_coeffs();
        Self([xa, yc, tx, xb, yd, ty, 0.0, 0.0, 1.0])
    }

    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }

    /// The weight `w` at `p`; a point is in front of the viewer when it is positive.
    #[must_use]
    pub const fn weight(&self, p: Point) -> f64 {
        let [_, _, _, _, _, _, g, h, i] = self.0;
        g * p.x + h * p.y + i
    }

    /// The image of `p`, or `None` when `p` is behind the viewer or the result is not finite.
    #[must_use]
    pub fn apply(&self, p: Point) -> Option<Point> {
        let [a, b, c, d, e, f, ..] = self.0;
        let w = self.weight(p);
        if w <= MIN_W {
            return None;
        }
        let q = Point::new((a * p.x + b * p.y + c) / w, (d * p.x + e * p.y + f) / w);
        (q.x.is_finite() && q.y.is_finite()).then_some(q)
    }

    /// `self` after `rhs` (`rhs` is applied first).
    #[must_use]
    pub fn compose(&self, rhs: &Self) -> Self {
        let [a, b, c, d, e, f, g, h, i] = self.0;
        let [p, q, r, s, t, u, v, w, x] = rhs.0;
        Self([
            a * p + b * s + c * v,
            a * q + b * t + c * w,
            a * r + b * u + c * x,
            d * p + e * s + f * v,
            d * q + e * t + f * w,
            d * r + e * u + f * x,
            g * p + h * s + i * v,
            g * q + h * t + i * w,
            g * r + h * u + i * x,
        ])
    }

    /// `self` after the affine map `rhs`.
    #[must_use]
    pub fn after_affine(&self, rhs: Affine) -> Self {
        self.compose(&Self::from_affine(rhs))
    }

    /// The affine map `lhs` after `self`.
    #[must_use]
    pub fn before_affine(&self, lhs: Affine) -> Self {
        Self::from_affine(lhs).compose(self)
    }

    /// The inverse map; `None` when `self` is singular.
    #[must_use]
    pub fn inverse(&self) -> Option<Self> {
        let [a, b, c, d, e, f, g, h, i] = self.0;
        let (co0, co1, co2) = (e * i - f * h, f * g - d * i, d * h - e * g);
        let det = a * co0 + b * co1 + c * co2;
        if !det.is_finite() || det.abs() < 1.0e-18 {
            return None;
        }
        let k = 1.0 / det;
        Some(Self([
            co0 * k,
            (c * h - b * i) * k,
            (b * f - c * e) * k,
            co1 * k,
            (a * i - c * g) * k,
            (c * d - a * f) * k,
            co2 * k,
            (b * g - a * h) * k,
            (a * e - b * d) * k,
        ]))
    }

    /// The affine map that agrees with `self` at `p` to first order (value and derivatives); `None` when `p` is
    /// behind the viewer.
    #[must_use]
    pub fn affine_at(&self, p: Point) -> Option<Affine> {
        let [a, b, c, d, e, f, g, h, _] = self.0;
        let w = self.weight(p);
        if w <= MIN_W {
            return None;
        }
        let image = self.apply(p)?;
        // d(X/w)/dx = (a - image.x * g) / w, and so on.
        let (dxx, dyx) = ((a - image.x * g) / w, (d - image.y * g) / w);
        let (dxy, dyy) = ((b - image.x * h) / w, (e - image.y * h) / w);
        let tx = image.x - (dxx * p.x + dxy * p.y);
        let ty = image.y - (dyx * p.x + dyy * p.y);
        let _ = (c, f);
        let affine = Affine::new([dxx, dyx, dxy, dyy, tx, ty]);
        affine.is_finite().then_some(affine)
    }

    /// True when the map is affine within `tol` (no perspective part).
    #[must_use]
    pub fn is_affine(&self, tol: f64) -> bool {
        let [_, _, _, _, _, _, g, h, i] = self.0;
        g.abs() <= tol && h.abs() <= tol && (i - 1.0).abs() <= tol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1.0e-6
    }

    #[test]
    fn planar_matrix_is_the_affine_it_embeds() {
        let a = Affine::new([2.0, 0.5, -0.5, 3.0, 10.0, 20.0]);
        let m = Matrix3D::from_affine(a);
        assert!(m.is_planar());
        assert_eq!(m.plane_affine(), a);
        let h = m.homography(&Perspective::for_stage(550.0, 400.0));
        let p = Point::new(7.0, -3.0);
        let q = h.apply(p).unwrap();
        let r = a * p;
        assert!(close(q.x, r.x) && close(q.y, r.y));
    }

    #[test]
    fn decompose_round_trips() {
        let t = Transform3D { translation: [10.0, -20.0, 30.0], rotation: [20.0, -35.0, 70.0], scale: [1.5, 0.75, 1.0] };
        let back = Matrix3D::compose(&t).decompose().unwrap();
        for (a, b) in t.translation.iter().zip(back.translation).chain(t.rotation.iter().zip(back.rotation)).chain(t.scale.iter().zip(back.scale)) {
            assert!(close(*a, b), "{t:?} vs {back:?}");
        }
    }

    #[test]
    fn rotation_about_y_foreshortens_x() {
        let m = Matrix3D::rotation_y(60.0);
        let [x, _, z] = m.transform_point([100.0, 0.0, 0.0]);
        assert!(close(x, 50.0));
        assert!(close(z.abs(), 100.0 * 60f64.to_radians().sin()));
    }

    #[test]
    fn farther_points_converge_on_the_vanishing_point() {
        let p = Perspective { angle: 90.0, stage_width: 400.0, vanishing_point: Point::new(200.0, 100.0) };
        assert!(close(p.focal_length(), 200.0));
        // z = f halves the distance to the vanishing point.
        let h = Matrix3D::translation(0.0, 0.0, 200.0).homography(&p);
        let q = h.apply(Point::new(300.0, 100.0)).unwrap();
        assert!(close(q.x, 250.0) && close(q.y, 100.0));
        // z = -f is the viewer's plane.
        let behind = Matrix3D::translation(0.0, 0.0, -200.0).homography(&p);
        assert!(behind.apply(Point::new(300.0, 100.0)).is_none());
    }

    #[test]
    fn homography_inverse_and_tangent() {
        let p = Perspective::for_stage(550.0, 400.0);
        let h = Matrix3D::rotation_y(40.0).then(&Matrix3D::translation(275.0, 200.0, 0.0)).homography(&p);
        let inverse = h.inverse().unwrap();
        let q = h.apply(Point::new(30.0, 10.0)).unwrap();
        let back = inverse.apply(q).unwrap();
        assert!(close(back.x, 30.0) && close(back.y, 10.0));
        let tangent = h.affine_at(Point::new(30.0, 10.0)).unwrap();
        let t = tangent * Point::new(30.0, 10.0);
        assert!(close(t.x, q.x) && close(t.y, q.y));
    }

    #[test]
    fn lerp_halfway_between_rotations() {
        let a = Matrix3D::rotation_y(0.0);
        let b = Matrix3D::rotation_y(80.0);
        let mid = a.lerp(&b, 0.5).decompose().unwrap();
        assert!(close(mid.rotation.get(1).copied().unwrap(), 40.0));
    }
}
