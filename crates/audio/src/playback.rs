//! The playback actor.
//!
//! ```text
//!   client ── PlaybackCommand ──► actor thread ──► Renderer ◄── device callback (or wall-clock ticks)
//!   client ◄── PlaybackEvent ───────────────────────┘
//! ```
//!
//! [`PlaybackActor`] is the handle clients hold. Its thread owns the output device (cpal streams are not `Send` on
//! every backend), prepares sounds off the audio thread (resampling and channel conversion happen on the actor thread,
//! never in the device callback) and hands them to the [`Renderer`], which is what the device callback calls. The
//! renderer owns all playback state; the client learns about it only through [`PlaybackEvent`]s.
//!
//! The device is abstracted by [`OutputBackend`], so the whole protocol runs in tests with a fake. When no device can
//! be opened the actor falls back to a wall clock: the playhead still advances in real time and events still flow, but
//! nothing is audible ([`ClockSource::WallClock`]).

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anim_core::{Tick, TimeRange};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};

use crate::buffer::{MAX_CHANNELS, MAX_SAMPLE_RATE, PcmBuffer, frame_to_tick, tick_to_frame};
use crate::error::{AudioError, Result};
use crate::resample::resample;

/// Rate and channel count the output runs at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat {
    /// Frames per second.
    pub sample_rate: u32,
    /// Interleaved channels per frame.
    pub channels: u16,
}

impl OutputFormat {
    /// What the wall-clock fallback pretends to be.
    pub const FALLBACK: Self = Self { sample_rate: 48_000, channels: 2 };

    const fn is_valid(self) -> bool {
        self.sample_rate > 0 && self.sample_rate <= MAX_SAMPLE_RATE && self.channels > 0 && self.channels <= MAX_CHANNELS
    }
}

/// What a client can ask the playback actor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaybackCommand {
    /// Replaces the sound. The playhead and the play state are kept, so a re-mixed track can be swapped in while
    /// playing. Any rate and channel count are accepted; the actor converts. An empty buffer unloads.
    Load(PcmBuffer),
    /// Moves the playhead to `from` and starts playing.
    Play {
        /// Where to start.
        from: Tick,
    },
    /// Continues playing from the current playhead.
    Resume,
    /// Stops advancing and keeps the playhead.
    Pause,
    /// Moves the playhead; playing or paused is unchanged.
    Seek(Tick),
    /// Output gain, clamped to `0.0..=4.0`; changes are ramped over one device buffer.
    SetVolume(f32),
    /// Loops the given time range while playing (`None` = no loop). Empty ranges are ignored.
    SetLoop(Option<TimeRange>),
    /// Pauses and moves the playhead to zero.
    Stop,
}

/// Where the clock comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSource {
    /// Frames consumed by the audio device, minus its latency: the playhead is what is audible.
    Device,
    /// No device: real time since playback started.
    WallClock,
}

/// The playhead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaybackClock {
    /// Position of the sound being heard now (device frames consumed minus latency, or wall time).
    pub position: Tick,
    /// True while the playhead advances.
    pub playing: bool,
}

/// What the actor reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackEvent {
    /// The actor is running; sent once, first.
    Ready {
        /// Format of the output.
        format: OutputFormat,
        /// Device or wall clock.
        clock_source: ClockSource,
    },
    /// The playhead, about every device buffer while playing and once after each state change.
    Clock(PlaybackClock),
    /// A [`PlaybackCommand::Load`] was applied.
    Loaded {
        /// Length of the sound.
        duration: Tick,
    },
    /// The playhead reached the end of the sound (once per pass; the clock keeps running until paused).
    Finished,
    /// Something failed (a sound that cannot be converted, a device that stopped).
    Error(String),
}

/// An audio output. The actor creates it on its own thread, so it need not be `Send`.
pub trait OutputBackend: Sized {
    /// Whatever must stay alive for audio to keep flowing (a cpal `Stream`). Dropping it stops the output.
    type Stream;

    /// The format the device runs at.
    fn format(&self) -> OutputFormat;

    /// Starts the device. From now on the device calls [`Renderer::render`] for every buffer it needs.
    ///
    /// # Errors
    /// [`AudioError::Device`] when the device refuses to start. The actor then uses the wall clock.
    fn start(self, renderer: Renderer) -> Result<Self::Stream>;
}

