//! Waveform peaks for the timeline: the minimum and maximum sample per horizontal bin.

use std::ops::Range;

use anim_core::TimeRange;

use crate::buffer::{PcmBuffer, tick_to_frame};

/// Minimum and maximum sample value over one bin, across all channels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Peak {
    /// Lowest sample (at most 0 for audio that crosses zero).
    pub min: f32,
    /// Highest sample.
    pub max: f32,
}

impl Peak {
    /// The peak of an empty bin.
    pub const SILENT: Self = Self { min: 0.0, max: 0.0 };

    /// The peak covering both `self` and `other`.
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        Self { min: if other.min < self.min { other.min } else { self.min }, max: if other.max > self.max { other.max } else { self.max } }
    }

    /// The larger of `|min|` and `|max|`.
    #[must_use]
    pub const fn magnitude(self) -> f32 {
        self.min.abs().max(self.max.abs())
    }
}

/// `bins` peaks over the whole buffer.
#[must_use]
pub fn waveform_peaks(buf: &PcmBuffer, bins: usize) -> Vec<Peak> {
    waveform_peaks_frames(buf, 0..buf.frames(), bins)
}

/// `bins` peaks over the part of the buffer inside `range`.
#[must_use]
pub fn waveform_peaks_range(buf: &PcmBuffer, range: TimeRange, bins: usize) -> Vec<Peak> {
    let start = tick_to_frame(range.start, buf.sample_rate);
    let end = tick_to_frame(anim_core::Tick(range.start.0.saturating_add(range.duration.0)), buf.sample_rate);
    waveform_peaks_frames(buf, start..end, bins)
}

/// `bins` peaks over the frames in `frames` (clamped to the buffer).
///
/// Bin `b` covers frames `start + b * n / bins .. start + (b + 1) * n / bins` for `n = frames.len()`. When there are
/// more bins than frames (zoomed far in) each bin shows the frame it falls on, and bins outside the buffer are
/// silent, so the result always has exactly `bins` entries.
#[must_use]
pub fn waveform_peaks_frames(buf: &PcmBuffer, frames: Range<usize>, bins: usize) -> Vec<Peak> {
    if bins == 0 {
        return Vec::new();
    }
    let total = buf.frames();
    let ch = usize::from(buf.channels);
    let start = frames.start.min(total);
    let end = frames.end.min(total).max(start);
    let n = end.saturating_sub(start);
    let mut out = Vec::with_capacity(bins);
    for b in 0..bins {
        let lo = start.saturating_add(split(n, b, bins));
        let hi = start.saturating_add(split(n, b.saturating_add(1), bins)).max(lo.saturating_add(1)).min(end);
        if lo >= hi {
            out.push(Peak::SILENT);
            continue;
        }
        let slice = buf.samples.get(lo.saturating_mul(ch)..hi.saturating_mul(ch)).unwrap_or_default();
        out.push(slice_peak(slice));
    }
    out
}

fn slice_peak(slice: &[f32]) -> Peak {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for &s in slice {
        if s.is_finite() {
            min = min.min(s);
            max = max.max(s);
        }
    }
    if min > max { Peak::SILENT } else { Peak { min, max } }
}

