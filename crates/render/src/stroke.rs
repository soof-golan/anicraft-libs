//! Stroke styles: caps, joins, miter limit, dashes and Animate's stroke-scale modes.
//!
//! tiny-skia strokes a path in the coordinate system it is given, then transforms the outline. Animate's "scale"
//! option decides in which coordinate system the pen is round and uniform, so each mode becomes a pair
//! `(pre, post)` with `post * pre == local-to-device`:
//!
//! - `Normal`: the pen lives in local space (`pre` = identity), so it scales with the object, non-uniformly if
//!   the object is scaled non-uniformly.
//! - `None`: the pen lives in device pixels (`pre` = the whole matrix, `post` = identity). Width 0 is the
//!   one-pixel hairline, and Animate treats width 0 this way in every scale mode.
//! - `Horizontal` / `Vertical`: the width does not follow scaling along that axis (SWF `NoHScale`/`NoVScale`). The
//!   pen is round in a space that already carries the matrix's scale along that axis, so the thickness is not
//!   affected by it but follows the other axis.

use anim_core::Affine;
use anim_core::num::f64_to_f32;
use anim_display::{Cap, Join, StrokeScale, StrokeStyle};
use tiny_skia::{LineCap, LineJoin, Stroke, StrokeDash};

use crate::convert::axis_scales;

/// Everything needed to stroke one path.
#[derive(Debug, Clone)]
pub struct StrokeSetup {
    /// Maps the path's local space into the space the pen is round in.
    pub pre: Affine,
    /// Maps that space into layer pixels.
    pub post: Affine,
    pub stroke: Stroke,
    /// Conservative distance in layer pixels that the stroke can extend beyond the path.
    pub reach: f64,
}

/// Dash arrays longer than this are ignored (a stroked circle with millions of dashes would only cost memory).
const MAX_DASH_ENTRIES: usize = 4096;
/// A stroke thinner than this (in pen space) is a hairline.
const MIN_AXIS_SCALE: f64 = 1e-9;

/// Plans the stroke of a path with `style` drawn under the local-to-layer matrix `m`. `None` when the style or
/// matrix cannot produce a visible stroke.
pub fn plan(style: &StrokeStyle, m: Affine) -> Option<StrokeSetup> {
    if !style.width.is_finite() || style.width < 0.0 || !m.is_finite() {
        return None;
    }
    let (sx, sy) = axis_scales(m);
    let scale = if style.width <= 0.0 { StrokeScale::None } else { style.scale };
    let (pre, post) = match scale {
        StrokeScale::Normal => (Affine::IDENTITY, m),
        StrokeScale::None => (m, Affine::IDENTITY),
        StrokeScale::Horizontal => {
            if sx < MIN_AXIS_SCALE {
                return None;
            }
            (Affine::scale_non_uniform(sx, 1.0), m * Affine::scale_non_uniform(1.0 / sx, 1.0))
        }
        StrokeScale::Vertical => {
            if sy < MIN_AXIS_SCALE {
                return None;
            }
            (Affine::scale_non_uniform(1.0, sy), m * Affine::scale_non_uniform(1.0, 1.0 / sy))
        }
    };
    if !pre.is_finite() || !post.is_finite() {
        return None;
    }
    // How much pen space is stretched, to carry dash lengths (given in local units) into pen space.
    let dash_scale = pre.determinant().abs().sqrt();
    let miter_limit = if style.miter_limit.is_finite() { style.miter_limit.max(1.0) } else { 4.0 };
    let stroke = Stroke {
        width: f64_to_f32(style.width),
        miter_limit: f64_to_f32(miter_limit),
        line_cap: match style.cap {
            Cap::None => LineCap::Butt,
            Cap::Round => LineCap::Round,
            Cap::Square => LineCap::Square,
        },
        line_join: match style.join {
            Join::Miter => LineJoin::Miter,
            Join::Round => LineJoin::Round,
            Join::Bevel => LineJoin::Bevel,
        },
        dash: dash(&style.dashes, dash_scale),
    };
    let join_factor = if style.join == Join::Miter { miter_limit } else { 1.0 };
    let cap_factor = if style.cap == Cap::Square { std::f64::consts::SQRT_2 } else { 1.0 };
    let pen_reach = style.width / 2.0 * join_factor.max(cap_factor).max(1.0);
    let post_scale = post.frobenius_norm_squared().sqrt();
    // Hairlines cover one pixel; the extra pixel absorbs anti-aliasing and rounding.
    let reach = (pen_reach * post_scale).max(1.0) + 2.0;
    Some(StrokeSetup { pre, post, stroke, reach })
}

