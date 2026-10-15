//! Easing: Flash's classic acceleration ease, named presets and custom bezier curves.
//!
//! Every function maps tween progress `p` (0 ..= 1) to eased progress. The end points are exact: `ease(0) = 0` and
//! `ease(1) = 1`.

use std::f64::consts::{FRAC_PI_2, TAU};

use anim_scene::{EaseCurve, EaseTarget, Frame};

/// Flash's classic ease. `acceleration` is the XFL value in -100 ..= 100 (negative eases in, positive eases out).
///
/// With `e = acceleration / 100` the eased progress is `p' = p + e * p * (1 - p)`. A strength of `+1` is the
/// quadratic ease-out `p * (2 - p)`, `-1` is the quadratic ease-in `p * p`, and `0` is linear. In between it is a
/// blend of linear and quadratic, which is how Flash's slider behaves.
#[must_use]
pub fn classic(acceleration: i32, p: f64) -> f64 {
    let e = (f64::from(acceleration) / 100.0).clamp(-1.0, 1.0);
    let p = p.clamp(0.0, 1.0);
    (e * p).mul_add(1.0 - p, p)
}

/// Eased progress of a keyframe for one property `target`, from its `<tweens>` entries, else the classic
/// acceleration.
#[must_use]
pub fn for_target(frame: &Frame, target: EaseTarget, p: f64) -> f64 {
    let p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 };
    match frame.ease_for(target) {
        Some(EaseCurve::Custom(points)) => custom(points, p),
        Some(EaseCurve::Named(name)) => named(name, p).unwrap_or_else(|| classic(frame.acceleration, p)),
        None => {
            frame.ease_method_name.as_deref().filter(|_| frame.acceleration == 0).and_then(|n| named(n, p)).unwrap_or_else(|| classic(frame.acceleration, p))
        }
    }
}

/// A named preset such as `cubicIn`, `sineInOut` or `bounceOut`. `None` when the name is unknown.
#[must_use]
pub fn named(name: &str, p: f64) -> Option<f64> {
    let p = p.clamp(0.0, 1.0);
    if name.eq_ignore_ascii_case("linear") || name.eq_ignore_ascii_case("none") {
        return Some(p);
    }
    let (family, mode) = name
        .strip_suffix("InOut")
        .map(|f| (f, Mode::InOut))
        .or_else(|| name.strip_suffix("In").map(|f| (f, Mode::In)))
        .or_else(|| name.strip_suffix("Out").map(|f| (f, Mode::Out)))?;
    let base: fn(f64) -> f64 = match family {
        "sine" => |t| 1.0 - (t * FRAC_PI_2).cos(),
        "quad" => |t| t * t,
        "cubic" => |t| t * t * t,
        "quart" => |t| t.powi(4),
        "quint" => |t| t.powi(5),
        "expo" => |t| if t <= 0.0 { 0.0 } else { 10.0f64.mul_add(t, -10.0).exp2() },
        "circ" => |t| 1.0 - t.mul_add(-t, 1.0).max(0.0).sqrt(),
        "back" => |t| t * t * (BACK_C3.mul_add(t, -BACK_C1)),
        "elastic" => elastic_in,
        "bounce" => |t| 1.0 - bounce_out(1.0 - t),
        _ => return None,
    };
    let v = match mode {
        Mode::In => base(p),
        Mode::Out => 1.0 - base(1.0 - p),
        Mode::InOut => {
            if p < 0.5 {
                base(2.0 * p) / 2.0
            } else {
                1.0 - base(2.0f64.mul_add(-p, 2.0)) / 2.0
            }
        }
    };
    // Bounce and elastic overshoot by design; the end points must still be exact.
    Some(if p <= 0.0 {
        0.0
    } else if p >= 1.0 {
        1.0
    } else {
        v
    })
}

const BACK_C1: f64 = 1.70158;
const BACK_C3: f64 = BACK_C1 + 1.0;

