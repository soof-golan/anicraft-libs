//! [`PcmBuffer`]: interleaved `f32` audio.

use anim_core::num::usize_to_f64;
use anim_core::{Tick, TimeRange};

/// Most channels any function in this crate accepts.
pub const MAX_CHANNELS: u16 = 64;
/// Highest sample rate accepted (Hz).
pub const MAX_SAMPLE_RATE: u32 = 768_000;

/// Interleaved 32-bit float PCM, nominally within `-1.0..=1.0`.
///
/// `samples.len()` should be a multiple of `channels`; [`PcmBuffer::frames`] ignores a trailing partial frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PcmBuffer {
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels per frame (1 = mono, 2 = stereo ...).
    pub channels: u16,
    /// Interleaved samples.
    pub samples: Vec<f32>,
}

impl PcmBuffer {
    /// A buffer from raw parts.
    #[must_use]
    pub const fn new(sample_rate: u32, channels: u16, samples: Vec<f32>) -> Self {
        Self { sample_rate, channels, samples }
    }

    /// `frames` frames of silence.
    #[must_use]
    pub fn silence(sample_rate: u32, channels: u16, frames: usize) -> Self {
        let n = frames.saturating_mul(usize::from(channels));
        Self { sample_rate, channels, samples: vec![0.0; n] }
    }

    /// True when the rate and channel count are in the range this crate supports.
    #[must_use]
    pub const fn is_valid_format(&self) -> bool {
        self.sample_rate > 0 && self.sample_rate <= MAX_SAMPLE_RATE && self.channels > 0 && self.channels <= MAX_CHANNELS
    }

    /// Number of whole frames.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len().checked_div(usize::from(self.channels)).unwrap_or(0)
    }

    /// True when there is no whole frame.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames() == 0
    }

    /// Length as a [`Tick`] duration.
    #[must_use]
    pub fn duration(&self) -> Tick {
        let frames = i64::try_from(self.frames()).unwrap_or(i64::MAX);
        Tick::from_units(frames, i64::from(self.sample_rate.max(1)))
    }

    /// Length in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        usize_to_f64(self.frames()) / f64::from(self.sample_rate.max(1))
    }

    /// The frame index at time `t`, floored and clamped to `0..=frames()`.
    #[must_use]
    pub fn frame_at(&self, t: Tick) -> usize {
        tick_to_frame(t, self.sample_rate).min(self.frames())
    }

    /// The `frames` frames starting at frame `start` (clamped to the buffer).
    #[must_use]
    pub fn slice_frames(&self, start: usize, frames: usize) -> Self {
        let total = self.frames();
        let start = start.min(total);
        let end = start.saturating_add(frames).min(total);
        let ch = usize::from(self.channels);
        let lo = start.saturating_mul(ch);
        let hi = end.saturating_mul(ch);
        let samples = self.samples.get(lo..hi).map(<[f32]>::to_vec).unwrap_or_default();
        Self { sample_rate: self.sample_rate, channels: self.channels, samples }
    }

    /// The part of the buffer inside `range`, which is half-open (`start..start + duration`) and clamped to the
    /// buffer, so a range that starts before zero or ends past the end yields the overlapping part only.
    #[must_use]
    pub fn slice_ticks(&self, range: TimeRange) -> Self {
        let rate = i64::from(self.sample_rate.max(1));
        let start = range.start.0.max(0);
        let end = range.start.0.saturating_add(range.duration.0).max(0);
        let lo = tick_to_frame(Tick(start), self.sample_rate);
        // Ceil the end so a range ending inside a frame keeps that frame.
        let hi = frame_ceil(end, rate);
        let hi = hi.min(self.frames());
        self.slice_frames(lo, hi.saturating_sub(lo))
    }

    /// Frame `i` as a slice of `channels` samples.
    #[must_use]
    pub fn frame(&self, i: usize) -> Option<&[f32]> {
        let ch = usize::from(self.channels);
        let lo = i.checked_mul(ch)?;
        self.samples.get(lo..lo.checked_add(ch)?)
    }

    /// Mixes every channel down to one by averaging.
    #[must_use]
    pub fn to_mono(&self) -> Self {
        self.remix(1)
    }

    /// Converts to `channels` channels.
    ///
    /// - Mono to N copies the signal to every channel.
    /// - N to mono averages all channels.
    /// - Other reductions keep the first `channels` channels; other expansions keep the existing channels and fill
    ///   the new ones with silence.
    #[must_use]
    pub fn remix(&self, channels: u16) -> Self {
        if channels == self.channels || channels == 0 || self.channels == 0 {
            return self.clone();
        }
        let src = usize::from(self.channels);
        let dst = usize::from(channels);
        let mut out = Vec::with_capacity(self.frames().saturating_mul(dst));
        for frame in self.samples.chunks_exact(src) {
            if dst == 1 {
                let sum: f32 = frame.iter().sum();
                out.push(sum / usize_to_f32(src));
            } else if src == 1 {
                let v = frame.first().copied().unwrap_or(0.0);
                out.extend(std::iter::repeat_n(v, dst));
            } else {
                for c in 0..dst {
                    out.push(frame.get(c).copied().unwrap_or(0.0));
                }
            }
        }
        Self { sample_rate: self.sample_rate, channels, samples: out }
    }

    /// Multiplies every sample by `gain`.
    pub fn apply_gain(&mut self, gain: f32) {
        for s in &mut self.samples {
            *s *= gain;
        }
    }

    /// The largest absolute sample value (0 for an empty buffer).
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0_f32, |m, s| if s.is_finite() { m.max(s.abs()) } else { m })
    }

    /// Replaces non-finite samples by zero and clamps to `-1.0..=1.0`. Decoders can produce either on damaged input.
    pub fn sanitize(&mut self) {
        for s in &mut self.samples {
            *s = if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 };
        }
    }

    /// Appends the frames of `other`, which must have the same rate and channel count. Returns false (and appends
    /// nothing) otherwise.
    pub fn append(&mut self, other: &Self) -> bool {
        if other.sample_rate != self.sample_rate || other.channels != self.channels {
            return false;
        }
        self.samples.extend_from_slice(&other.samples);
        true
    }
}

