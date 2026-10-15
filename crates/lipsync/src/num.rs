//! Tiny numeric helpers on top of `anim_core::num` (the only place `as` lives).

/// `usize` to `f32` through the checked `f64` helper.
#[must_use]
pub const fn usize_f32(v: usize) -> f32 {
    anim_core::num::f64_to_f32(anim_core::num::usize_to_f64(v))
}

/// Rounds a positive duration in samples (`seconds * rate`) to a `usize`, at least `min`.
#[must_use]
pub fn samples_for(seconds: f32, rate: f32, min: usize) -> usize {
    let n = anim_core::num::round_f64_to_u32(f64::from(seconds) * f64::from(rate));
    anim_core::num::u32_to_usize(n).max(min)
}

/// `usize` to `i64`, saturating.
#[must_use]
pub fn usize_i64(v: usize) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// `u64` to `i64`, saturating.
#[must_use]
pub fn u64_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// `u64` to `f32` (frame counts are tiny compared to 2^24 in the places this is used, otherwise it rounds).
#[must_use]
pub fn u32_f32(v: u32) -> f32 {
    anim_core::num::f64_to_f32(f64::from(v))
}
