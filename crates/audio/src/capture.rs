//! The microphone actor: opens the input device on `Start`, delivers [`PcmBuffer`] chunks over a channel, closes it on
//! `Stop`. The device is abstracted by [`InputBackend`], so the protocol is tested with a fake.

use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError, bounded, unbounded};

use crate::buffer::{MAX_CHANNELS, MAX_SAMPLE_RATE, PcmBuffer};
use crate::error::{AudioError, Result};

/// Rate and channel count of the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputFormat {
    /// Frames per second.
    pub sample_rate: u32,
    /// Interleaved channels per frame.
    pub channels: u16,
}

impl InputFormat {
    const fn is_valid(self) -> bool {
        self.sample_rate > 0 && self.sample_rate <= MAX_SAMPLE_RATE && self.channels > 0 && self.channels <= MAX_CHANNELS
    }
}

/// What a client can ask the capture actor to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureCommand {
    /// Opens the input device and starts delivering chunks. Ignored while capturing.
    Start,
    /// Closes the device. Chunks already delivered stay in the event queue.
    Stop,
}

/// What the capture actor reports.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureEvent {
    /// Capturing has begun in this format; every following chunk has it. A device that then fails to start is
    /// reported by [`CaptureEvent::Error`] followed by [`CaptureEvent::Stopped`].
    Started(InputFormat),
    /// The next piece of audio, in capture order.
    Chunk(PcmBuffer),
    /// The client fell behind and the queue was full: `dropped` chunks before the next one were lost.
    Overrun {
        /// How many chunks were discarded.
        dropped: u32,
    },
    /// Capturing has ended.
    Stopped,
    /// No device, a refused configuration, or a stream that failed.
    Error(String),
}

/// Where a backend's device callback delivers audio.
#[derive(Debug)]
pub struct ChunkSink {
    format: InputFormat,
    events: Sender<CaptureEvent>,
    dropped: u32,
}

impl ChunkSink {
    /// The format chunks are delivered in.
    #[must_use]
    pub const fn format(&self) -> InputFormat {
        self.format
    }

    /// Delivers interleaved samples. Never blocks: a full queue drops the chunk and counts it.
    pub fn push(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        if self.dropped > 0 && self.events.try_send(CaptureEvent::Overrun { dropped: self.dropped }).is_ok() {
            self.dropped = 0;
        }
        let mut chunk = PcmBuffer::new(self.format.sample_rate, self.format.channels, samples.to_vec());
        chunk.sanitize();
        match self.events.try_send(CaptureEvent::Chunk(chunk)) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => self.dropped = self.dropped.saturating_add(1),
        }
    }

    /// A handle for reporting stream failures from the device's error callback.
    #[must_use]
    pub fn reporter(&self) -> ErrorReporter {
        ErrorReporter { events: self.events.clone() }
    }
}

/// Reports device failures to the capture client.
#[derive(Clone, Debug)]
pub struct ErrorReporter {
    events: Sender<CaptureEvent>,
}

impl ErrorReporter {
    /// Sends [`CaptureEvent::Error`].
    pub fn report(&self, message: String) {
        let _ = self.events.try_send(CaptureEvent::Error(message));
    }
}

/// An audio input. Created on the actor thread, so it need not be `Send`.
pub trait InputBackend: Sized {
    /// Whatever must stay alive while capturing (a cpal `Stream`). Dropping it closes the device.
    type Stream;

    /// The format the device records at.
    fn format(&self) -> InputFormat;

    /// Starts the device; its callback feeds `sink`.
    ///
    /// # Errors
    /// [`AudioError::Device`] when the device refuses to start.
    fn start(self, sink: ChunkSink) -> Result<Self::Stream>;
}

/// Chunks the event queue holds before new ones are dropped (about 40 s at 10 ms per chunk).
const EVENT_QUEUE: usize = 4096;

