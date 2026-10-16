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

/// Linear blend: `a` at `t = 0`, `b` at `t = 1` (not clamped).
#[must_use]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (b - a).mul_add(t, a)
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
    // `f32::round` is a library call on baseline x86; truncating and testing the (exact) remainder rounds half
    // up the same way. NaN truncates to 0 and fails the remainder test.
    let scaled = v.clamp(0.0, 1.0) * 255.0;
    let whole = scaled as u8;
    whole.saturating_add(u8::from(scaled - f32::from(whole) >= 0.5))
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
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "compares against the library rounding"
    )]
    fn unit_to_byte_matches_library_rounding_on_every_nearby_float() {
        // Every float within a few ulps of each half-way point and each whole byte, plus a sweep of the range.
        let mut values: Vec<f32> = (0..=255_u16).map(|b| f32::from(b) / 255.0).collect();
        for b in 0..255_u16 {
            values.push((f32::from(b) + 0.5) / 255.0);
        }
        for step in 0..=2_000_000_u32 {
            values.push(f32::from_bits(0x3f80_0000_u32.saturating_sub(step.saturating_mul(2))));
            values.push(step as f32 / 2_000_000.0);
        }
        let around: Vec<f32> = values
            .iter()
            .flat_map(|v| (0..8_u32).flat_map(move |k| [f32::from_bits(v.to_bits().wrapping_add(k)), f32::from_bits(v.to_bits().wrapping_sub(k))]))
            .collect();
        for v in values.into_iter().chain(around) {
            let expected = if v.is_nan() { 0 } else { (v.clamp(0.0, 1.0) * 255.0).round() as u8 };
            assert_eq!(unit_f32_to_u8(v), expected, "{v:e}");
        }
    }

    #[test]
    fn lerp_hits_endpoints_and_midpoint() {
        assert!((lerp(2.0, 6.0, 0.0) - 2.0).abs() < f64::EPSILON);
        assert!((lerp(2.0, 6.0, 1.0) - 6.0).abs() < f64::EPSILON);
        assert!((lerp(2.0, 6.0, 0.5) - 4.0).abs() < f64::EPSILON);
        assert!((lerp(2.0, 6.0, 1.5) - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn integer_conversions_saturate() {
        assert_eq!(usize_to_u32_sat(usize::MAX), u32::MAX);
        assert_eq!(u32_to_usize(7), 7);
    }
}
