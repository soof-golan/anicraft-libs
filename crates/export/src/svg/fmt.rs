//! Number, colour and path formatting for SVG output.

use anim_core::kurbo::PathEl;
use anim_core::{Affine, BezPath, Point, Rgba8};

/// Formats `v` with at most `precision` decimals and no trailing zeros. Non-finite values become `0` and
/// magnitudes are clamped to 1e9 so a hostile coordinate cannot produce a 300-digit number.
pub(super) fn num(v: f64, precision: u8) -> String {
    let mut s = String::new();
    push_num(&mut s, v, precision);
    s
}

const POW10: [u32; 10] = [1, 10, 100, 1000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000];

/// Appends [`num`]'s text to `out`.
pub(super) fn push_num(out: &mut String, v: f64, precision: u8) {
    if !v.is_finite() {
        out.push('0');
        return;
    }
    let v = v.clamp(-1e9, 1e9);
    let places = usize::from(precision.min(9));
    if push_fixed(out, v, places) {
        return;
    }
    let mut s = format!("{v:.places$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        out.push('0');
    } else {
        out.push_str(&s);
    }
}

/// The fast path of [`push_num`]: scales to an integer and writes the digits by hand. Declines (returns `false`, writing
/// nothing) when `v` scaled is so close to a rounding tie, or so large, that `format!` could round the exact binary
/// value differently from the scaled `f64`.
fn push_fixed(out: &mut String, v: f64, places: usize) -> bool {
    let Some(&scale) = POW10.get(places) else { return false };
    let scaled = v.abs() * f64::from(scale);
    let fraction = scaled - scaled.floor();
    // `scaled` carries one rounding error of at most scaled * 2^-53; stay well clear of it.
    if scaled >= 4e15 || (fraction - 0.5).abs() <= scaled.mul_add(1e-15, 1e-9) {
        return false;
    }
    let Ok(rounded) = u64::try_from(anim_core::num::round_f64_to_i64(scaled)) else { return false };
    let scale = u64::from(scale);
    let (whole, part) = (rounded.checked_div(scale).unwrap_or(0), rounded.checked_rem(scale).unwrap_or(0));
    if rounded != 0 && v < 0.0 {
        out.push('-');
    }
    let mut digits = [b'0'; 29];
    let (mut at, mut rest) = (digits.len(), part);
    // The `places` fraction digits, least significant first, then the whole part.
    for _ in 0..places {
        at = at.saturating_sub(1);
        set_digit(&mut digits, at, rest);
        rest = rest.checked_div(10).unwrap_or(0);
    }
    let fraction_start = at;
    let mut rest = whole;
    loop {
        at = at.saturating_sub(1);
        set_digit(&mut digits, at, rest);
        rest = rest.checked_div(10).unwrap_or(0);
        if rest == 0 {
            break;
        }
    }
    out.extend(digits.get(at..fraction_start).unwrap_or_default().iter().map(|&d| char::from(d)));
    if part != 0 {
        let fraction = digits.get(fraction_start..).unwrap_or_default();
        let end = fraction.iter().rposition(|&d| d != b'0').map_or(0, |last| last.saturating_add(1));
        out.push('.');
        out.extend(fraction.get(..end).unwrap_or_default().iter().map(|&d| char::from(d)));
    }
    true
}

fn set_digit(digits: &mut [u8], at: usize, n: u64) {
    if let Some(slot) = digits.get_mut(at) {
        *slot = b'0'.saturating_add(u8::try_from(n.checked_rem(10).unwrap_or(0)).unwrap_or(0));
    }
}

pub(super) fn is_identity(m: Affine) -> bool {
    let c = m.as_coeffs();
    let id = Affine::IDENTITY.as_coeffs();
    c.iter().zip(id.iter()).all(|(a, b)| (a - b).abs() < 1e-12)
}

/// `matrix(a b c d e f)` or `None` for the identity.
pub(super) fn matrix(m: Affine, precision: u8) -> Option<String> {
    if is_identity(m) {
        return None;
    }
    // Matrix entries need more digits than coordinates: a 2 decimal rotation is visibly wrong.
    let p = precision.max(5);
    let mut s = String::from("matrix(");
    for (i, v) in m.as_coeffs().iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        push_num(&mut s, *v, if i >= 4 { precision } else { p });
    }
    s.push(')');
    Some(s)
}

