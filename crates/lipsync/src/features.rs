//! Per-frame acoustic features: energy, zero-crossing rate, voicing, band shares and LPC formants.

use crate::dsp::{Spectrum, autocorrelation, envelope_peaks, hann, levinson};
use crate::num::{samples_for, usize_f32};

/// Analysis window length in seconds.
pub const WINDOW_SECONDS: f32 = 0.025;
/// Hop between analysis frames in seconds.
pub const HOP_SECONDS: f32 = 0.010;
/// Energy floor reported for digital silence, dBFS.
pub const MIN_DB: f32 = -120.0;
/// Below this frequency the "low band" share is measured (nasal murmur lives here).
pub const LOW_BAND_HZ: f32 = 700.0;

/// What the classifier knows about one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Features {
    /// RMS level of the frame in dBFS (DC removed).
    pub energy_db: f32,
    /// Zero crossings per sample, 0..=1.
    pub zcr: f32,
    /// Normalised autocorrelation peak in the pitch range, 0..=1 (high for voiced sound).
    pub voicing: f32,
    /// Share of spectral energy above the high-band edge (about 4 kHz), 0..=1.
    pub high_share: f32,
    /// Share of spectral energy below [`LOW_BAND_HZ`], 0..=1.
    pub low_share: f32,
    /// First three LPC envelope peaks in Hz (F1, F2, F3), when found.
    pub formants: [Option<f32>; 3],
}

/// Computes [`Features`] for fixed-size frames at one sample rate; owns its scratch buffers.
pub struct FrameAnalyzer {
    rate: f32,
    window: Vec<f32>,
    spectrum: Spectrum,
    order: usize,
    high_edge_hz: f32,
    work: Vec<f32>,
    windowed: Vec<f32>,
}

impl FrameAnalyzer {
    /// Analyzer for frames of [`Self::frame_len`] samples at `rate` Hz.
    #[must_use]
    pub fn new(rate: f32) -> Self {
        let len = Self::frame_len_for(rate);
        let nyquist = rate * 0.5;
        // About one pole per kHz minus two, clamped: 14 poles at 16 kHz (three to four formants plus tilt).
        let order = samples_for(rate / 1000.0 - 2.0, 1.0, 8).min(16);
        Self {
            rate,
            window: hann(len),
            spectrum: Spectrum::new(len),
            order,
            high_edge_hz: (nyquist * 0.7).min(4000.0),
            work: Vec::with_capacity(len),
            windowed: Vec::with_capacity(len),
        }
    }

    /// Frame length in samples for `rate`.
    #[must_use]
    pub fn frame_len_for(rate: f32) -> usize {
        samples_for(WINDOW_SECONDS, rate, 32)
    }

    /// Hop in samples for `rate`.
    #[must_use]
    pub fn hop_for(rate: f32) -> usize {
        samples_for(HOP_SECONDS, rate, 8)
    }

    /// RMS level in dBFS of `frame` after removing its mean.
    #[must_use]
    pub fn energy_db(frame: &[f32]) -> f32 {
        if frame.is_empty() {
            return MIN_DB;
        }
        let n = usize_f32(frame.len());
        let mean = frame.iter().sum::<f32>() / n;
        let power = frame.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n;
        (10.0 * power.max(1e-14).log10()).max(MIN_DB)
    }

    /// Full feature set for a frame whose [`Self::energy_db`] is `energy_db`.
    pub fn features(&mut self, frame: &[f32], energy_db: f32) -> Features {
        let n = usize_f32(frame.len().max(1));
        let mean = frame.iter().sum::<f32>() / n;
        self.work.clear();
        self.work.extend(frame.iter().map(|x| x - mean));
        let zcr = zero_crossing_rate(&self.work);
        let voicing = voicing(&self.work, self.rate);
        self.windowed.clear();
        self.windowed.extend(self.work.iter().zip(&self.window).map(|(x, w)| x * w));
        let (high_share, low_share) = self.band_shares();
        let formants = self.formants();
        Features { energy_db, zcr, voicing, high_share, low_share, formants }
    }

    fn band_shares(&mut self) -> (f32, f32) {
        let bin_hz = self.rate / usize_f32(self.spectrum.size());
        let half = self.spectrum.size() / 2;
        let bins = self.spectrum.forward(&self.windowed);
        let (mut total, mut high, mut low) = (0.0_f32, 0.0_f32, 0.0_f32);
        for (k, c) in bins.iter().enumerate().take(half.saturating_add(1)).skip(1) {
            let hz = usize_f32(k) * bin_hz;
            if hz < 60.0 {
                continue;
            }
            let p = c.norm_sqr();
            total += p;
            if hz >= self.high_edge_hz {
                high += p;
            }
            if hz < LOW_BAND_HZ {
                low += p;
            }
        }
        if total <= f32::MIN_POSITIVE {
            return (0.0, 0.0);
        }
        (high / total, low / total)
    }

