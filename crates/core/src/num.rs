//! Checked and saturating numeric conversions.
//!
//! The workspace denies `as` casts. Float-to-integer conversions have no `TryFrom`, so they live here, once, with
//! their saturation behaviour documented and tested. Everything else should use `From`/`TryFrom`.

/// Rounds `v` to the nearest integer and saturates into `i32`. `NaN` becomes 0.
#[must_use]
#[expect(clippy::as_conversions, clippy::cast_possible_truncation, reason = "float-to-int `as` saturates and maps NaN to 0, which is the documented behaviour")]
pub const fn round_f64_to_i32(v: f64) -> i32 {
    v.round() as i32
}

/// Rounds `v` to the nearest integer and saturates into `i64`. `NaN` becomes 0.
#[must_use]
#[expect(clippy::as_conversions, clippy::cast_possible_truncation, reason = "float-to-int `as` saturates and maps NaN to 0, which is the documented behaviour")]
pub const fn round_f64_to_i64(v: f64) -> i64 {
    v.round() as i64
}

/// Rounds `v` to the nearest integer and saturates into `u32` (negative values become 0). `NaN` becomes 0.
#[must_use]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "float-to-int `as` saturates and maps NaN to 0, which is the documented behaviour"
)]
pub const fn round_f64_to_u32(v: f64) -> u32 {
    v.round() as u32
}

/// Floors `v` and saturates into `i32`. `NaN` becomes 0.
#[must_use]
#[expect(clippy::as_conversions, clippy::cast_possible_truncation, reason = "float-to-int `as` saturates and maps NaN to 0, which is the documented behaviour")]
pub const fn floor_f64_to_i32(v: f64) -> i32 {
    v.floor() as i32
}

/// Ceils `v` and saturates into `i32`. `NaN` becomes 0.
#[must_use]
#[expect(clippy::as_conversions, clippy::cast_possible_truncation, reason = "float-to-int `as` saturates and maps NaN to 0, which is the documented behaviour")]
pub const fn ceil_f64_to_i32(v: f64) -> i32 {
    v.ceil() as i32
}

/// Maps a unit value (0.0 ..= 1.0, clamped) to 0 ..= 255 with rounding. `NaN` becomes 0.
#[must_use]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=255 first, so the cast is exact"
)]
pub fn unit_f32_to_u8(v: f32) -> u8 {
    if v.is_nan() {
        return 0;
    }
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `f64` to `f32`, rounding to nearest (values beyond `f32::MAX` become infinity, as IEEE rounding does).
#[must_use]
#[expect(clippy::as_conversions, clippy::cast_possible_truncation, reason = "f64-to-f32 `as` rounds to nearest, which is the documented behaviour")]
pub const fn f64_to_f32(v: f64) -> f32 {
    v as f32
}

/// Exact `usize` to `f64` for values up to 2^53; larger values round to the nearest representable `f64`.
#[must_use]
#[expect(clippy::as_conversions, clippy::cast_precision_loss, reason = "counts and sizes in this code base are far below 2^53")]
pub const fn usize_to_f64(v: usize) -> f64 {
    v as f64
}

/// Exact `i64` to `f64` for magnitudes up to 2^53; larger values round to the nearest representable `f64`.
#[must_use]
#[expect(clippy::as_conversions, clippy::cast_precision_loss, reason = "documented rounding for magnitudes above 2^53")]
pub const fn i64_to_f64(v: i64) -> f64 {
    v as f64
}

/// `u32` to `usize`, saturating on (hypothetical) 16-bit targets.
#[must_use]
pub fn u32_to_usize(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// `usize` to `u32`, saturating at `u32::MAX`.
#[must_use]
pub fn usize_to_u32_sat(v: usize) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_to_int_saturates_and_maps_nan_to_zero() {
        assert_eq!(round_f64_to_i32(2.5), 3);
        assert_eq!(round_f64_to_i32(-2.5), -3);
        assert_eq!(round_f64_to_i32(f64::NAN), 0);
        assert_eq!(round_f64_to_i32(1e20), i32::MAX);
        assert_eq!(round_f64_to_u32(-4.0), 0);
        assert_eq!(floor_f64_to_i32(-0.5), -1);
        assert_eq!(ceil_f64_to_i32(-0.5), 0);
    }

    #[test]
    fn unit_to_byte_clamps() {
        assert_eq!(unit_f32_to_u8(-1.0), 0);
        assert_eq!(unit_f32_to_u8(0.5), 128);
        assert_eq!(unit_f32_to_u8(2.0), 255);
        assert_eq!(unit_f32_to_u8(f32::NAN), 0);
    }

    #[test]
    fn integer_conversions_saturate() {
        assert_eq!(usize_to_u32_sat(usize::MAX), u32::MAX);
        assert_eq!(u32_to_usize(7), 7);
    }
}
