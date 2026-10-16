//! Matrix decomposition and interpolation for classic tweens.
//!
//! A Flash matrix `[a b c d tx ty]` maps `x' = a*x + c*y + tx`, `y' = b*x + d*y + ty`. The columns `(a, b)` and
//! `(c, d)` are the images of the x and y axes. They decompose into a rotation `theta` (the x axis angle, y down so
//! positive is clockwise on screen), scales `sx` and `sy` (the lengths of the axis images; `sy` is negative for a
//! mirrored matrix) and a skew (the angle of the y axis relative to a right angle from the x axis).

use std::f64::consts::{PI, TAU};

use anim_core::Affine;
use anim_scene::TweenRotate;

/// A matrix split into the components a tween interpolates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decomposed {
    pub tx: f64,
    pub ty: f64,
    pub sx: f64,
    pub sy: f64,
    /// Rotation of the x axis, radians.
    pub rotation: f64,
    /// Y axis angle minus the rotation, radians.
    pub skew: f64,
}

/// Splits `m` into translation, scales, rotation and skew. Non-finite input decomposes to the identity.
#[must_use]
pub fn decompose(m: Affine) -> Decomposed {
    let coeffs = m.as_coeffs();
    if !coeffs.iter().all(|v| v.is_finite()) {
        return Decomposed { tx: 0.0, ty: 0.0, sx: 1.0, sy: 1.0, rotation: 0.0, skew: 0.0 };
    }
    let [xa, xb, yc, yd, tx, ty] = coeffs;
    let sx = xa.hypot(xb);
    let rotation = if sx > 1e-12 { xb.atan2(xa) } else { 0.0 };
    let mirrored = xa.mul_add(yd, -(xb * yc)) < 0.0;
    // For a mirrored matrix the y axis is measured from its negation, with a negative sy.
    let (cy, dy) = if mirrored { (-yc, -yd) } else { (yc, yd) };
    let sy_len = cy.hypot(dy);
    let phi = if sy_len > 1e-12 { (-cy).atan2(dy) } else { rotation };
    Decomposed { tx, ty, sx, sy: if mirrored { -sy_len } else { sy_len }, rotation, skew: wrap_angle(phi - rotation) }
}

/// The matrix with the given components. Inverse of [`decompose`].
#[must_use]
pub fn compose(d: &Decomposed) -> Affine {
    let phi = d.rotation + d.skew;
    Affine::new([d.sx * d.rotation.cos(), d.sx * d.rotation.sin(), -d.sy * phi.sin(), d.sy * phi.cos(), d.tx, d.ty])
}

/// `a` at `t = 0`, `b` at `t = 1`.
#[must_use]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (b - a).mul_add(t, a)
}

/// Wraps an angle to `(-pi, pi]`.
#[must_use]
pub fn wrap_angle(a: f64) -> f64 {
    if !a.is_finite() {
        return 0.0;
    }
    let r = (a + PI).rem_euclid(TAU) - PI;
    if r <= -PI { PI } else { r }
}

/// How a classic tween treats rotation and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TweenParams {
    pub rotate: TweenRotate,
    /// Extra full turns for clockwise and counter-clockwise rotation.
    pub times: f64,
    /// False holds the start scale.
    pub scale: bool,
}

impl Default for TweenParams {
    fn default() -> Self {
        Self { rotate: TweenRotate::Auto, times: 1.0, scale: true }
    }
}

/// Eased progress per property group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub position: f64,
    pub rotation: f64,
    pub scale: f64,
}

impl Progress {
    /// The same progress for every property.
    #[must_use]
    pub const fn uniform(p: f64) -> Self {
        Self { position: p, rotation: p, scale: p }
    }
}

