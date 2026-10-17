//! Interpolating fill and stroke styles.
//!
//! Colours, gradient matrices, gradient stops and stroke widths blend smoothly. Things that cannot blend (a linear
//! against a radial gradient, two different bitmaps, caps and joins) switch at the halfway point.

use anim_core::num::lerp;
use anim_core::{Affine, Rgba8};
use anim_display::GradientStop;
use anim_shape::{BitmapFill, FillStyle, GradientFill, SolidStroke, StrokeStyle};

/// Most stops an interpolated gradient keeps.
const MAX_STOPS: usize = 32;

/// Coefficient-wise blend of two affine maps (what Animate does for gradient and bitmap matrices).
#[must_use]
pub fn lerp_affine(a: Affine, b: Affine, t: f64) -> Affine {
    let (ca, cb) = (a.as_coeffs(), b.as_coeffs());
    let mut out = [0.0; 6];
    for ((slot, x), y) in out.iter_mut().zip(ca).zip(cb) {
        *slot = lerp(x, y, t);
    }
    Affine::new(out)
}

fn color_at(stops: &[GradientStop], offset: f32) -> Rgba8 {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else { return Rgba8::TRANSPARENT };
    if offset <= first.offset {
        return first.color;
    }
    for (lo, hi) in stops.iter().zip(stops.iter().skip(1)) {
        if offset <= hi.offset {
            let span = f64::from(hi.offset - lo.offset);
            let f = if span > 0.0 { f64::from(offset - lo.offset) / span } else { 1.0 };
            return lo.color.lerp(hi.color, f);
        }
    }
    last.color
}

/// Blends two stop lists over the union of their offsets.
#[must_use]
pub fn lerp_stops(a: &[GradientStop], b: &[GradientStop], t: f64) -> Vec<GradientStop> {
    let mut offsets: Vec<f32> = a.iter().chain(b).map(|s| s.offset).filter(|o| o.is_finite()).collect();
    offsets.sort_by(f32::total_cmp);
    offsets.dedup_by(|x, y| (*x - *y).abs() < 1.0e-6);
    if offsets.len() > MAX_STOPS {
        // Keep an even spread, always including both ends.
        let step = offsets.len().div_ceil(MAX_STOPS);
        let last = offsets.last().copied();
        offsets = offsets.into_iter().step_by(step).collect();
        if let (Some(last), Some(have)) = (last, offsets.last().copied())
            && last > have
        {
            offsets.push(last);
        }
    }
    offsets.into_iter().map(|offset| GradientStop { offset, color: color_at(a, offset).lerp(color_at(b, offset), t) }).collect()
}

fn lerp_gradient(a: &GradientFill, b: &GradientFill, t: f64) -> GradientFill {
    let from_b = t >= 0.5;
    GradientFill {
        matrix: lerp_affine(a.matrix, b.matrix, t),
        stops: lerp_stops(&a.stops, &b.stops, t),
        spread: if from_b { b.spread } else { a.spread },
        interpolation: if from_b { b.interpolation } else { a.interpolation },
        focal: lerp(a.focal, b.focal, t),
    }
}

/// A solid colour as a gradient shaped like `like`, so it can fade into it.
fn gradient_of_solid(color: Rgba8, like: &GradientFill) -> GradientFill {
    GradientFill { stops: vec![GradientStop { offset: 0.0, color }, GradientStop { offset: 1.0, color }], ..like.clone() }
}

fn lerp_bitmap(a: &BitmapFill, b: &BitmapFill, t: f64) -> BitmapFill {
    BitmapFill { matrix: lerp_affine(a.matrix, b.matrix, t), ..if t >= 0.5 { b.clone() } else { a.clone() } }
}

/// The fill `t` of the way from `a` to `b`.
#[must_use]
pub fn lerp_fill(from: &FillStyle, to: &FillStyle, t: f64) -> FillStyle {
    match (from, to) {
        (FillStyle::Solid(c0), FillStyle::Solid(c1)) => FillStyle::Solid(c0.lerp(*c1, t)),
        (FillStyle::LinearGradient(g0), FillStyle::LinearGradient(g1)) => FillStyle::LinearGradient(lerp_gradient(g0, g1, t)),
        (FillStyle::RadialGradient(g0), FillStyle::RadialGradient(g1)) => FillStyle::RadialGradient(lerp_gradient(g0, g1, t)),
        (FillStyle::Solid(c0), FillStyle::LinearGradient(g1)) => FillStyle::LinearGradient(lerp_gradient(&gradient_of_solid(*c0, g1), g1, t)),
        (FillStyle::Solid(c0), FillStyle::RadialGradient(g1)) => FillStyle::RadialGradient(lerp_gradient(&gradient_of_solid(*c0, g1), g1, t)),
        (FillStyle::LinearGradient(g0), FillStyle::Solid(c1)) => FillStyle::LinearGradient(lerp_gradient(g0, &gradient_of_solid(*c1, g0), t)),
        (FillStyle::RadialGradient(g0), FillStyle::Solid(c1)) => FillStyle::RadialGradient(lerp_gradient(g0, &gradient_of_solid(*c1, g0), t)),
        (FillStyle::Bitmap(m0), FillStyle::Bitmap(m1)) if m0.path == m1.path => FillStyle::Bitmap(lerp_bitmap(m0, m1, t)),
        _ => {
            if t >= 0.5 {
                to.clone()
            } else {
                from.clone()
            }
        }
    }
}

const fn solid_of(style: &StrokeStyle) -> &SolidStroke {
    match style {
        StrokeStyle::Solid(s) => s,
        StrokeStyle::Other(o) => &o.base,
    }
}

/// The stroke `t` of the way from `a` to `b`: weight, colour and miter limit blend, the rest switches at half.
/// Patterned strokes blend as their solid base.
#[must_use]
pub fn lerp_stroke(from: &StrokeStyle, to: &StrokeStyle, t: f64) -> StrokeStyle {
    let (s0, s1) = (solid_of(from), solid_of(to));
    let pick = if t >= 0.5 { s1 } else { s0 };
    StrokeStyle::Solid(SolidStroke {
        weight: lerp(s0.weight, s1.weight, t),
        fill: lerp_fill(&s0.fill, &s1.fill, t),
        miter_limit: lerp(s0.miter_limit, s1.miter_limit, t),
        caps: pick.caps,
        joints: pick.joints,
        scale_mode: pick.scale_mode,
        pixel_hinting: pick.pixel_hinting,
    })
}