#[derive(Clone, Copy)]
enum Mode {
    In,
    Out,
    InOut,
}

fn elastic_in(t: f64) -> f64 {
    if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else {
        -(10.0f64.mul_add(t, -10.0).exp2()) * ((t.mul_add(10.0, -10.75)) * (TAU / 3.0)).sin()
    }
}

fn bounce_out(t: f64) -> f64 {
    const N: f64 = 7.5625;
    const D: f64 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        (N * t).mul_add(t, 0.75)
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        (N * t).mul_add(t, 0.9375)
    } else {
        let t = t - 2.625 / D;
        (N * t).mul_add(t, 0.984_375)
    }
}

/// A custom ease from Animate's `<CustomEase>` points.
///
/// The points are anchors with two control points between each pair (`1 + 3k` points, `(0,0)` first and `(1,1)`
/// last, coordinates as `[time, value]`). Other point counts are read as a polyline through the points sorted by
/// time.
#[must_use]
pub fn custom(points: &[[f64; 2]], p: f64) -> f64 {
    let p = p.clamp(0.0, 1.0);
    if points.len() < 2 || points.iter().any(|q| !q[0].is_finite() || !q[1].is_finite()) {
        return p;
    }
    let bezier_chain = points.len() >= 4 && points.len().checked_sub(1).is_some_and(|n| n % 3 == 0);
    if bezier_chain { bezier_chain_value(points, p) } else { polyline_value(points, p) }
}

fn bezier_chain_value(points: &[[f64; 2]], progress: f64) -> f64 {
    let mut start = 0usize;
    while let Some(seg) = points.get(start..start.saturating_add(4)) {
        let &[from, c1, c2, to] = seg else { break };
        let last_segment = points.len().saturating_sub(start.saturating_add(4)) == 0;
        if progress <= to[0] || last_segment {
            return solve_segment([from, c1, c2, to], progress);
        }
        start = start.saturating_add(3);
    }
    progress
}