/// Interpolates two matrices.
///
/// - Position is a straight line between the translations.
/// - Rotation: `Auto` takes the shortest way round, `Clockwise` and `CounterClockwise` go the stated way and add
///   `times` full turns, `None` keeps the start rotation and skew (the object does not rotate during the tween).
/// - Scale is interpolated linearly unless `params.scale` is false, which holds the start scale.
#[must_use]
pub fn interpolate(a: Affine, b: Affine, progress: Progress, params: &TweenParams) -> Affine {
    let (da, db) = (decompose(a), decompose(b));
    let times = if params.times.is_finite() { params.times.max(0.0) } else { 1.0 };
    // Total change of the rotation over the tween, and of the skew.
    let turn = match params.rotate {
        TweenRotate::None => 0.0,
        TweenRotate::Auto => wrap_angle(db.rotation - da.rotation),
        TweenRotate::Clockwise => TAU.mul_add(times, (db.rotation - da.rotation).rem_euclid(TAU)),
        TweenRotate::CounterClockwise => -TAU.mul_add(times, (da.rotation - db.rotation).rem_euclid(TAU)),
    };
    let skew_change = if params.rotate == TweenRotate::None { 0.0 } else { wrap_angle(db.skew - da.skew) };
    let rotation = turn.mul_add(progress.rotation, da.rotation);
    let skew = skew_change.mul_add(progress.rotation, da.skew);
    let (sx, sy) = if params.scale { (lerp(da.sx, db.sx, progress.scale), lerp(da.sy, db.sy, progress.scale)) } else { (da.sx, da.sy) };
    compose(&Decomposed { tx: lerp(da.tx, db.tx, progress.position), ty: lerp(da.ty, db.ty, progress.position), sx, sy, rotation, skew })
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use proptest::prelude::*;

    use super::*;

    fn near(a: Affine, b: Affine) -> bool {
        a.as_coeffs().iter().zip(b.as_coeffs().iter()).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn decompose_known_matrix() {
        let m = Affine::translate((10.0, 20.0)) * Affine::rotate(0.5) * Affine::scale_non_uniform(2.0, 3.0);
        let d = decompose(m);
        assert!((d.rotation - 0.5).abs() < 1e-9);
        assert!((d.sx - 2.0).abs() < 1e-9 && (d.sy - 3.0).abs() < 1e-9);
        assert!(d.skew.abs() < 1e-9);
        assert!((d.tx - 10.0).abs() < 1e-9 && (d.ty - 20.0).abs() < 1e-9);
    }

    #[test]
    fn mirrored_matrices_keep_a_negative_scale() {
        let d = decompose(Affine::scale_non_uniform(-1.0, 1.0));
        assert!(d.sx * d.sy < 0.0);
        assert!(near(compose(&d), Affine::scale_non_uniform(-1.0, 1.0)));
    }

    #[test]
    fn junk_is_identity() {
        assert!(near(compose(&decompose(Affine::new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]))), Affine::IDENTITY));
        assert!(compose(&decompose(Affine::new([0.0; 6]))).as_coeffs().iter().all(|v| v.is_finite()));
    }

    #[test]
    fn auto_rotation_takes_the_short_way() {
        let a = Affine::rotate(170f64.to_radians());
        let b = Affine::rotate(-170f64.to_radians());
        let mid = decompose(interpolate(a, b, Progress::uniform(0.5), &TweenParams::default()));
        assert!((wrap_angle(mid.rotation) - PI).abs() < 1e-6, "halfway is 180 degrees, got {}", mid.rotation);
    }

    #[test]
    fn clockwise_and_counter_clockwise_add_turns() {
        let a = Affine::IDENTITY;
        let b = Affine::rotate(90f64.to_radians());
        let cw = TweenParams { rotate: TweenRotate::Clockwise, times: 2.0, scale: true };
        let end = decompose(interpolate(a, b, Progress::uniform(1.0), &cw));
        assert!(wrap_angle(end.rotation - 90f64.to_radians()).abs() < 1e-6, "after the extra turns the tween ends at the target");
        // Halfway is 90 degrees plus two turns, halved: 2.25 turns, a quarter turn on screen.
        let half = decompose(interpolate(a, b, Progress::uniform(0.5), &cw));
        assert!(wrap_angle(half.rotation - FRAC_PI_2 / 2.0).abs() < 1e-6, "{}", half.rotation);
        let ccw = TweenParams { rotate: TweenRotate::CounterClockwise, times: 1.0, scale: true };
        let back = decompose(interpolate(a, b, Progress::uniform(0.25), &ccw));
        assert!(back.rotation < 0.0, "counter-clockwise starts negative");
        let end = decompose(interpolate(a, b, Progress::uniform(1.0), &ccw));
        assert!(near(compose(&end), b));
    }

    #[test]
    fn none_keeps_the_start_rotation() {
        let b = Affine::rotate(1.0) * Affine::translate((5.0, 0.0));
        let p = TweenParams { rotate: TweenRotate::None, times: 1.0, scale: true };
        let mid = decompose(interpolate(Affine::IDENTITY, b, Progress::uniform(0.5), &p));
        assert!(mid.rotation.abs() < 1e-9);
    }

    #[test]
    fn scale_flag_holds_the_start_scale() {
        let b = Affine::scale(4.0);
        let p = TweenParams { rotate: TweenRotate::Auto, times: 1.0, scale: false };
        let m = interpolate(Affine::IDENTITY, b, Progress::uniform(0.5), &p);
        assert!(near(m, Affine::IDENTITY));
        let m = interpolate(Affine::IDENTITY, b, Progress::uniform(0.5), &TweenParams::default());
        assert!(near(m, Affine::scale(2.5)));
    }

    #[test]
    fn per_property_progress() {
        let b = Affine::translate((10.0, 0.0)) * Affine::scale(3.0);
        let progress = Progress { position: 0.5, rotation: 0.0, scale: 0.0 };
        let m = interpolate(Affine::IDENTITY, b, progress, &TweenParams::default());
        assert!(near(m, Affine::translate((5.0, 0.0))));
    }

    proptest! {
        #[test]
        fn decompose_compose_round_trips(a in -5.0f64..5.0, b in -5.0f64..5.0, c in -5.0f64..5.0, d in -5.0f64..5.0, tx in -100.0f64..100.0, ty in -100.0f64..100.0) {
            let m = Affine::new([a, b, c, d, tx, ty]);
            let det = a.mul_add(d, -(b * c));
            prop_assume!(det.abs() > 1e-3 && a.hypot(b) > 1e-3);
            prop_assert!(near(compose(&decompose(m)), m));
        }

        #[test]
        fn interpolation_is_finite(
            a in -5.0f64..5.0, b in -5.0f64..5.0, c in -5.0f64..5.0, d in -5.0f64..5.0,
            e in -5.0f64..5.0, f in -5.0f64..5.0, g in -5.0f64..5.0, h in -5.0f64..5.0,
            t in 0.0f64..1.0, times in -2.0f64..4.0,
        ) {
            for rotate in [TweenRotate::Auto, TweenRotate::None, TweenRotate::Clockwise, TweenRotate::CounterClockwise] {
                let params = TweenParams { rotate, times, scale: true };
                let m = interpolate(Affine::new([a, b, c, d, 0.0, 0.0]), Affine::new([e, f, g, h, 1.0, 1.0]), Progress::uniform(t), &params);
                prop_assert!(m.as_coeffs().iter().all(|v| v.is_finite()));
            }
        }
    }
}