/// Owns the playback state and produces device buffers. Created by the actor; backends call [`Renderer::render`].
#[derive(Debug)]
pub struct Renderer {
    format: OutputFormat,
    commands: Receiver<PlaybackCommand>,
    events: Sender<PlaybackEvent>,
    buffer: Option<PcmBuffer>,
    cursor: usize,
    playing: bool,
    volume: f32,
    gain: f32,
    loop_frames: Option<(usize, usize)>,
    finished_sent: bool,
    dirty: bool,
    scratch: Vec<f32>,
}

impl Renderer {
    /// A renderer reading commands from `commands` and reporting to `events`.
    #[must_use]
    pub const fn new(format: OutputFormat, commands: Receiver<PlaybackCommand>, events: Sender<PlaybackEvent>) -> Self {
        Self {
            format,
            commands,
            events,
            buffer: None,
            cursor: 0,
            playing: false,
            volume: 1.0,
            gain: 1.0,
            loop_frames: None,
            finished_sent: false,
            dirty: true,
            scratch: Vec::new(),
        }
    }

    /// The output format.
    #[must_use]
    pub const fn format(&self) -> OutputFormat {
        self.format
    }

    /// A renderer in its initial state on the same channels. Backends use it to retry a stream configuration after
    /// the first attempt consumed the renderer; only valid before any audio has been rendered.
    #[must_use]
    pub fn fresh(&self) -> Self {
        Self::new(self.format, self.commands.clone(), self.events.clone())
    }

    /// A sender for reporting device failures from an error callback.
    #[must_use]
    pub fn event_sender(&self) -> Sender<PlaybackEvent> {
        self.events.clone()
    }

    /// Fills `out` (interleaved, `format().channels` per frame) with the next audio, applying pending commands first.
    /// `latency_frames` is how long after this call the first frame of `out` will be heard; the reported clock is
    /// behind the frames handed out by that much. Real-time safe apart from channel traffic: no allocation and no
    /// resampling.
    pub fn render(&mut self, out: &mut [f32], latency_frames: usize) {
        self.drain_commands();
        out.fill(0.0);
        let ch = usize::from(self.format.channels).max(1);
        let frames = out.len().checked_div(ch).unwrap_or(0);
        let start_cursor = self.cursor;
        if self.playing {
            self.fill(out, frames, ch);
        }
        self.apply_gain(out, frames, ch);
        self.report(start_cursor, latency_frames);
    }

    /// Advances the playhead by `frames` without producing sound: what the wall-clock fallback does.
    pub fn advance_silent(&mut self, frames: usize) {
        let ch = usize::from(self.format.channels).max(1);
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        scratch.resize(frames.saturating_mul(ch).min(1 << 20), 0.0);
        let n = scratch.len();
        self.render(&mut scratch, 0);
        // `render` handed out at most `n` samples; account for a request larger than the scratch limit.
        let missed = frames.saturating_sub(n.checked_div(ch).unwrap_or(0));
        if self.playing {
            self.cursor = self.cursor.saturating_add(missed);
        }
        self.scratch = scratch;
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.try_recv() {
            self.apply(cmd);
        }
    }

    fn frames_at(&self, t: Tick) -> usize {
        tick_to_frame(t, self.format.sample_rate)
    }

    fn apply(&mut self, cmd: PlaybackCommand) {
        self.dirty = true;
        match cmd {
            PlaybackCommand::Load(buf) => match prepare(&buf, self.format) {
                Ok(prepared) => {
                    let duration = prepared.duration();
                    self.buffer = if prepared.is_empty() { None } else { Some(prepared) };
                    self.finished_sent = false;
                    self.send(PlaybackEvent::Loaded { duration });
                }
                Err(e) => self.send(PlaybackEvent::Error(e.to_string())),
            },
            PlaybackCommand::Play { from } => {
                self.cursor = self.frames_at(from);
                self.finished_sent = false;
                self.playing = true;
            }
            PlaybackCommand::Resume => self.playing = true,
            PlaybackCommand::Pause => self.playing = false,
            PlaybackCommand::Seek(t) => {
                self.cursor = self.frames_at(t);
                self.finished_sent = false;
            }
            PlaybackCommand::SetVolume(v) => self.volume = if v.is_finite() { v.clamp(0.0, 4.0) } else { 0.0 },
            PlaybackCommand::SetLoop(range) => {
                self.loop_frames = range.and_then(|r| {
                    let start = self.frames_at(r.start);
                    let end = self.frames_at(Tick(r.start.0.saturating_add(r.duration.0)));
                    (end > start).then_some((start, end))
                });
            }
            PlaybackCommand::Stop => {
                self.playing = false;
                self.cursor = 0;
                self.finished_sent = false;
            }
        }
    }