/// `n * k / d` without overflow.
fn split(n: usize, k: usize, d: usize) -> usize {
    let v = u128::try_from(n).unwrap_or(0).saturating_mul(u128::try_from(k).unwrap_or(0)).checked_div(u128::try_from(d).unwrap_or(1)).unwrap_or(0);
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// Frames per block at the finest level of a [`PeakPyramid`].
const BASE_BLOCK: usize = 64;
/// Blocks merged into one at each coarser level.
const FANOUT: usize = 8;

/// Precomputed peaks at several resolutions, so a long sound can be drawn at any zoom without scanning samples.
#[derive(Clone, Debug)]
pub struct PeakPyramid {
    levels: Vec<Level>,
}

#[derive(Clone, Debug)]
struct Level {
    block: usize,
    peaks: Vec<Peak>,
}

impl PeakPyramid {
    /// Builds the levels for `buf`: blocks of 64 frames, then 512, 4096 ... until one block covers everything.
    #[must_use]
    pub fn build(buf: &PcmBuffer) -> Self {
        let ch = usize::from(buf.channels).max(1);
        let finest: Vec<Peak> = buf.samples.chunks(BASE_BLOCK.saturating_mul(ch)).map(slice_peak).collect();
        let mut levels = vec![Level { block: BASE_BLOCK, peaks: finest }];
        while levels.last().is_some_and(|l| l.peaks.len() > 1) {
            let Some(last) = levels.last() else { break };
            let coarser: Vec<Peak> = last.peaks.chunks(FANOUT).map(|c| c.iter().fold(Peak::SILENT, |a, p| a.merge(*p))).collect();
            let block = last.block.saturating_mul(FANOUT);
            levels.push(Level { block, peaks: coarser });
        }
        Self { levels }
    }

    /// `bins` peaks over `frames` of `buf` (the buffer this pyramid was built from), using the coarsest level whose
    /// blocks are no larger than a bin, or the samples themselves when a bin is smaller than the finest block.
    #[must_use]
    pub fn query(&self, buf: &PcmBuffer, frames: Range<usize>, bins: usize) -> Vec<Peak> {
        if bins == 0 {
            return Vec::new();
        }
        let total = buf.frames();
        let start = frames.start.min(total);
        let end = frames.end.min(total).max(start);
        let n = end.saturating_sub(start);
        let per_bin = n.checked_div(bins).unwrap_or(0);
        let Some(level) = self.levels.iter().rev().find(|l| l.block <= per_bin) else {
            return waveform_peaks_frames(buf, start..end, bins);
        };
        (0..bins)
            .map(|b| {
                let lo = start.saturating_add(split(n, b, bins));
                let hi = start.saturating_add(split(n, b.saturating_add(1), bins)).max(lo.saturating_add(1)).min(end);
                let first = lo.checked_div(level.block).unwrap_or(0);
                let last = hi.saturating_sub(1).checked_div(level.block).unwrap_or(0);
                if lo >= hi {
                    return Peak::SILENT;
                }
                level
                    .peaks
                    .get(first..=last)
                    .unwrap_or_default()
                    .iter()
                    .fold(None, |a: Option<Peak>, p| Some(a.map_or(*p, |a| a.merge(*p))))
                    .unwrap_or(Peak::SILENT)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_core::Tick;

    fn square(frames: usize) -> PcmBuffer {
        // Alternating blocks of 100 frames at +0.5 and -0.25.
        PcmBuffer::new(1000, 1, (0..frames).map(|i| if (i / 100) % 2 == 0 { 0.5 } else { -0.25 }).collect())
    }

    #[test]
    fn min_max_per_bin() {
        let b = square(1000);
        let p = waveform_peaks(&b, 10);
        assert_eq!(p.len(), 10);
        assert_eq!(p[0], Peak { min: 0.5, max: 0.5 });
        assert_eq!(p[1], Peak { min: -0.25, max: -0.25 });
        let two = waveform_peaks(&b, 5);
        assert_eq!(two[0], Peak { min: -0.25, max: 0.5 });
    }

    #[test]
    fn channels_are_combined() {
        let b = PcmBuffer::new(1000, 2, vec![0.1, -0.9, 0.2, 0.8]);
        assert_eq!(waveform_peaks(&b, 1), vec![Peak { min: -0.9, max: 0.8 }]);
    }

    #[test]
    fn more_bins_than_frames_and_empty_input() {
        let b = PcmBuffer::new(1000, 1, vec![0.5, -0.5]);
        let p = waveform_peaks(&b, 6);
        assert_eq!(p.len(), 6);
        assert_eq!(p[0], Peak { min: 0.5, max: 0.5 });
        assert_eq!(p[5], Peak { min: -0.5, max: -0.5 });
        let e = waveform_peaks(&PcmBuffer::new(1000, 1, vec![]), 4);
        assert_eq!(e, vec![Peak::SILENT; 4]);
        assert_eq!(waveform_peaks(&b, 0).len(), 0);
    }

    #[test]
    fn range_by_ticks() {
        let b = square(1000);
        let r = TimeRange::new(Tick::from_units(100, 1000), Tick::from_units(100, 1000));
        assert_eq!(waveform_peaks_range(&b, r, 4), vec![Peak { min: -0.25, max: -0.25 }; 4]);
        let outside = TimeRange::new(Tick::from_units(5000, 1000), Tick::from_units(100, 1000));
        assert_eq!(waveform_peaks_range(&b, outside, 3), vec![Peak::SILENT; 3]);
    }

    #[test]
    fn nan_samples_are_ignored() {
        let b = PcmBuffer::new(1000, 1, vec![f32::NAN, 0.3, f32::INFINITY]);
        assert_eq!(waveform_peaks(&b, 1), vec![Peak { min: 0.3, max: 0.3 }]);
    }

    #[test]
    fn pyramid_agrees_with_direct_scan_at_coarse_zoom() {
        let b = PcmBuffer::new(8000, 2, (0..2 * 100_000).map(|i| ((i as f32) * 0.37).sin() * (1.0 - (i as f32) / 250_000.0)).collect());
        let pyr = PeakPyramid::build(&b);
        for bins in [4_usize, 50, 300, 1000] {
            let fast = pyr.query(&b, 0..b.frames(), bins);
            let slow = waveform_peaks(&b, bins);
            assert_eq!(fast.len(), slow.len());
            for (f, s) in fast.iter().zip(&slow) {
                // The pyramid may include up to a block of neighbouring frames, so it only ever over-reports.
                assert!(f.min <= s.min + 1e-6 && f.max >= s.max - 1e-6, "{f:?} vs {s:?}");
            }
        }
        // Deep zoom falls back to the exact scan.
        assert_eq!(pyr.query(&b, 100..200, 10), waveform_peaks_frames(&b, 100..200, 10));
    }

    #[test]
    fn pyramid_of_empty_buffer() {
        let b = PcmBuffer::new(8000, 1, vec![]);
        let pyr = PeakPyramid::build(&b);
        assert_eq!(pyr.query(&b, 0..10, 3), vec![Peak::SILENT; 3]);
    }
}
