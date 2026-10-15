//! Number, colour and path formatting for SVG output.

use anim_core::kurbo::PathEl;
use anim_core::{Affine, BezPath, Point, Rgba8};

/// Formats `v` with at most `precision` decimals and no trailing zeros. Non-finite values become `0` and
/// magnitudes are clamped to 1e9 so a hostile coordinate cannot produce a 300-digit number.
pub(super) fn num(v: f64, precision: u8) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let v = v.clamp(-1e9, 1e9);
    let places = usize::from(precision.min(9));
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
        "0".clone_into(&mut s);
    }
    s
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
    let c = m.as_coeffs();
    let parts: Vec<String> = c.iter().enumerate().map(|(i, v)| num(*v, if i >= 4 { precision } else { p })).collect();
    Some(format!("matrix({})", parts.join(" ")))
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

fn point(p: Point, precision: u8) -> String {
    format!("{} {}", num(p.x, precision), num(p.y, precision))
}

/// SVG path data, or `None` when the path is empty or has a non-finite coordinate.
pub(super) fn path_data(path: &BezPath, precision: u8) -> Option<String> {
    let mut d = String::new();
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
        let coords: Vec<String> = pts.iter().map(|p| point(*p, precision)).collect();
        d.push_str(&coords.join(" "));
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