    /// Copies sound at the cursor into `out`, wrapping at the loop end, and moves the cursor.
    fn fill(&mut self, out: &mut [f32], frames: usize, ch: usize) {
        let total = self.buffer.as_ref().map_or(0, PcmBuffer::frames);
        let mut written = 0_usize;
        while written < frames {
            if let Some((start, end)) = self.loop_frames
                && self.cursor >= end
            {
                // Wrap, keeping the overshoot so a seek past the loop end lands inside the loop.
                let len = end.saturating_sub(start).max(1);
                self.cursor = start.saturating_add(self.cursor.saturating_sub(start).checked_rem(len).unwrap_or(0));
            }
            let limit = match self.loop_frames {
                Some((_, end)) if self.cursor < end => end,
                _ => usize::MAX,
            };
            let n = frames.saturating_sub(written).min(limit.saturating_sub(self.cursor));
            if n == 0 {
                break;
            }
            let copy = n.min(total.saturating_sub(self.cursor));
            if copy > 0
                && let Some(buf) = &self.buffer
            {
                let src = buf.samples.get(self.cursor.saturating_mul(ch)..self.cursor.saturating_add(copy).saturating_mul(ch));
                let dst = out.get_mut(written.saturating_mul(ch)..written.saturating_add(copy).saturating_mul(ch));
                if let (Some(src), Some(dst)) = (src, dst) {
                    dst.copy_from_slice(src);
                }
            }
            written = written.saturating_add(n);
            self.cursor = self.cursor.saturating_add(n);
        }
    }

    fn apply_gain(&mut self, out: &mut [f32], frames: usize, ch: usize) {
        let (from, to) = (self.gain, self.volume);
        self.gain = to;
        if (from - 1.0).abs() < f32::EPSILON && (to - 1.0).abs() < f32::EPSILON {
            return;
        }
        let steps = crate::buffer::usize_to_f32(frames.max(1));
        for (i, frame) in out.chunks_exact_mut(ch).enumerate() {
            let g = (to - from).mul_add(crate::buffer::usize_to_f32(i.saturating_add(1)) / steps, from);
            for s in frame {
                *s *= g;
            }
        }
    }

    fn report(&mut self, start_cursor: usize, latency_frames: usize) {
        if let Some(buf) = &self.buffer
            && self.playing
            && !self.finished_sent
            && self.loop_frames.is_none()
            && start_cursor < buf.frames()
            && self.cursor >= buf.frames()
        {
            self.finished_sent = true;
            self.send(PlaybackEvent::Finished);
        }
        if self.playing || self.dirty {
            // The block just rendered is not audible for `latency_frames`, so what is heard now is that much earlier.
            let heard = self.cursor.saturating_sub(latency_frames);
            let position = frame_to_tick(if self.playing { heard } else { self.cursor }, self.format.sample_rate);
            self.send(PlaybackEvent::Clock(PlaybackClock { position, playing: self.playing }));
            self.dirty = false;
        }
    }

    fn send(&self, event: PlaybackEvent) {
        // A full queue means the client stopped listening; dropping is the only thing a real-time callback can do.
        let _ = self.events.try_send(event);
    }
}

/// Converts `buf` to `format` (rate and channel count). A sound that already matches is cloned.
///
/// # Errors
/// [`AudioError::InvalidFormat`] for an unusable buffer or output format.
pub fn prepare(buf: &PcmBuffer, format: OutputFormat) -> Result<PcmBuffer> {
    if !format.is_valid() {
        return Err(AudioError::InvalidFormat(format!("output {} Hz, {} channels", format.sample_rate, format.channels)));
    }
    if buf.is_empty() {
        return Ok(PcmBuffer::new(format.sample_rate, format.channels, Vec::new()));
    }
    let mut converted = resample(buf, format.sample_rate)?.remix(format.channels);
    converted.sanitize();
    Ok(converted)
}

