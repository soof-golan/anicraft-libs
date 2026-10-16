//! The streaming pipeline shared by the offline and live front ends: decimation, framing, voice activity,
//! classification and smoothing.

use anim_core::Tick;
use serde::{Deserialize, Serialize};

use crate::classify::{Context, VOICED, classify};
use crate::features::{FrameAnalyzer, MIN_DB};
use crate::num::{u32_f32, u64_i64, usize_i64};
use crate::viseme::Viseme;

/// Highest rate the spectral analysis runs at; faster input is decimated by an integer factor.
const TARGET_RATE: u32 = 16_000;
/// Slowest rise of the noise-floor estimate, dB per frame (2 dB per second at a 10 ms hop).
const FLOOR_RISE_DB: f32 = 0.02;
/// Decay of the speech peak, dB per frame (3 dB per second).
const PEAK_DECAY_DB: f32 = 0.03;
/// Frame-to-frame energy rise out of silence that marks a stop burst, dB.
const ONSET_RISE_DB: f32 = 8.0;
/// Frames a burst viseme (D) is held at least.
const BURST_HOLD: u32 = 2;

/// A viseme starting at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VisemeKey {
    /// When the mouth shape starts.
    pub start: Tick,
    /// The shape.
    pub viseme: Viseme,
}

/// Tuning knobs. The defaults suit speech from a microphone or a recording.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Frames quieter than this (dBFS) are always silence.
    pub silence_floor_db: f32,
    /// Silence threshold above the tracked noise floor, dB.
    pub noise_margin_db: f32,
    /// Silence threshold below the tracked speech peak, dB (quiet fricatives after loud vowels stay audible).
    pub dynamic_range_db: f32,
    /// Consecutive frames a new viseme must be seen before it is shown.
    pub confirm_frames: u32,
    /// Consecutive silent frames before the mouth returns to Neutral.
    pub release_frames: u32,
    /// Minimum frames a viseme stays on screen (bursts excepted).
    pub min_hold_frames: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self { silence_floor_db: -52.0, noise_margin_db: 12.0, dynamic_range_db: 42.0, confirm_frames: 2, release_frames: 3, min_hold_frames: 4 }
    }
}

impl Config {
    /// Replaces non-finite or nonsensical values with defaults and clamps counts to sane ranges.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        let d = Self::default();
        let fix = |v: f32, fallback: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { fallback };
        self.silence_floor_db = fix(self.silence_floor_db, d.silence_floor_db, -100.0, 0.0);
        self.noise_margin_db = fix(self.noise_margin_db, d.noise_margin_db, 0.0, 60.0);
        self.dynamic_range_db = fix(self.dynamic_range_db, d.dynamic_range_db, 10.0, 100.0);
        self.confirm_frames = self.confirm_frames.clamp(1, 20);
        self.release_frames = self.release_frames.clamp(1, 50);
        self.min_hold_frames = self.min_hold_frames.clamp(1, 100);
        self
    }
}

/// Averages `factor` input samples into one output sample.
struct Decimator {
    factor: usize,
    sum: f32,
    count: usize,
}

impl Decimator {
    fn push(&mut self, x: f32) -> Option<f32> {
        self.sum += x;
        self.count = self.count.saturating_add(1);
        if self.count < self.factor {
            return None;
        }
        let out = self.sum / crate::num::usize_f32(self.factor);
        self.sum = 0.0;
        self.count = 0;
        Some(out)
    }
}

/// Voice-activity tracker: noise floor, speech peak and the silence threshold derived from them.
struct Vad {
    floor: f32,
    peak: f32,
}

impl Vad {
    const fn new() -> Self {
        Self { floor: -60.0, peak: -60.0 }
    }

    /// Updates with this frame's level; returns the silence threshold for the frame.
    fn update(&mut self, energy_db: f32, cfg: &Config) -> f32 {
        if energy_db < self.floor {
            self.floor = energy_db.max(MIN_DB);
        } else {
            self.floor += FLOOR_RISE_DB.min(energy_db - self.floor);
        }
        self.peak = (self.peak - PEAK_DECAY_DB).max(energy_db);
        cfg.silence_floor_db.max(self.floor + cfg.noise_margin_db).max(self.peak - cfg.dynamic_range_db)
    }
}

/// Debounces raw frame labels into stable viseme changes (confirmation, minimum hold, back-dating).
struct Smoother {
    current: Viseme,
    last_start: u64,
    candidate: Viseme,
    run: u32,
    candidate_start: u64,
}

impl Smoother {
    const fn new() -> Self {
        Self { current: Viseme::Neutral, last_start: 0, candidate: Viseme::Neutral, run: 0, candidate_start: 0 }
    }

