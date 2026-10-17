//! Synthetic audio for tests: formant vowels, noise, silence and hums.
//!
//! The lip sync tests of `anim-lipsync` and the speech tests of `charcraft-record` both need speech-like signals with
//! a known mouth shape, so the generators live here once. Everything is deterministic: noise comes from a seeded
//! xorshift, pitch from a pulse train, so the same call gives the same samples on every machine.

#![allow(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::many_single_char_names,
    clippy::suboptimal_flops,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    reason = "signal synthesis for tests: casts, indexing and plain arithmetic, in a crate that is never shipped"
)]

/// Deterministic noise in 0 .. 1 for the integer `i`: a hash, not a sequence, so any index can be asked for.
#[must_use]
pub fn noise(i: u32) -> f64 {
    let x = i.wrapping_mul(2_654_435_761).rotate_left(13).wrapping_mul(40_503);
    f64::from(x >> 8) / f64::from(1_u32 << 24)
}

/// A small xorshift generator of uniform values in -1 ..= 1.
#[derive(Debug, Clone, Copy)]
pub struct Rng(pub u32);

impl Rng {
    /// The next value in -1 ..= 1.
    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Samples in `seconds` at `rate`.
#[must_use]
pub fn n(seconds: f32, rate: u32) -> usize {
    (seconds * rate as f32).round() as usize
}

/// `seconds` of zeros.
#[must_use]
pub fn silence(seconds: f32, rate: u32) -> Vec<f32> {
    vec![0.0; n(seconds, rate)]
}

/// Root mean square.
#[must_use]
pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// `x` scaled to the root mean square `target`.
#[must_use]
pub fn scale_to_rms(mut x: Vec<f32>, target: f32) -> Vec<f32> {
    let cur = rms(&x).max(1e-9);
    for v in &mut x {
        *v *= target / cur;
    }
    x
}

/// Two-pole resonator in place.
pub fn resonate(x: &mut [f32], rate: u32, hz: f32, bw: f32) {
    let r = (-std::f32::consts::PI * bw / rate as f32).exp();
    let c1 = 2.0 * r * (std::f32::consts::TAU * hz / rate as f32).cos();
    let c2 = -r * r;
    let (mut y1, mut y2) = (0.0, 0.0);
    for v in x.iter_mut() {
        let y = *v + c1 * y1 + c2 * y2;
        y2 = y1;
        y1 = y;
        *v = y;
    }
}

/// Glottal pulse train with a pitch that moves linearly from `f0_start` to `f0_end`.
#[must_use]
pub fn pulses(f0_start: f32, f0_end: f32, seconds: f32, rate: u32) -> Vec<f32> {
    let len = n(seconds, rate);
    let mut src = vec![0.0; len];
    let mut phase = 0.0_f32;
    for (i, v) in src.iter_mut().enumerate() {
        let f0 = f0_start + (f0_end - f0_start) * (i as f32 / len.max(1) as f32);
        phase += f0 / rate as f32;
        if phase >= 1.0 {
            phase -= 1.0;
            *v = 1.0;
        }
    }
    // Glottal spectral tilt.
    let mut y = 0.0;
    for v in &mut src {
        y = 0.5 * y + *v;
        *v = y;
    }
    src
}

/// A vowel through a formant cascade (no fade; use [`fade`] or [`crossfade_concat`]).
#[must_use]
pub fn vowel_raw(formants: &[f32], f0_start: f32, f0_end: f32, seconds: f32, rate: u32, level: f32) -> Vec<f32> {
    let mut src = pulses(f0_start, f0_end, seconds, rate);
    for &f in formants {
        resonate(&mut src, rate, f, 70.0 + f * 0.04);
    }
    scale_to_rms(src, level)
}

/// 10 ms linear fades at both ends.
#[must_use]
pub fn fade(mut x: Vec<f32>, rate: u32) -> Vec<f32> {
    let k = n(0.01, rate).min(x.len() / 2);
    for i in 0..k {
        let g = i as f32 / k as f32;
        x[i] *= g;
        let last = x.len() - 1 - i;
        x[last] *= g;
    }
    x
}

/// A steady vowel at `f0` Hz with the given formants, root mean square `level`, faded at both ends.
#[must_use]
pub fn vowel(formants: &[f32], f0: f32, seconds: f32, rate: u32, level: f32) -> Vec<f32> {
    fade(vowel_raw(formants, f0, f0, seconds, rate, level), rate)
}

/// Joins sounds with a linear cross-fade of `overlap` seconds (no gap).
#[must_use]
pub fn crossfade_concat(parts: &[Vec<f32>], overlap: f32, rate: u32) -> Vec<f32> {
    let k = n(overlap, rate);
    let mut out: Vec<f32> = Vec::new();
    for part in parts {
        if out.len() < k || part.len() < k {
            out.extend_from_slice(part);
            continue;
        }
        let base = out.len() - k;
        for i in 0..k {
            let w = i as f32 / k as f32;
            out[base + i] = out[base + i] * (1.0 - w) + part[i] * w;
        }
        out.extend_from_slice(&part[k..]);
    }
    out
}

/// Noise concentrated around `centre` Hz (S: about 6.5 kHz).
#[must_use]
pub fn band_noise(centre: f32, bw: f32, seconds: f32, rate: u32, level: f32, seed: u32) -> Vec<f32> {
    let mut rng = Rng(seed);
    let mut x: Vec<f32> = (0..n(seconds, rate)).map(|_| rng.next_f32()).collect();
    resonate(&mut x, rate, centre, bw);
    resonate(&mut x, rate, centre, bw);
    fade(scale_to_rms(x, level), rate)
}

/// White noise without fades.
#[must_use]
pub fn white_noise_raw(seconds: f32, rate: u32, level: f32, seed: u32) -> Vec<f32> {
    let mut rng = Rng(seed);
    scale_to_rms((0..n(seconds, rate)).map(|_| rng.next_f32()).collect(), level)
}

/// White noise, faded at both ends.
#[must_use]
pub fn white_noise(seconds: f32, rate: u32, level: f32, seed: u32) -> Vec<f32> {
    fade(white_noise_raw(seconds, rate, level, seed), rate)
}

/// A pure hum at `hz` (a closed-mouth murmur).
#[must_use]
pub fn hum(hz: f32, seconds: f32, rate: u32, level: f32) -> Vec<f32> {
    let x: Vec<f32> = (0..n(seconds, rate)).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin()).collect();
    fade(scale_to_rms(x, level), rate)
}