/// How long the actor waits for a command before ticking the wall clock.
const WALL_CLOCK_TICK: Duration = Duration::from_millis(5);
/// How many events the actor keeps for a slow client before dropping new ones.
const EVENT_QUEUE: usize = 1024;

/// Handle to the playback thread. Dropping it stops playback and joins the thread.
#[derive(Debug)]
pub struct PlaybackActor {
    commands: Option<Sender<PlaybackCommand>>,
    events: Receiver<PlaybackEvent>,
    thread: Option<JoinHandle<()>>,
    format: OutputFormat,
    clock_source: ClockSource,
}

impl PlaybackActor {
    /// Starts an actor on the default output device (wall clock when there is none).
    ///
    /// # Errors
    /// [`AudioError::ActorStopped`] when the thread cannot be started.
    #[cfg(feature = "device")]
    pub fn spawn() -> Result<Self> {
        Self::spawn_with(crate::device::CpalOutput::open_default)
    }

    /// Starts an actor whose output is created by `make` on the actor thread. When `make` or the backend's start
    /// fails, the actor runs on the wall clock.
    ///
    /// # Errors
    /// [`AudioError::ActorStopped`] when the thread cannot be started or dies before reporting.
    pub fn spawn_with<B, F>(make: F) -> Result<Self>
    where
        B: OutputBackend,
        F: FnOnce() -> Result<B> + Send + 'static,
    {
        let (cmd_tx, cmd_rx) = unbounded::<PlaybackCommand>();
        let (evt_tx, evt_rx) = bounded::<PlaybackEvent>(EVENT_QUEUE);
        let thread = std::thread::Builder::new()
            .name("anim-audio-out".into())
            .spawn(move || actor_main(make, &cmd_rx, &evt_tx))
            .map_err(|_| AudioError::ActorStopped)?;
        // The first event says what the actor became.
        let first = evt_rx.recv_timeout(Duration::from_secs(10)).map_err(|_| AudioError::ActorStopped)?;
        let PlaybackEvent::Ready { format, clock_source } = first else {
            return Err(AudioError::ActorStopped);
        };
        Ok(Self { commands: Some(cmd_tx), events: evt_rx, thread: Some(thread), format, clock_source })
    }

    /// The output format sounds are converted to.
    #[must_use]
    pub const fn format(&self) -> OutputFormat {
        self.format
    }

    /// Whether the playhead follows a device or the wall clock.
    #[must_use]
    pub const fn clock_source(&self) -> ClockSource {
        self.clock_source
    }

    /// The events the actor reports.
    #[must_use]
    pub const fn events(&self) -> &Receiver<PlaybackEvent> {
        &self.events
    }

    /// Sends a command.
    ///
    /// # Errors
    /// [`AudioError::ActorStopped`] when the actor thread has exited.
    pub fn send(&self, command: PlaybackCommand) -> Result<()> {
        self.commands.as_ref().ok_or(AudioError::ActorStopped)?.send(command).map_err(|_| AudioError::ActorStopped)
    }

    /// Takes every pending event and returns the newest clock among them (`None` when no clock event was pending),
    /// together with the other events in order.
    #[must_use]
    pub fn poll(&self) -> (Option<PlaybackClock>, Vec<PlaybackEvent>) {
        let mut clock = None;
        let mut others = Vec::new();
        for event in self.events.try_iter() {
            match event {
                PlaybackEvent::Clock(c) => clock = Some(c),
                other => others.push(other),
            }
        }
        (clock, others)
    }
}

