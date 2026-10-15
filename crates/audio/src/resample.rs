//! Band-limited sample-rate conversion with a Blackman-windowed sinc.

use anim_core::num::{ceil_f64_to_i32, floor_f64_to_i32, usize_to_f64};

use crate::buffer::{MAX_SAMPLE_RATE, PcmBuffer};
use crate::error::{AudioError, Result};

/// Zero crossings of the sinc on each side of the centre.
const HALF_WIDTH: usize = 16;
/// Kernel table entries per zero crossing (linear interpolation between them).
const TABLE_RES: usize = 512;
/// Fraction of the Nyquist rate kept by the filter, leaving a transition band.
const PASSBAND: f64 = 0.97;
/// Most samples a conversion may produce.
const MAX_OUTPUT_SAMPLES: usize = 1 << 30;

/// Converts `buf` to `target_rate`. The same rate returns a copy.
///
/// Downsampling low-passes below the new Nyquist frequency first, so nothing aliases. The first output sample
/// lines up with the first input sample, and the duration is preserved to within half an output frame.
///
/// # Errors
/// [`AudioError::InvalidFormat`] for a zero or out-of-range rate or channel count and
/// [`AudioError::OutputTooLarge`] when the result would exceed about a billion samples.
pub fn resample(buf: &PcmBuffer, target_rate: u32) -> Result<PcmBuffer> {
    if !buf.is_valid_format() || target_rate == 0 || target_rate > MAX_SAMPLE_RATE {
        return Err(AudioError::InvalidFormat(format!("{} Hz -> {target_rate} Hz, {} channels", buf.sample_rate, buf.channels)));
    }
    if target_rate == buf.sample_rate {
        return Ok(buf.clone());
    }
    let channels = usize::from(buf.channels);
    let in_frames = buf.frames();
    let out_frames = {
        let n = u128::try_from(in_frames).unwrap_or(0).saturating_mul(u128::from(target_rate));
        let d = u128::from(buf.sample_rate);
        let rounded = n.saturating_add(d / 2).checked_div(d).unwrap_or(0);
        usize::try_from(rounded).unwrap_or(usize::MAX)
    };
    if out_frames.saturating_mul(channels) > MAX_OUTPUT_SAMPLES {
        return Err(AudioError::OutputTooLarge { limit: MAX_OUTPUT_SAMPLES });
    }

    let step = f64::from(buf.sample_rate) / f64::from(target_rate);
    let cutoff = (f64::from(target_rate) / f64::from(buf.sample_rate)).min(1.0) * PASSBAND;
    let support = usize_to_f64(HALF_WIDTH) / cutoff;
    let table = kernel_table();

    let mut out = vec![0.0_f32; out_frames.saturating_mul(channels)];
    let mut weights: Vec<f32> = Vec::new();
    let mut acc = vec![0.0_f32; channels];
    for (i, dst) in out.chunks_exact_mut(channels).enumerate() {
        let centre = usize_to_f64(i) * step;
        let lo = ceil_f64_to_i32(centre - support).max(0);
        let hi = floor_f64_to_i32(centre + support).min(i32::try_from(in_frames.saturating_sub(1)).unwrap_or(i32::MAX));
        weights.clear();
        let mut sum = 0.0_f32;
        for j in lo..=hi {
            let u = (f64::from(j) - centre).abs() * cutoff;
            let w = kernel(&table, u);
            sum += w;
            weights.push(w);
        }
        if sum.abs() < 1e-9 {
            continue;
        }
        let norm = 1.0 / sum;
        acc.fill(0.0);
        for (k, &w) in weights.iter().enumerate() {
            let frame = usize::try_from(lo).unwrap_or(0).saturating_add(k);
            let base = frame.saturating_mul(channels);
            if let Some(src) = buf.samples.get(base..base.saturating_add(channels)) {
                for (a, s) in acc.iter_mut().zip(src) {
                    *a += s * w;
                }
            }
        }
        for (d, a) in dst.iter_mut().zip(&acc) {
            *d = a * norm;
        }
    }
    Ok(PcmBuffer { sample_rate: target_rate, channels: buf.channels, samples: out })
}