    fn formants(&mut self) -> [Option<f32>; 3] {
        let none = [None; 3];
        let pre: Vec<f32> = self.windowed.iter().zip(self.windowed.iter().skip(1)).map(|(a, b)| 0.97_f32.mul_add(-a, *b)).collect();
        let mut r = autocorrelation(&pre, self.order);
        if let Some(r0) = r.first_mut() {
            *r0 = r0.mul_add(1.0001, 1e-9);
        }
        let Some(a) = levinson(&r, self.order) else { return none };
        let bins = self.spectrum.forward(&a);
        let peaks = envelope_peaks(bins, self.rate, 180.0, self.rate * 0.5 * 0.96);
        let mut it = peaks.into_iter();
        [it.next(), it.next(), it.next()]
    }
}

fn zero_crossing_rate(x: &[f32]) -> f32 {
    if x.len() < 2 {
        return 0.0;
    }
    let crossings = x.iter().zip(x.iter().skip(1)).filter(|(a, b)| (**a < 0.0) != (**b < 0.0)).count();
    usize_f32(crossings) / usize_f32(x.len().saturating_sub(1))
}

/// Highest normalised autocorrelation for lags corresponding to 70..400 Hz.
#[allow(clippy::suboptimal_flops, reason = "mul_add is a libm call on baseline x86-64")]
fn voicing(x: &[f32], rate: f32) -> f32 {
    let n = x.len();
    let lo = samples_for(1.0 / 400.0, rate, 2);
    let hi = samples_for(1.0 / 70.0, rate, 3).min(n / 2);
    // Running sums of squares: the energy of any prefix or suffix is one subtraction instead of a pass over the frame.
    let mut squares = Vec::with_capacity(n.saturating_add(1));
    squares.push(0.0_f64);
    let mut total = 0.0_f64;
    for v in x {
        total += f64::from(*v) * f64::from(*v);
        squares.push(total);
    }
    let mut best = 0.0_f32;
    for lag in lo..=hi {
        let (Some(a), Some(b)) = (x.get(..n.saturating_sub(lag)), x.get(lag..)) else { continue };
        let (Some(&head), Some(&all), Some(&skip)) = (squares.get(a.len()), squares.get(n), squares.get(lag)) else { continue };
        let denom = (head * (all - skip)).sqrt();
        if denom > 1e-12 {
            best = best.max(anim_core::num::f64_to_f32(f64::from(dot(a, b)) / denom));
        }
    }
    best.clamp(0.0, 1.0)
}

/// Dot product with eight independent accumulators so the loop vectorises. A separate multiply and add: without a
/// target FMA feature `mul_add` is a call into libm, many times slower.
#[allow(clippy::suboptimal_flops, reason = "mul_add is a libm call on baseline x86-64")]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut lanes = [0.0_f32; 8];
    let (a8, a_rest) = a.as_chunks::<8>();
    let (b8, b_rest) = b.as_chunks::<8>();
    for (p, q) in a8.iter().zip(b8) {
        for ((lane, x), y) in lanes.iter_mut().zip(p).zip(q) {
            *lane += x * y;
        }
    }
    let tail: f32 = a_rest.iter().zip(b_rest).map(|(x, y)| x * y).sum();
    lanes.iter().sum::<f32>() + tail
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, rate: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (std::f32::consts::TAU * hz * usize_f32(i) / rate).sin() * 0.3).collect()
    }

    #[test]
    fn silence_is_at_the_floor() {
        assert!((FrameAnalyzer::energy_db(&[0.0; 400]) - MIN_DB).abs() < 1e-3);
        assert!((FrameAnalyzer::energy_db(&[]) - MIN_DB).abs() < 1e-3);
    }

    #[test]
    fn sine_features() {
        let rate = 16_000.0;
        let frame = sine(200.0, rate, FrameAnalyzer::frame_len_for(rate));
        let mut fa = FrameAnalyzer::new(rate);
        let e = FrameAnalyzer::energy_db(&frame);
        assert!((-15.0..-12.0).contains(&e), "{e}");
        let f = fa.features(&frame, e);
        assert!(f.voicing > 0.9, "{f:?}");
        assert!(f.low_share > 0.95, "{f:?}");
        assert!(f.high_share < 0.01);
    }

    #[test]
    fn hostile_frames_do_not_panic() {
        let mut fa = FrameAnalyzer::new(16_000.0);
        for frame in [vec![], vec![f32::NAN; 400], vec![f32::INFINITY; 400], vec![1.0; 3], vec![0.0; 400]] {
            let e = FrameAnalyzer::energy_db(&frame);
            let _ = fa.features(&frame, e);
        }
    }
}