/// Handle to the capture thread. Dropping it closes the device and joins the thread.
#[derive(Debug)]
pub struct CaptureActor {
    commands: Option<Sender<CaptureCommand>>,
    events: Receiver<CaptureEvent>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureActor {
    /// Starts an actor on the default input device.
    ///
    /// # Errors
    /// [`AudioError::ActorStopped`] when the thread cannot be started.
    #[cfg(feature = "device")]
    pub fn spawn() -> Result<Self> {
        Self::spawn_with(crate::device::CpalInput::open_default)
    }

    /// Starts an actor that calls `make` (on the actor thread) every time capturing starts.
    ///
    /// # Errors
    /// [`AudioError::ActorStopped`] when the thread cannot be started.
    pub fn spawn_with<B, F>(make: F) -> Result<Self>
    where
        B: InputBackend,
        F: FnMut() -> Result<B> + Send + 'static,
    {
        let (cmd_tx, cmd_rx) = unbounded::<CaptureCommand>();
        let (evt_tx, evt_rx) = bounded::<CaptureEvent>(EVENT_QUEUE);
        let thread =
            std::thread::Builder::new().name("anim-audio-in".into()).spawn(move || actor_main(make, &cmd_rx, &evt_tx)).map_err(|_| AudioError::ActorStopped)?;
        Ok(Self { commands: Some(cmd_tx), events: evt_rx, thread: Some(thread) })
    }

    /// The events the actor reports.
    #[must_use]
    pub const fn events(&self) -> &Receiver<CaptureEvent> {
        &self.events
    }

    /// Sends a command.
    ///
    /// # Errors
    /// [`AudioError::ActorStopped`] when the actor thread has exited.
    pub fn send(&self, command: CaptureCommand) -> Result<()> {
        self.commands.as_ref().ok_or(AudioError::ActorStopped)?.send(command).map_err(|_| AudioError::ActorStopped)
    }
}

impl Drop for CaptureActor {
    fn drop(&mut self) {
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn actor_main<B, F>(mut make: F, commands: &Receiver<CaptureCommand>, events: &Sender<CaptureEvent>)
where
    B: InputBackend,
    F: FnMut() -> Result<B>,
{
    let mut stream: Option<B::Stream> = None;
    loop {
        match commands.recv_timeout(Duration::from_millis(500)) {
            Ok(CaptureCommand::Start) if stream.is_none() => match make() {
                Ok(backend) if backend.format().is_valid() => {
                    let format = backend.format();
                    let sink = ChunkSink { format, events: events.clone(), dropped: 0 };
                    // Announced first so no chunk can overtake it; a failed start is reported as an error plus Stopped.
                    let _ = events.try_send(CaptureEvent::Started(format));
                    match backend.start(sink) {
                        Ok(s) => stream = Some(s),
                        Err(e) => {
                            let _ = events.try_send(CaptureEvent::Error(e.to_string()));
                            let _ = events.try_send(CaptureEvent::Stopped);
                        }
                    }
                }
                Ok(_) => {
                    let _ = events.try_send(CaptureEvent::Error("the input device reported an unusable format".into()));
                }
                Err(e) => {
                    let _ = events.try_send(CaptureEvent::Error(e.to_string()));
                }
            },
            Ok(CaptureCommand::Start) | Err(RecvTimeoutError::Timeout) => {}
            Ok(CaptureCommand::Stop) => {
                if stream.take().is_some() {
                    let _ = events.try_send(CaptureEvent::Stopped);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stream);
}

/// Collects chunks into one buffer, up to a size cap.
#[derive(Debug)]
pub struct Recorder {
    buffer: Option<PcmBuffer>,
    max_samples: usize,
    truncated: bool,
}

impl Recorder {
    /// A recorder that keeps at most `max_samples` samples (all channels).
    #[must_use]
    pub const fn new(max_samples: usize) -> Self {
        Self { buffer: None, max_samples, truncated: false }
    }

    /// Adds a chunk. A chunk whose format differs from the first one, or that would exceed the cap, is dropped and
    /// [`Recorder::truncated`] turns true.
    pub fn push(&mut self, chunk: &PcmBuffer) {
        let buf = self.buffer.get_or_insert_with(|| PcmBuffer::new(chunk.sample_rate, chunk.channels, Vec::new()));
        if buf.samples.len().saturating_add(chunk.samples.len()) > self.max_samples || !buf.append(chunk) {
            self.truncated = true;
        }
    }

    /// Feeds the chunk of a [`CaptureEvent::Chunk`]; other events are ignored.
    pub fn feed(&mut self, event: &CaptureEvent) {
        if let CaptureEvent::Chunk(chunk) = event {
            self.push(chunk);
        }
    }

    /// True when audio was dropped because of the cap or a format change.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// The recording so far, or `None` before the first chunk.
    #[must_use]
    pub fn finish(self) -> Option<PcmBuffer> {
        self.buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FMT: InputFormat = InputFormat { sample_rate: 16_000, channels: 1 };

    /// Pushes `chunks` chunks of 160 samples when started.
    struct Fake {
        chunks: usize,
    }

    impl InputBackend for Fake {
        type Stream = ();

        fn format(&self) -> InputFormat {
            FMT
        }

        fn start(self, mut sink: ChunkSink) -> Result<()> {
            for i in 0..self.chunks {
                sink.push(&vec![crate::buffer::usize_to_f32(i) * 0.25; 160]);
            }
            Ok(())
        }
    }

    fn next(actor: &CaptureActor) -> CaptureEvent {
        actor.events().recv_timeout(Duration::from_secs(5)).unwrap()
    }

    #[test]
    fn start_delivers_chunks_in_order_then_stop() {
        let actor = CaptureActor::spawn_with(|| Ok(Fake { chunks: 3 })).unwrap();
        actor.send(CaptureCommand::Start).unwrap();
        assert_eq!(next(&actor), CaptureEvent::Started(FMT));
        let mut rec = Recorder::new(10_000);
        for expect in 0..3 {
            let e = next(&actor);
            let CaptureEvent::Chunk(c) = &e else { panic!("{e:?}") };
            assert_eq!((c.sample_rate, c.channels, c.frames()), (16_000, 1, 160));
            assert_eq!(c.samples[0], expect as f32 * 0.25);
            rec.feed(&e);
        }
        actor.send(CaptureCommand::Stop).unwrap();
        assert_eq!(next(&actor), CaptureEvent::Stopped);
        let got = rec.finish().unwrap();
        assert_eq!(got.frames(), 480);
        // A second stop is silent; start works again.
        actor.send(CaptureCommand::Stop).unwrap();
        actor.send(CaptureCommand::Start).unwrap();
        assert_eq!(next(&actor), CaptureEvent::Started(FMT));
    }

    #[test]
    fn a_missing_device_is_an_error_event() {
        let actor = CaptureActor::spawn_with(|| -> Result<Fake> { Err(AudioError::Device("no microphone".into())) }).unwrap();
        actor.send(CaptureCommand::Start).unwrap();
        assert!(matches!(next(&actor), CaptureEvent::Error(m) if m.contains("no microphone")));
    }

    #[test]
    fn a_full_queue_counts_overruns() {
        let (tx, rx) = bounded(2);
        let mut sink = ChunkSink { format: FMT, events: tx, dropped: 0 };
        for _ in 0..5 {
            sink.push(&[0.1; 4]);
        }
        assert_eq!(sink.dropped, 3);
        assert_eq!(rx.try_iter().count(), 2);
        sink.push(&[0.1; 4]);
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(events, vec![CaptureEvent::Overrun { dropped: 3 }, CaptureEvent::Chunk(PcmBuffer::new(16_000, 1, vec![0.1; 4]))]);
    }

    #[test]
    fn recorder_caps_and_rejects_format_changes() {
        let mut rec = Recorder::new(10);
        rec.push(&PcmBuffer::new(8000, 1, vec![0.5; 6]));
        assert!(!rec.truncated());
        rec.push(&PcmBuffer::new(8000, 1, vec![0.5; 6]));
        assert!(rec.truncated());
        rec.push(&PcmBuffer::new(44_100, 2, vec![0.5; 2]));
        assert_eq!(rec.finish().unwrap().samples.len(), 6);
        assert!(Recorder::new(10).finish().is_none());
    }

    #[test]
    fn non_finite_input_is_sanitised() {
        let (tx, rx) = bounded(2);
        let mut sink = ChunkSink { format: FMT, events: tx, dropped: 0 };
        sink.push(&[f32::NAN, 2.0]);
        let CaptureEvent::Chunk(c) = rx.recv().unwrap() else { panic!() };
        assert_eq!(c.samples, vec![0.0, 1.0]);
    }
}
