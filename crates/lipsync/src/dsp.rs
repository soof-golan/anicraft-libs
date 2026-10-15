//! Small DSP building blocks: FFT wrapper, autocorrelation, Levinson-Durbin LPC, envelope peak picking.
//!
//! Everything is index-free (iterators and `get`) and total: degenerate input yields `None` or zeros, never a panic.

use rustfft::algorithm::Radix4;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftDirection};

/// A forward FFT of power-of-four length with its scratch space.
pub struct Spectrum {
    fft: Radix4<f32>,
    len: usize,
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl Spectrum {
    /// Smallest power of four that is at least `n` (and at least 4).
    #[must_use]
    pub const fn size_for(n: usize) -> usize {
        let mut len = 4_usize;
        while len < n {
            len = len.saturating_mul(4);
        }
        len
    }

    /// Plans a forward FFT of `Self::size_for(min_len)` points.
    #[must_use]
    pub fn new(min_len: usize) -> Self {
        let len = Self::size_for(min_len);
        let fft = Radix4::new(len, FftDirection::Forward);
        let scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        Self { fft, len, buf: vec![Complex::new(0.0, 0.0); len], scratch }
    }

    /// The transform length.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.len
    }

    /// Transforms real `input` (zero padded or truncated to the FFT length) and returns the complex bins.
    pub fn forward(&mut self, input: &[f32]) -> &[Complex<f32>] {
        let mut src = input.iter();
        for slot in &mut self.buf {
            *slot = Complex::new(src.next().copied().unwrap_or(0.0), 0.0);
        }
        self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);
        &self.buf
    }
}

/// Hann window of `n` points.
#[must_use]
pub fn hann(n: usize) -> Vec<f32> {
    let denom = anim_core::num::usize_to_f64(n.saturating_sub(1).max(1));
    (0..n)
        .map(|i| {
            let phase = std::f64::consts::TAU * anim_core::num::usize_to_f64(i) / denom;
            anim_core::num::f64_to_f32(0.5_f64.mul_add(-phase.cos(), 0.5))
        })
        .collect()
}

/// Autocorrelation `r[0..=max_lag]` of `x` (missing lags beyond the input are zero).
#[must_use]
pub fn autocorrelation(x: &[f32], max_lag: usize) -> Vec<f32> {
    (0..=max_lag).map(|lag| x.iter().zip(x.iter().skip(lag)).map(|(a, b)| a * b).sum()).collect()
}

/// Levinson-Durbin recursion: prediction polynomial `A(z) = 1 + a1 z^-1 + ...` of the given order from the
/// autocorrelation `r` (which must hold at least `order + 1` lags). Returns the coefficients with `a[0] == 1`, or
/// `None` when the recursion is unstable or `r` is degenerate.
#[must_use]
pub fn levinson(r: &[f32], order: usize) -> Option<Vec<f32>> {
    let mut err = *r.first()?;
    if !(err.is_finite() && err > 0.0) {
        return None;
    }
    let mut a = vec![1.0_f32];
    for i in 1..=order {
        let lags = r.get(1..=i)?;
        let acc: f32 = a.iter().zip(lags.iter().rev()).map(|(c, rv)| c * rv).sum();
        let k = -acc / err;
        if !k.is_finite() || k.abs() >= 1.0 {
            return None;
        }
        a.push(0.0);
        let mirrored: Vec<f32> = a.iter().rev().copied().collect();
        a = a.iter().zip(&mirrored).map(|(x, y)| k.mul_add(*y, *x)).collect();
        err *= k.mul_add(-k, 1.0);
        if err <= 0.0 {
            return None;
        }
    }
    Some(a)
}

/// Frequencies (Hz) of the local maxima of `1 / |A|^2`, in ascending order, refined by parabolic interpolation.
///
/// `bins` are the FFT bins of the zero padded polynomial `A` (the full spectrum, `bins.len()` points); only the
/// lower half is inspected. Peaks below `min_hz` or at/above `max_hz` are dropped.
#[must_use]
pub fn envelope_peaks(bins: &[Complex<f32>], sample_rate: f32, min_hz: f32, max_hz: f32) -> Vec<f32> {
    let half = bins.len() / 2;
    let env: Vec<f32> = bins.iter().take(half).map(|c| 1.0 / c.norm_sqr().max(1e-12)).collect();
    let bin_hz = sample_rate / crate::num::usize_f32(bins.len());
    let mut peaks = Vec::new();
    for (i, w) in env.windows(3).enumerate() {
        let (Some(&l), Some(&m), Some(&r)) = (w.first(), w.get(1), w.get(2)) else { continue };
        if m > l && m >= r {
            // Parabola through the three points; the vertex offset is in -0.5..=0.5 bins.
            let denom = 2.0_f32.mul_add(-m, l) + r;
            let offset = if denom.abs() > f32::EPSILON { (0.5 * (l - r) / denom).clamp(-0.5, 0.5) } else { 0.0 };
            let hz = (crate::num::usize_f32(i.saturating_add(1)) + offset) * bin_hz;
            if hz >= min_hz && hz < max_hz {
                peaks.push(hz);
            }
        }
    }
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_for_rounds_up_to_power_of_four() {
        assert_eq!(Spectrum::size_for(0), 4);
        assert_eq!(Spectrum::size_for(5), 16);
        assert_eq!(Spectrum::size_for(400), 1024);
        assert_eq!(Spectrum::size_for(256), 256);
    }

    #[test]
    fn fft_finds_a_sine() {
        let mut spec = Spectrum::new(256);
        let x: Vec<f32> = (0..256).map(|i| (std::f32::consts::TAU * 16.0 * f32::from(u16::try_from(i).unwrap()) / 256.0).sin()).collect();
        let bins = spec.forward(&x);
        let best = bins.iter().take(128).enumerate().max_by(|a, b| a.1.norm_sqr().total_cmp(&b.1.norm_sqr())).unwrap().0;
        assert_eq!(best, 16);
    }

    #[test]
    fn levinson_recovers_an_ar1_process() {
        // r[k] = 0.9^k  =>  a1 = -0.9.
        let r: Vec<f32> = (0..4).map(|k| 0.9_f32.powi(k)).collect();
        let a = levinson(&r, 1).unwrap();
        assert!((a[1] + 0.9).abs() < 1e-5);
    }

    #[test]
    fn levinson_rejects_degenerate_input() {
        assert!(levinson(&[], 4).is_none());
        assert!(levinson(&[0.0; 8], 4).is_none());
        assert!(levinson(&[1.0, 0.5], 4).is_none());
        assert!(levinson(&[f32::NAN; 8], 2).is_none());
    }

    #[test]
    fn hann_is_zero_at_the_ends() {
        let w = hann(64);
        assert!(w[0].abs() < 1e-6 && w[63].abs() < 1e-6);
        assert_eq!(hann(1).len(), 1);
        assert_eq!(hann(0).len(), 0);
    }
}