impl Drop for PlaybackActor {
    fn drop(&mut self) {
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn actor_main<B, F>(make: F, commands: &Receiver<PlaybackCommand>, events: &Sender<PlaybackEvent>)
where
    B: OutputBackend,
    F: FnOnce() -> Result<B>,
{
    let (render_tx, render_rx) = unbounded::<PlaybackCommand>();
    let mut report_error = None;
    let backend = match make() {
        Ok(b) if b.format().is_valid() => Some(b),
        Ok(_) => {
            report_error = Some("the output device reported an unusable format".to_string());
            None
        }
        Err(e) => {
            report_error = Some(e.to_string());
            None
        }
    };
    let device_format = backend.as_ref().map(OutputBackend::format);
    let format = device_format.unwrap_or(OutputFormat::FALLBACK);

    // Start the device. If that fails the same command queue feeds a fallback renderer.
    let mut stream = None;
    if let Some(b) = backend {
        match b.start(Renderer::new(format, render_rx.clone(), events.clone())) {
            Ok(s) => stream = Some(s),
            Err(e) => report_error = Some(e.to_string()),
        }
    }
    let mut wall = if stream.is_none() { Some((Renderer::new(format, render_rx, events.clone()), Instant::now(), 0.0_f64)) } else { None };
    let clock_source = if wall.is_some() { ClockSource::WallClock } else { ClockSource::Device };
    if events.send(PlaybackEvent::Ready { format, clock_source }).is_err() {
        return;
    }
    if let Some(why) = report_error {
        let _ = events.try_send(PlaybackEvent::Error(format!("no audio output, using a silent wall clock: {why}")));
    }

    loop {
        let timeout = if wall.is_some() { WALL_CLOCK_TICK } else { Duration::from_millis(250) };
        match commands.recv_timeout(timeout) {
            Ok(cmd) => forward(cmd, format, &render_tx, events),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
        if let Some((renderer, last, remainder)) = &mut wall {
            let now = Instant::now();
            let due = now.duration_since(*last).as_secs_f64().mul_add(f64::from(format.sample_rate), *remainder);
            let whole = due.floor();
            *remainder = due - whole;
            *last = now;
            renderer.advance_silent(usize::try_from(anim_core::num::round_f64_to_i64(whole).max(0)).unwrap_or(0));
        }
    }
    drop(stream);
}

/// Prepares a command (converting sounds) and passes it to the renderer.
fn forward(cmd: PlaybackCommand, format: OutputFormat, to_renderer: &Sender<PlaybackCommand>, events: &Sender<PlaybackEvent>) {
    let cmd = match cmd {
        PlaybackCommand::Load(buf) => match prepare(&buf, format) {
            Ok(p) => PlaybackCommand::Load(p),
            Err(e) => {
                let _ = events.try_send(PlaybackEvent::Error(e.to_string()));
                return;
            }
        },
        other => other,
    };
    let _ = to_renderer.send(cmd);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::testutil::sine;

    const FMT: OutputFormat = OutputFormat { sample_rate: 1000, channels: 2 };

    fn rig() -> (Renderer, Sender<PlaybackCommand>, Receiver<PlaybackEvent>) {
        let (ctx, crx) = unbounded();
        let (etx, erx) = bounded(4096);
        (Renderer::new(FMT, crx, etx), ctx, erx)
    }

    fn ms(n: i64) -> Tick {
        Tick::from_units(n, 1000)
    }

    fn ramp_stereo(frames: usize) -> PcmBuffer {
        PcmBuffer::new(1000, 2, (0..frames).flat_map(|i| [i as f32 / 1000.0, -(i as f32) / 1000.0]).collect())
    }

    fn last_clock(rx: &Receiver<PlaybackEvent>) -> Option<PlaybackClock> {
        rx.try_iter().filter_map(|e| if let PlaybackEvent::Clock(c) = e { Some(c) } else { None }).last()
    }

    #[test]
    fn silent_until_played_then_plays_in_order() {
        let (mut r, tx, rx) = rig();
        let mut out = vec![1.0_f32; 20];
        tx.send(PlaybackCommand::Load(ramp_stereo(100))).unwrap();
        r.render(&mut out, 0);
        assert!(out.iter().all(|s| *s == 0.0));
        assert!(matches!(rx.try_recv(), Ok(PlaybackEvent::Loaded { .. })));
        tx.send(PlaybackCommand::Play { from: ms(10) }).unwrap();
        r.render(&mut out, 0);
        assert_eq!(out[0], 0.010);
        assert_eq!(out[1], -0.010);
        assert_eq!(out[18], 0.019);
        let c = last_clock(&rx).unwrap();
        assert!(c.playing);
        assert_eq!(c.position, ms(20));
    }

    #[test]
    fn clock_is_behind_by_the_latency() {
        let (mut r, tx, rx) = rig();
        tx.send(PlaybackCommand::Play { from: Tick::ZERO }).unwrap();
        let mut out = vec![0.0_f32; 200];
        r.render(&mut out, 30);
        assert_eq!(last_clock(&rx).unwrap().position, ms(70));
        r.render(&mut out, 30);
        assert_eq!(last_clock(&rx).unwrap().position, ms(170));
    }

    #[test]
    fn pause_resume_seek_stop() {
        let (mut r, tx, rx) = rig();
        let mut out = vec![0.0_f32; 20];
        tx.send(PlaybackCommand::Load(ramp_stereo(100))).unwrap();
        tx.send(PlaybackCommand::Play { from: Tick::ZERO }).unwrap();
        r.render(&mut out, 0);
        tx.send(PlaybackCommand::Pause).unwrap();
        r.render(&mut out, 0);
        assert!(out.iter().all(|s| *s == 0.0));
        let c = last_clock(&rx).unwrap();
        assert!(!c.playing);
        assert_eq!(c.position, ms(10));
        // Paused: no further events, the position holds.
        r.render(&mut out, 0);
        assert!(last_clock(&rx).is_none());
        tx.send(PlaybackCommand::Seek(ms(50))).unwrap();
        r.render(&mut out, 0);
        assert_eq!(last_clock(&rx).unwrap().position, ms(50));
        tx.send(PlaybackCommand::Resume).unwrap();
        r.render(&mut out, 0);
        assert_eq!(out[0], 0.050);
        tx.send(PlaybackCommand::Stop).unwrap();
        r.render(&mut out, 0);
        let c = last_clock(&rx).unwrap();
        assert_eq!((c.playing, c.position), (false, Tick::ZERO));
    }

    #[test]
    fn finishes_once_and_the_clock_keeps_running() {
        let (mut r, tx, rx) = rig();
        let mut out = vec![0.0_f32; 80];
        tx.send(PlaybackCommand::Load(ramp_stereo(30))).unwrap();
        tx.send(PlaybackCommand::Play { from: Tick::ZERO }).unwrap();
        r.render(&mut out, 0);
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(events.iter().filter(|e| **e == PlaybackEvent::Finished).count(), 1);
        assert_eq!(out[58], 0.029);
        assert_eq!(out[60], 0.0, "silence after the end");
        r.render(&mut out, 0);
        let events: Vec<_> = rx.try_iter().collect();
        assert!(!events.contains(&PlaybackEvent::Finished));
        let c = events.iter().rev().find_map(|e| if let PlaybackEvent::Clock(c) = e { Some(*c) } else { None });
        assert_eq!(c.map(|c| (c.playing, c.position)), Some((true, ms(80))));
        tx.send(PlaybackCommand::Seek(Tick::ZERO)).unwrap();
        r.render(&mut out, 0);
        assert!(rx.try_iter().any(|e| e == PlaybackEvent::Finished));
    }

    #[test]
    fn loops_inside_the_range() {
        let (mut r, tx, rx) = rig();
        let mut out = vec![0.0_f32; 2 * 25];
        tx.send(PlaybackCommand::Load(ramp_stereo(100))).unwrap();
        tx.send(PlaybackCommand::SetLoop(Some(TimeRange::new(ms(10), ms(10))))).unwrap();
        tx.send(PlaybackCommand::Play { from: ms(10) }).unwrap();
        r.render(&mut out, 0);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        // 10..20 then wraps to 10.. again.
        assert_eq!(left[0], 0.010);
        assert_eq!(left[9], 0.019);
        assert_eq!(left[10], 0.010);
        assert_eq!(left[24], 0.014);
        assert!(!rx.try_iter().any(|e| e == PlaybackEvent::Finished));
        // Seeking far past the loop end lands inside the loop.
        tx.send(PlaybackCommand::Seek(ms(95))).unwrap();
        r.render(&mut out, 0);
        assert!(out[0] >= 0.010 && out[0] < 0.020, "{}", out[0]);
        // A reversed or empty range is ignored.
        tx.send(PlaybackCommand::SetLoop(Some(TimeRange::new(ms(20), ms(0))))).unwrap();
        r.render(&mut out, 0);
    }

    #[test]
    fn volume_is_clamped_and_ramped() {
        let (mut r, tx, _rx) = rig();
        tx.send(PlaybackCommand::Load(PcmBuffer::new(1000, 2, vec![1.0; 400]))).unwrap();
        tx.send(PlaybackCommand::Play { from: Tick::ZERO }).unwrap();
        tx.send(PlaybackCommand::SetVolume(0.5)).unwrap();
        let mut out = vec![0.0_f32; 40];
        r.render(&mut out, 0);
        assert!(out[0] > 0.5 && out[0] < 1.0, "ramp starts near the old gain: {}", out[0]);
        assert!((out[38] - 0.5).abs() < 1e-5);
        r.render(&mut out, 0);
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-5));
        tx.send(PlaybackCommand::SetVolume(100.0)).unwrap();
        r.render(&mut out, 0);
        r.render(&mut out, 0);
        assert!((out[0] - 4.0).abs() < 1e-5);
        tx.send(PlaybackCommand::SetVolume(f32::NAN)).unwrap();
        r.render(&mut out, 0);
        r.render(&mut out, 0);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn load_converts_rate_and_channels() {
        let (mut r, tx, rx) = rig();
        let mono = sine(2000, 1, 2000, 50.0, 0.5);
        tx.send(PlaybackCommand::Load(mono)).unwrap();
        r.render(&mut [0.0; 2], 0);
        let loaded = rx.try_iter().find_map(|e| if let PlaybackEvent::Loaded { duration } = e { Some(duration) } else { None });
        assert_eq!(loaded, Some(Tick::from_units(1, 1)));
        tx.send(PlaybackCommand::Play { from: Tick::ZERO }).unwrap();
        let mut out = vec![0.0_f32; 2 * 500];
        r.render(&mut out, 0);
        // Both channels carry the (resampled) mono signal.
        assert!(out.chunks(2).skip(10).take(400).all(|f| (f[0] - f[1]).abs() < 1e-6));
        assert!(out.iter().any(|s| s.abs() > 0.4));
    }

    #[test]
    fn unloading_and_playing_without_a_sound_still_runs_the_clock() {
        let (mut r, tx, rx) = rig();
        tx.send(PlaybackCommand::Play { from: ms(5) }).unwrap();
        let mut out = vec![0.0_f32; 20];
        r.render(&mut out, 0);
        assert_eq!(last_clock(&rx).unwrap().position, ms(15));
        tx.send(PlaybackCommand::Load(PcmBuffer::new(44_100, 2, vec![]))).unwrap();
        r.render(&mut out, 0);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn invalid_sounds_report_an_error_and_keep_the_old_one() {
        let (mut r, tx, rx) = rig();
        tx.send(PlaybackCommand::Load(ramp_stereo(10))).unwrap();
        tx.send(PlaybackCommand::Load(PcmBuffer::new(0, 2, vec![0.0; 4]))).unwrap();
        r.render(&mut [0.0; 2], 0);
        let events: Vec<_> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, PlaybackEvent::Error(_))));
        tx.send(PlaybackCommand::Play { from: ms(1) }).unwrap();
        let mut out = vec![0.0_f32; 4];
        r.render(&mut out, 0);
        assert_eq!(out[0], 0.001);
    }