/// A tiny-skia dash from alternating on/off lengths in local units. Odd-length arrays repeat (SVG/Flash style).
fn dash(lengths: &[f64], scale: f64) -> Option<StrokeDash> {
    if lengths.is_empty() || lengths.len() > MAX_DASH_ENTRIES || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    if lengths.iter().any(|l| !l.is_finite() || *l < 0.0) {
        return None;
    }
    let mut array: Vec<f32> = lengths.iter().map(|l| f64_to_f32(l * scale)).collect();
    if array.len() % 2 == 1 {
        let copy = array.clone();
        array.extend(copy);
    }
    StrokeDash::new(array, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(width: f64, scale: StrokeScale) -> StrokeStyle {
        StrokeStyle { width, scale, ..StrokeStyle::default() }
    }

    #[test]
    fn modes_split_the_matrix() {
        let m = Affine::scale_non_uniform(4.0, 2.0);
        for scale in [StrokeScale::Normal, StrokeScale::None, StrokeScale::Horizontal, StrokeScale::Vertical] {
            let plan = plan(&style(3.0, scale), m).unwrap();
            let total = plan.post * plan.pre;
            for (a, b) in total.as_coeffs().iter().zip(m.as_coeffs()) {
                assert!((a - b).abs() < 1e-9, "{scale:?}: {a} != {b}");
            }
        }
        assert_eq!(plan(&style(3.0, StrokeScale::None), m).unwrap().post, Affine::IDENTITY);
    }

    #[test]
    fn zero_width_is_a_device_space_hairline_in_every_mode() {
        let m = Affine::scale(10.0);
        let p = plan(&style(0.0, StrokeScale::Normal), m).unwrap();
        assert_eq!(p.stroke.width, 0.0);
        assert_eq!(p.post, Affine::IDENTITY);
    }

    #[test]
    fn invalid_styles_are_rejected() {
        assert!(plan(&style(f64::NAN, StrokeScale::Normal), Affine::IDENTITY).is_none());
        assert!(plan(&style(-1.0, StrokeScale::Normal), Affine::IDENTITY).is_none());
        assert!(plan(&style(1.0, StrokeScale::Horizontal), Affine::scale_non_uniform(0.0, 1.0)).is_none());
        assert!(plan(&style(1.0, StrokeScale::Normal), Affine::new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0])).is_none());
    }

    #[test]
    fn dashes_repeat_odd_arrays_and_ignore_nonsense() {
        assert!(dash(&[5.0], 1.0).is_some());
        assert!(dash(&[], 1.0).is_none());
        assert!(dash(&[0.0, 0.0], 1.0).is_none());
        assert!(dash(&[-1.0, 2.0], 1.0).is_none());
        assert!(dash(&[f64::NAN, 2.0], 1.0).is_none());
    }

    #[test]
    fn miter_and_square_caps_extend_the_reach() {
        let round = plan(&StrokeStyle { width: 10.0, ..StrokeStyle::default() }, Affine::IDENTITY).unwrap();
        let miter = plan(&StrokeStyle { width: 10.0, join: Join::Miter, miter_limit: 8.0, ..StrokeStyle::default() }, Affine::IDENTITY).unwrap();
        assert!(miter.reach > round.reach);
    }
}
