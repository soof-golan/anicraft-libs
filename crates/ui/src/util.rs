//! Small numeric helpers: the only place where float and integer worlds meet in this crate.

use anim_core::num;
use egui::Color32;

/// `u32` to `f32` (exact up to 2^24, which is far beyond any frame number shown on screen).
#[must_use]
pub fn f32_from_u32(v: u32) -> f32 {
    num::f64_to_f32(f64::from(v))
}

/// `usize` to `f32`.
#[must_use]
pub const fn f32_from_usize(v: usize) -> f32 {
    num::f64_to_f32(num::usize_to_f64(v))
}

/// Floors to `i32`, saturating; non-finite input gives 0.
#[must_use]
pub fn floor_i32(v: f32) -> i32 {
    num::floor_f64_to_i32(f64::from(v))
}

/// Floors to `u32`; negative and non-finite input gives 0.
#[must_use]
pub fn floor_u32(v: f32) -> u32 {
    u32::try_from(floor_i32(v)).unwrap_or(0)
}

/// Ceils to `u32`; negative and non-finite input gives 0.
#[must_use]
pub fn ceil_u32(v: f32) -> u32 {
    u32::try_from(num::ceil_f64_to_i32(f64::from(v))).unwrap_or(0)
}

/// Rounds to `usize`, clamping negatives to 0.
#[must_use]
pub fn round_usize(v: f32) -> usize {
    num::u32_to_usize(num::round_f64_to_u32(f64::from(v)))
}

/// Straight RGBA8 to an egui colour.
#[must_use]
pub fn color32(c: anim_core::Rgba8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// An egui colour back to straight RGBA8.
#[must_use]
pub fn rgba8(c: Color32) -> anim_core::Rgba8 {
    let [red, green, blue, alpha] = c.to_srgba_unmultiplied();
    anim_core::Rgba8 { r: red, g: green, b: blue, a: alpha }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_saturate() {
        assert_eq!(floor_u32(-3.5), 0);
        assert_eq!(floor_u32(f32::NAN), 0);
        assert_eq!(floor_u32(7.9), 7);
        assert_eq!(ceil_u32(7.1), 8);
        assert_eq!(round_usize(2.5), 3);
        assert!((f32_from_u32(12) - 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn colour_round_trip() {
        let c = anim_core::Rgba8 { r: 10, g: 20, b: 30, a: 255 };
        assert_eq!(rgba8(color32(c)), c);
    }
}