    #[test]
    fn huge_seek_does_not_panic() {
        let (mut r, tx, _rx) = rig();
        tx.send(PlaybackCommand::Load(ramp_stereo(10))).unwrap();
        tx.send(PlaybackCommand::Play { from: Tick(i64::MAX) }).unwrap();
        tx.send(PlaybackCommand::SetLoop(Some(TimeRange::new(Tick(i64::MIN), Tick(i64::MAX))))).unwrap();
        r.render(&mut [0.0; 64], usize::MAX);
        tx.send(PlaybackCommand::Seek(Tick(i64::MIN))).unwrap();
        r.render(&mut [0.0; 64], 7);
    }

    #[test]
    fn advance_silent_moves_the_clock() {
        let (mut r, tx, rx) = rig();
        tx.send(PlaybackCommand::Play { from: Tick::ZERO }).unwrap();
        r.advance_silent(250);
        assert_eq!(last_clock(&rx).unwrap().position, ms(250));
    }

    /// A backend that hands its renderer to the test instead of opening a device.
    struct Fake {
        format: OutputFormat,
        out: Sender<Renderer>,
    }

    impl OutputBackend for Fake {
        type Stream = ();

        fn format(&self) -> OutputFormat {
            self.format
        }

        fn start(self, renderer: Renderer) -> Result<()> {
            self.out.send(renderer).map_err(|_| AudioError::ActorStopped)
        }
    }