    /// Feeds frame number `frame`'s raw label; returns `(start frame, viseme)` when the shown viseme changes.
    fn update(&mut self, frame: u64, raw: Viseme, cfg: &Config) -> Option<(u64, Viseme)> {
        if raw == self.current {
            self.run = 0;
            return None;
        }
        if raw == self.candidate && self.run > 0 {
            self.run = self.run.saturating_add(1);
        } else {
            self.candidate = raw;
            self.run = 1;
            self.candidate_start = frame;
        }
        let need = match raw {
            Viseme::D => 1,
            Viseme::Neutral => cfg.release_frames,
            _ => cfg.confirm_frames,
        };
        let hold = u64::from(if self.current == Viseme::D { BURST_HOLD } else { cfg.min_hold_frames });
        let earliest = self.last_start.saturating_add(hold);
        if self.run < need || frame < earliest {
            return None;
        }
        let start = self.candidate_start.max(earliest).min(frame);
        self.current = raw;
        self.last_start = start;
        self.run = 0;
        Some((start, raw))
    }
}

/// The streaming analyser. Sample-for-sample deterministic, independent of how the input is chunked.
pub struct Pipeline {
    rate: u32,
    factor: usize,
    decimator: Decimator,
    analyzer: FrameAnalyzer,
    win: usize,
    hop: usize,
    buffer: Vec<f32>,
    cfg: Config,
    vad: Vad,
    smoother: Smoother,
    frame: u64,
    prev_energy: f32,
    prev_silent: bool,
    /// The frame of a burst waiting for the next frame's voicing.
    pending_burst: Option<u64>,
}

impl Pipeline {
    pub fn new(sample_rate: u32, cfg: Config) -> Self {
        let rate = sample_rate.clamp(4_000, 384_000);
        let factor = usize::try_from((rate / TARGET_RATE).max(1)).unwrap_or(1);
        let eff = u32_f32(rate) / crate::num::usize_f32(factor);
        Self {
            rate,
            factor,
            decimator: Decimator { factor, sum: 0.0, count: 0 },
            analyzer: FrameAnalyzer::new(eff),
            win: FrameAnalyzer::frame_len_for(eff),
            hop: FrameAnalyzer::hop_for(eff),
            buffer: Vec::new(),
            cfg: cfg.sanitized(),
            vad: Vad::new(),
            smoother: Smoother::new(),
            frame: 0,
            prev_energy: MIN_DB,
            prev_silent: true,
            pending_burst: None,
        }
    }

    pub const fn current(&self) -> Viseme {
        self.smoother.current
    }

    /// Input samples per analysis window.
    pub const fn window_input_samples(&self) -> usize {
        self.win.saturating_mul(self.factor)
    }

    /// Input samples per hop.
    pub const fn hop_input_samples(&self) -> usize {
        self.hop.saturating_mul(self.factor)
    }

    pub const fn config(&self) -> &Config {
        &self.cfg
    }

    pub const fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Start time of the hop that frame `frame` describes (the middle hop of its window).
    fn frame_start(&self, frame: u64) -> Tick {
        let offset = self.win.saturating_sub(self.hop) / 2;
        let eff_samples = u64_i64(frame).saturating_mul(usize_i64(self.hop)).saturating_add(usize_i64(offset));
        Tick::from_units(eff_samples.saturating_mul(usize_i64(self.factor)), i64::from(self.rate))
    }

    /// Feeds samples; returns every viseme change they completed, with start times.
    pub fn push(&mut self, samples: &[f32]) -> Vec<VisemeKey> {
        let mut out = Vec::new();
        for &x in samples {
            let x = if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };
            let Some(d) = self.decimator.push(x) else { continue };
            self.buffer.push(d);
            if self.buffer.len() < self.win {
                continue;
            }
            let (raw, voiced) = self.analyze_frame();
            let burst = self.pending_burst.take();
            let mut changes = Vec::new();
            // A burst is judged one frame late: the first window of a vowel that starts abruptly is also an
            // unvoiced rise out of silence, and the next frame, now mostly vowel, tells the two apart.
            if let Some(at) = burst.filter(|_| !voiced) {
                changes.extend(self.smoother.update(at, Viseme::D, &self.cfg));
            }
            let raw = if raw == Viseme::D {
                self.pending_burst = Some(self.frame);
                Viseme::Neutral
            } else {
                raw
            };
            changes.extend(self.smoother.update(self.frame, raw, &self.cfg));
            out.extend(changes.into_iter().map(|(start, viseme)| VisemeKey { start: self.frame_start(start), viseme }));
            self.frame = self.frame.saturating_add(1);
            self.buffer.drain(..self.hop.min(self.buffer.len()));
        }
        out
    }

    /// The frame's raw label and whether it sounds voiced.
    fn analyze_frame(&mut self) -> (Viseme, bool) {
        let Some(frame) = self.buffer.get(..self.win) else { return (Viseme::Neutral, false) };
        let energy = FrameAnalyzer::energy_db(frame);
        let threshold = self.vad.update(energy, &self.cfg);
        let silent = energy < threshold;
        let onset = self.prev_silent && energy - self.prev_energy >= ONSET_RISE_DB;
        self.prev_energy = energy;
        self.prev_silent = silent;
        if silent {
            return (Viseme::Neutral, false);
        }
        let features = self.analyzer.features(frame, energy);
        let ctx = Context { current: self.smoother.current, peak_db: self.vad.peak, onset };
        (classify(&features, ctx), features.voicing >= VOICED)
    }
}