/// Floor of `t` in frames at `rate`, negative times giving 0.
#[must_use]
pub fn tick_to_frame(t: Tick, rate: u32) -> usize {
    let frames = t.to_units_floor(i64::from(rate.max(1))).max(0);
    usize::try_from(frames).unwrap_or(usize::MAX)
}

/// The time of frame `frame` at `rate`.
#[must_use]
pub fn frame_to_tick(frame: usize, rate: u32) -> Tick {
    Tick::from_units(i64::try_from(frame).unwrap_or(i64::MAX), i64::from(rate.max(1)))
}

fn frame_ceil(ticks: i64, rate: i64) -> usize {
    // ceil(ticks * rate / TICKS_PER_SECOND) via the floor of the next tick.
    let floor = Tick(ticks).to_units_floor(rate);
    let exact = Tick::from_units(floor, rate).0 == ticks;
    let n = if exact { floor } else { floor.saturating_add(1) };
    usize::try_from(n.max(0)).unwrap_or(usize::MAX)
}

/// `usize` to `f32` through `f64` (exact for the small values used here).
#[must_use]
pub const fn usize_to_f32(v: usize) -> f32 {
    anim_core::num::f64_to_f32(usize_to_f64(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> PcmBuffer {
        PcmBuffer::new(1000, 2, (0..n * 2).map(usize_to_f32).collect())
    }

    #[test]
    fn frames_and_duration() {
        let b = ramp(500);
        assert_eq!(b.frames(), 500);
        assert!((b.seconds() - 0.5).abs() < 1e-9);
        assert_eq!(b.duration(), Tick::from_units(500, 1000));
        assert_eq!(b.frame_at(Tick::from_units(250, 1000)), 250);
        assert_eq!(b.frame_at(Tick::from_units(9999, 1000)), 500);
        assert_eq!(b.frame_at(Tick(-5)), 0);
    }

    #[test]
    fn slice_by_ticks_is_half_open_and_clamped() {
        let b = ramp(100);
        let r = TimeRange::new(Tick::from_units(10, 1000), Tick::from_units(20, 1000));
        let s = b.slice_ticks(r);
        assert_eq!(s.frames(), 20);
        assert_eq!(s.samples.first().copied(), Some(20.0));
        let before = TimeRange::new(Tick::from_units(-50, 1000), Tick::from_units(60, 1000));
        assert_eq!(b.slice_ticks(before).frames(), 10);
        let past = TimeRange::new(Tick::from_units(90, 1000), Tick::from_units(500, 1000));
        assert_eq!(b.slice_ticks(past).frames(), 10);
        let none = TimeRange::new(Tick::from_units(500, 1000), Tick::from_units(5, 1000));
        assert!(b.slice_ticks(none).is_empty());
        let huge = TimeRange::new(Tick(i64::MAX), Tick(i64::MAX));
        assert!(b.slice_ticks(huge).is_empty());
    }

    #[test]
    fn remix_rules() {
        let st = PcmBuffer::new(8000, 2, vec![1.0, 0.0, 0.5, 0.5]);
        assert_eq!(st.to_mono().samples, vec![0.5, 0.5]);
        let mono = PcmBuffer::new(8000, 1, vec![0.25, -0.25]);
        assert_eq!(mono.remix(2).samples, vec![0.25, 0.25, -0.25, -0.25]);
        assert_eq!(st.remix(3).samples, vec![1.0, 0.0, 0.0, 0.5, 0.5, 0.0]);
        assert_eq!(st.remix(2), st);
    }

    #[test]
    fn sanitize_and_peak() {
        let mut b = PcmBuffer::new(8000, 1, vec![f32::NAN, 3.0, -2.0, f32::INFINITY, 0.5]);
        assert!((b.peak() - 3.0).abs() < 1e-6);
        b.sanitize();
        assert_eq!(b.samples, vec![0.0, 1.0, -1.0, 0.0, 0.5]);
    }

    #[test]
    fn zero_channels_is_empty_not_a_panic() {
        let b = PcmBuffer::new(8000, 0, vec![1.0; 10]);
        assert_eq!(b.frames(), 0);
        assert!(b.slice_ticks(TimeRange::new(Tick::ZERO, Tick::MAX)).is_empty());
        assert!(!b.is_valid_format());
    }
}