/// `sinc(u) * window(u)` for `u` in `0..=HALF_WIDTH`, sampled every `1 / TABLE_RES`.
fn kernel_table() -> Vec<f32> {
    let n = HALF_WIDTH.saturating_mul(TABLE_RES).saturating_add(2);
    (0..n)
        .map(|k| {
            let u = usize_to_f64(k) / usize_to_f64(TABLE_RES);
            let half = usize_to_f64(HALF_WIDTH);
            if u >= half {
                return 0.0;
            }
            let sinc = if u < 1e-12 { 1.0 } else { (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u) };
            let t = std::f64::consts::PI * u / half;
            // Blackman window.
            let window = 0.08_f64.mul_add((2.0 * t).cos(), 0.5_f64.mul_add(t.cos(), 0.42));
            anim_core::num::f64_to_f32(sinc * window)
        })
        .collect()
}

fn kernel(table: &[f32], u: f64) -> f32 {
    let pos = u * usize_to_f64(TABLE_RES);
    let idx = usize::try_from(floor_f64_to_i32(pos).max(0)).unwrap_or(0);
    let frac = anim_core::num::f64_to_f32(pos - pos.floor());
    let a = table.get(idx).copied().unwrap_or(0.0);
    let b = table.get(idx.saturating_add(1)).copied().unwrap_or(0.0);
    (b - a).mul_add(frac, a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::testutil::sine;

    fn rms(b: &PcmBuffer, skip: usize) -> f32 {
        let s: Vec<f32> = b.samples.iter().skip(skip * usize::from(b.channels)).copied().collect();
        (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
    }

    #[test]
    fn same_rate_is_a_copy() {
        let b = sine(8000, 1, 100, 100.0, 0.5);
        assert_eq!(resample(&b, 8000).unwrap(), b);
    }

    #[test]
    fn length_scales_with_rate() {
        let b = sine(44_100, 2, 44_100, 440.0, 0.5);
        let r = resample(&b, 48_000).unwrap();
        assert_eq!(r.frames(), 48_000);
        assert_eq!((r.sample_rate, r.channels), (48_000, 2));
        let d = resample(&b, 22_050).unwrap();
        assert_eq!(d.frames(), 22_050);
    }

    #[test]
    fn sine_survives_up_and_down() {
        let b = sine(44_100, 1, 8000, 1000.0, 0.5);
        let up = resample(&b, 48_000).unwrap();
        // RMS of a 0.5 amplitude sine is 0.3535, away from the edges.
        assert!((rms(&up, 200) - 0.3535).abs() < 0.01, "{}", rms(&up, 200));
        let want = sine(48_000, 1, up.frames(), 1000.0, 0.5);
        let err = up.samples.iter().zip(&want.samples).skip(400).take(5000).map(|(a, b)| (a - b).abs()).fold(0.0_f32, f32::max);
        assert!(err < 0.01, "max error {err}");
        let down = resample(&up, 44_100).unwrap();
        assert!(down.frames().abs_diff(b.frames()) <= 1);
    }

    #[test]
    fn dc_is_preserved() {
        let b = PcmBuffer::new(8000, 1, vec![0.5; 2000]);
        let r = resample(&b, 11_025).unwrap();
        let mid = r.samples.get(100..r.samples.len() - 100).unwrap();
        assert!(mid.iter().all(|v| (v - 0.5).abs() < 1e-3));
    }

    #[test]
    fn downsampling_removes_content_above_new_nyquist() {
        // 10 kHz at 44.1 kHz is above the 8 kHz Nyquist of the 16 kHz target.
        let b = sine(44_100, 1, 8000, 10_000.0, 0.8);
        let r = resample(&b, 16_000).unwrap();
        assert!(rms(&r, 100) < 0.02, "{}", rms(&r, 100));
    }

    #[test]
    fn rejects_bad_formats_and_handles_tiny_input() {
        assert!(resample(&PcmBuffer::new(0, 1, vec![]), 8000).is_err());
        assert!(resample(&PcmBuffer::new(8000, 0, vec![]), 8000).is_err());
        assert!(resample(&PcmBuffer::new(8000, 1, vec![]), 0).is_err());
        assert_eq!(resample(&PcmBuffer::new(8000, 1, vec![]), 16_000).unwrap().frames(), 0);
        let one = resample(&PcmBuffer::new(8000, 1, vec![1.0]), 48_000).unwrap();
        assert_eq!(one.frames(), 6);
        assert!(one.samples.iter().all(|v| v.is_finite()));
    }
}