/// ` transform="matrix(...)"` (with the leading space) or an empty string for the identity.
pub(super) fn transform_attr(m: Affine, precision: u8) -> String {
    matrix(m, precision).map_or_else(String::new, |t| format!(" transform=\"{t}\""))
}

pub(super) fn hex(c: Rgba8) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// Alpha 0 ..= 255 as an opacity attribute value.
pub(super) fn opacity(a: u8) -> String {
    num(f64::from(a) / 255.0, 4)
}

/// SVG path data, or `None` when the path is empty or has a non-finite coordinate.
pub(super) fn path_data(path: &BezPath, precision: u8) -> Option<String> {
    let mut d = String::with_capacity(path.elements().len().saturating_mul(16));
    for el in path.elements() {
        let pts: &[Point] = match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => std::slice::from_ref(p),
            PathEl::QuadTo(a, b) => &[*a, *b],
            PathEl::CurveTo(a, b, c) => &[*a, *b, *c],
            PathEl::ClosePath => &[],
        };
        if pts.iter().any(|p| !(p.x.is_finite() && p.y.is_finite())) {
            return None;
        }
        if !d.is_empty() {
            d.push(' ');
        }
        match el {
            PathEl::MoveTo(_) => d.push('M'),
            PathEl::LineTo(_) => d.push('L'),
            PathEl::QuadTo(..) => d.push('Q'),
            PathEl::CurveTo(..) => d.push('C'),
            PathEl::ClosePath => d.push('Z'),
        }
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                d.push(' ');
            }
            push_num(&mut d, p.x, precision);
            d.push(' ');
            push_num(&mut d, p.y, precision);
        }
    }
    if d.is_empty() { None } else { Some(d) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_safe() {
        assert_eq!(num(1.0, 3), "1");
        assert_eq!(num(1.23456, 3), "1.235");
        assert_eq!(num(-0.0001, 3), "0");
        assert_eq!(num(f64::NAN, 3), "0");
        assert_eq!(num(1e300, 3), "1000000000");
        assert_eq!(num(2.50, 3), "2.5");
    }

    /// The straightforward `format!` version that `num` replaced.
    fn reference_num(v: f64, precision: u8) -> String {
        if !v.is_finite() {
            return "0".to_owned();
        }
        let places = usize::from(precision.min(9));
        let mut s = format!("{:.places$}", v.clamp(-1e9, 1e9));
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
        }
        if s == "-0" { "0".to_owned() } else { s }
    }

    #[test]
    fn fast_numbers_match_format() {
        let mut seed = 99_u64;
        let mut next_u32 = move || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            u32::try_from(seed >> 32).unwrap_or(0)
        };
        let mut values = vec![0.0, -0.0, 0.5, -0.5, 1.5, 2.5, 0.125, 0.0005, 0.000_499_99, 1e-12, 999_999_999.9996, 1e9, -1e9, 1e12, f64::MIN_POSITIVE];
        for _ in 0..20_000 {
            let unit = |n: u32| f64::from(n) / 4_294_967_296.0;
            let exponent = i32::try_from(next_u32() % 12).unwrap_or(0) - 3;
            values.push((unit(next_u32()) - 0.5) * 10_f64.powi(exponent));
            // Values with exactly `k` decimal digits, the hardest rounding cases.
            values.push((unit(next_u32()) * 100_000.0).round() / 1000.0);
            values.push((unit(next_u32()) * 1_000_000.0).round() / 10_000.0 + 0.00005);
        }
        for v in values {
            for precision in [0, 1, 2, 3, 4, 5, 9, 12] {
                assert_eq!(num(v, precision), reference_num(v, precision), "{v} at {precision}");
            }
        }
    }

    #[test]
    fn identity_has_no_matrix() {
        assert!(matrix(Affine::IDENTITY, 3).is_none());
        assert_eq!(matrix(Affine::translate((2.0, 3.0)), 3).as_deref(), Some("matrix(1 0 0 1 2 3)"));
    }

    #[test]
    fn path_data_rejects_non_finite() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((f64::INFINITY, 1.0));
        assert!(path_data(&p, 3).is_none());
        let mut q = BezPath::new();
        q.move_to((0.0, 0.0));
        q.quad_to((1.0, 1.0), (2.0, 0.0));
        q.close_path();
        assert_eq!(path_data(&q, 3).as_deref(), Some("M0 0 Q1 1 2 0 Z"));
    }
}
