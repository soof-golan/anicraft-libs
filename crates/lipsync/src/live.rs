//! Streaming lip sync for microphone input.

use anim_core::Tick;

use crate::engine::{Config, Pipeline, VisemeKey};
use crate::num::usize_i64;
use crate::viseme::Viseme;

/// Incremental audio-to-viseme analyser: feed microphone chunks of any size, read viseme changes.
///
/// A change is reported once the evidence is in: roughly half a window (12.5 ms) plus the confirmation frames, which
/// is about 40-50 ms after the sound begins (see [`Self::latency`]).
pub struct LiveLipSync {
    pipeline: Pipeline,
}

impl LiveLipSync {
    /// Analyser for mono `f32` samples at `sample_rate` Hz (clamped to 4 kHz ..= 384 kHz).
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self::with_config(sample_rate, Config::default())
    }

    /// Analyser with explicit tuning.
    #[must_use]
    pub fn with_config(sample_rate: u32, config: Config) -> Self {
        Self { pipeline: Pipeline::new(sample_rate, config) }
    }

    /// Feeds samples; returns the viseme the mouth should now show if it changed during this chunk (the last
    /// change when there were several), otherwise `None`.
    pub fn push(&mut self, samples: &[f32]) -> Option<Viseme> {
        self.pipeline.push(samples).last().map(|k| k.viseme)
    }

    /// Like [`Self::push`] but returns every change with its start time (measured from the first sample pushed).
    pub fn push_keys(&mut self, samples: &[f32]) -> Vec<VisemeKey> {
        self.pipeline.push(samples)
    }

    /// The viseme currently shown.
    #[must_use]
    pub const fn current(&self) -> Viseme {
        self.pipeline.current()
    }

    /// Typical delay between a sound starting and [`Self::push`] reporting it.
    #[must_use]
    pub fn latency(&self) -> Tick {
        let cfg = self.pipeline.config();
        let hops = usize::try_from(cfg.confirm_frames).unwrap_or(2);
        let samples = (self.pipeline.window_input_samples() / 2).saturating_add(self.pipeline.hop_input_samples().saturating_mul(hops));
        Tick::from_units(usize_i64(samples), i64::from(self.pipeline.sample_rate()))
    }

    /// Ends the stream: feeds silence until pending changes settle and returns them.
    pub fn finish(&mut self) -> Vec<VisemeKey> {
        let cfg = *self.pipeline.config();
        let frames = usize::try_from(cfg.release_frames.saturating_add(cfg.min_hold_frames).saturating_add(cfg.confirm_frames)).unwrap_or(16);
        let n = self.pipeline.window_input_samples().saturating_add(self.pipeline.hop_input_samples().saturating_mul(frames));
        self.pipeline.push(&vec![0.0; n])
    }

    /// Forgets all history and restarts at time zero with a Neutral mouth.
    pub fn reset(&mut self) {
        let rate = self.pipeline.sample_rate();
        let cfg = *self.pipeline.config();
        self.pipeline = Pipeline::new(rate, cfg);
    }
}