    fn wait_for<T>(rx: &Receiver<T>, mut pick: impl FnMut(&T) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(20))
                && pick(&e)
            {
                return true;
            }
        }
        false
    }

    #[test]
    fn actor_protocol_with_a_fake_device() {
        let (rtx, rrx) = unbounded();
        let actor = PlaybackActor::spawn_with(move || Ok(Fake { format: FMT, out: rtx })).unwrap();
        assert_eq!(actor.format(), FMT);
        assert_eq!(actor.clock_source(), ClockSource::Device);
        let mut renderer = rrx.recv_timeout(Duration::from_secs(5)).unwrap();

        // 4 kHz mono: the actor resamples to the device's 1 kHz stereo off the audio thread.
        actor.send(PlaybackCommand::Load(PcmBuffer::new(4000, 1, vec![0.25; 4000]))).unwrap();
        actor.send(PlaybackCommand::Play { from: Tick::ZERO }).unwrap();
        let mut out = vec![0.0_f32; 2 * 100];
        let mut loaded = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut heard = false;
        while Instant::now() < deadline && !(loaded && heard) {
            renderer.render(&mut out, 10);
            heard |= out.iter().skip(40).any(|s| (s - 0.25).abs() < 0.01);
            let (_, others) = actor.poll();
            loaded |= others.iter().any(|e| matches!(e, PlaybackEvent::Loaded { duration } if *duration == Tick::from_units(1, 1)));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(loaded && heard);
        let (clock, _) = actor.poll();
        assert!(clock.is_none() || clock.unwrap().playing);

        // Dropping the handle stops the thread even while a device callback is alive.
        drop(actor);
        renderer.render(&mut out, 0);
    }

    #[test]
    fn falls_back_to_a_wall_clock_without_a_device() {
        struct Never;
        impl OutputBackend for Never {
            type Stream = ();

            fn format(&self) -> OutputFormat {
                FMT
            }

            fn start(self, _: Renderer) -> Result<()> {
                Err(AudioError::Device("no device".into()))
            }
        }
        let actor = PlaybackActor::spawn_with(|| Ok(Never)).unwrap();
        assert_eq!(actor.clock_source(), ClockSource::WallClock);
        actor.send(PlaybackCommand::Play { from: ms(100) }).unwrap();
        let mut last = Tick::ZERO;
        assert!(wait_for(actor.events(), |e| {
            if let PlaybackEvent::Clock(c) = e {
                last = c.position;
                c.playing && c.position > ms(160)
            } else {
                false
            }
        }));
        assert!(last > ms(160));
        actor.send(PlaybackCommand::Pause).unwrap();
        assert!(wait_for(actor.events(), |e| matches!(e, PlaybackEvent::Clock(c) if !c.playing)));
    }

    #[test]
    fn failing_factory_also_falls_back() {
        struct Never;
        impl OutputBackend for Never {
            type Stream = ();

            fn format(&self) -> OutputFormat {
                FMT
            }

            fn start(self, _: Renderer) -> Result<()> {
                Ok(())
            }
        }
        let actor = PlaybackActor::spawn_with(|| -> Result<Never> { Err(AudioError::Device("none".into())) }).unwrap();
        assert_eq!(actor.clock_source(), ClockSource::WallClock);
        assert_eq!(actor.format(), OutputFormat::FALLBACK);
        assert!(wait_for(actor.events(), |e| matches!(e, PlaybackEvent::Error(_))));
    }
}