/// Sample-wise sum; the shorter signal counts as silence past its end.
#[must_use]
pub fn add(a: &[f32], b: &[f32]) -> Vec<f32> {
    let len = a.len().max(b.len());
    (0..len).map(|i| a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0)).collect()
}

/// The parts one after another.
#[must_use]
pub fn cat(parts: &[Vec<f32>]) -> Vec<f32> {
    parts.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_are_deterministic_and_at_the_asked_level() {
        let a = vowel(&[800.0, 1300.0, 2600.0], 120.0, 0.2, 16_000, 0.1);
        assert_eq!(a, vowel(&[800.0, 1300.0, 2600.0], 120.0, 0.2, 16_000, 0.1));
        assert_eq!(a.len(), 3200);
        assert!((rms(&white_noise_raw(0.5, 16_000, 0.05, 3)) - 0.05).abs() < 1e-4);
        assert_eq!(white_noise(0.1, 8_000, 0.1, 1), white_noise(0.1, 8_000, 0.1, 1));
        assert_ne!(white_noise(0.1, 8_000, 0.1, 1), white_noise(0.1, 8_000, 0.1, 2));
    }

    #[test]
    fn joining_and_adding_follow_the_shorter_part_rules() {
        assert_eq!(cat(&[vec![1.0], vec![2.0, 3.0]]), [1.0, 2.0, 3.0]);
        assert_eq!(add(&[1.0, 1.0], &[2.0]), [3.0, 1.0]);
        let joined = crossfade_concat(&[vec![1.0; 100], vec![0.0; 100]], 0.005, 8_000);
        assert_eq!(joined.len(), 200 - n(0.005, 8_000));
        assert!(silence(0.5, 100).iter().all(|v| v.abs() < f32::EPSILON));
    }

    #[test]
    fn noise_stays_in_the_unit_interval() {
        assert!((0..10_000).all(|i| (0.0..1.0).contains(&noise(i))));
        assert!((noise(1) - noise(2)).abs() > 1e-9);
    }
}