/// Solves `x(u) = progress` on one cubic segment by bisection (x is assumed monotonic) and returns `y(u)`.
fn solve_segment(seg: [[f64; 2]; 4], progress: f64) -> f64 {
    let at = |u: f64, axis: usize| -> f64 {
        let w = 1.0 - u;
        let [from, c1, c2, to] = seg.map(|q| q.get(axis).copied().unwrap_or(0.0));
        w.powi(3).mul_add(from, (3.0 * w * w * u).mul_add(c1, (3.0 * w * u * u).mul_add(c2, u.powi(3) * to)))
    };
    let [from, _, _, to] = seg;
    if progress <= from[0] {
        return from[1];
    }
    if progress >= to[0] {
        return to[1];
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..40 {
        let mid = f64::midpoint(lo, hi);
        if at(mid, 0) < progress {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(f64::midpoint(lo, hi), 1)
}

fn polyline_value(points: &[[f64; 2]], p: f64) -> f64 {
    let mut sorted: Vec<[f64; 2]> = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut prev: Option<[f64; 2]> = None;
    for q in sorted {
        if let Some(a) = prev
            && p <= q[0]
        {
            let span = q[0] - a[0];
            if span <= f64::EPSILON {
                return q[1];
            }
            return (q[1] - a[1]).mul_add((p - a[0]) / span, a[1]);
        }
        prev = Some(q);
    }
    prev.map_or(p, |q| q[1])
}

#[cfg(test)]
mod tests {
    use anim_scene::{EaseCurve, EaseTarget, Frame, TweenEase};

    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn classic_matches_quadratic_extremes() {
        assert!(close(classic(0, 0.3), 0.3));
        assert!(close(classic(100, 0.5), 0.75));
        assert!(close(classic(-100, 0.5), 0.25));
        assert!(close(classic(100, 0.2), 0.2 * 1.8));
        assert!(close(classic(50, 0.5), 0.625));
        assert!(close(classic(500, 0.5), 0.75), "strength is clamped");
    }

    #[test]
    fn classic_end_points_are_exact() {
        for a in [-100, -37, 0, 64, 100] {
            assert!(close(classic(a, 0.0), 0.0));
            assert!(close(classic(a, 1.0), 1.0));
        }
    }

    #[test]
    fn named_presets() {
        assert!(close(named("cubicIn", 0.5).unwrap_or(-1.0), 0.125));
        assert!(close(named("quadOut", 0.5).unwrap_or(-1.0), 0.75));
        assert!(close(named("quadInOut", 0.25).unwrap_or(-1.0), 0.125));
        assert!(close(named("linear", 0.4).unwrap_or(-1.0), 0.4));
        assert!(named("wobbleIn", 0.4).is_none());
        for name in ["sine", "quad", "cubic", "quart", "quint", "expo", "circ", "back", "elastic", "bounce"] {
            for mode in ["In", "Out", "InOut"] {
                let n = format!("{name}{mode}");
                assert!(close(named(&n, 0.0).unwrap_or(-1.0), 0.0), "{n} start");
                assert!(close(named(&n, 1.0).unwrap_or(-1.0), 1.0), "{n} end");
                assert!(named(&n, 0.37).is_some_and(f64::is_finite), "{n} mid");
            }
        }
    }

    #[test]
    fn custom_bezier_follows_the_curve() {
        // The sample's ease: slow start, fast middle.
        let pts = [[0.0, 0.0], [0.443, 0.1337], [0.7498, 0.4956], [1.0, 1.0]];
        assert!(close(custom(&pts, 0.0), 0.0));
        assert!(close(custom(&pts, 1.0), 1.0));
        let mid = custom(&pts, 0.5);
        assert!(mid > 0.0 && mid < 1.0);
        assert!(custom(&pts, 0.25) < custom(&pts, 0.75));
        // A straight diagonal is linear.
        let lin = [[0.0, 0.0], [1.0 / 3.0, 1.0 / 3.0], [2.0 / 3.0, 2.0 / 3.0], [1.0, 1.0]];
        assert!(close(custom(&lin, 0.3), 0.3));
    }

    #[test]
    fn custom_chain_of_two_segments() {
        let pts = [[0.0, 0.0], [0.1, 0.0], [0.4, 0.0], [0.5, 0.0], [0.6, 1.0], [0.9, 1.0], [1.0, 1.0]];
        assert!(custom(&pts, 0.25) < 0.01);
        assert!(custom(&pts, 0.75) > 0.8);
        assert!(close(custom(&pts, 0.5), 0.0), "the anchor between the segments");
    }

    #[test]
    fn custom_polyline_and_junk() {
        let poly = [[0.0, 0.0], [0.5, 1.0], [1.0, 1.0]];
        assert!(close(custom(&poly, 0.25), 0.5));
        assert!(close(custom(&[], 0.3), 0.3));
        assert!(close(custom(&[[f64::NAN, 0.0], [1.0, 1.0]], 0.3), 0.3));
    }

    #[test]
    fn for_target_precedence() {
        let mut f = Frame::blank(0, 10);
        f.acceleration = 100;
        assert!(close(for_target(&f, EaseTarget::Position, 0.5), 0.75));
        f.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Named("cubicIn".into()) });
        assert!(close(for_target(&f, EaseTarget::Position, 0.5), 0.125));
        f.eases.push(TweenEase { target: EaseTarget::Rotation, curve: EaseCurve::Named("linear".into()) });
        assert!(close(for_target(&f, EaseTarget::Rotation, 0.5), 0.5));
        assert!(close(for_target(&f, EaseTarget::Scale, 0.5), 0.125), "falls back to the all entry");
        assert!(close(for_target(&f, EaseTarget::Scale, f64::NAN), 0.0));
    }
}
