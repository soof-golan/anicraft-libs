//! The plumbing of one call: a [`Backend`] hands a [`Request`] to whatever owns the session and gets events back.
//!
//! This is the actor boundary. The side that owns the document (a headless actor thread, or the app's UI thread
//! draining a [`crate::ControlInbox`]) receives an [`Incoming`] holding the request and a [`Ticket`]. The ticket is how
//! it answers: [`Ticket::progress`] any number of times, then [`Ticket::finish`] once, now or on a later frame. The
//! caller holds a [`PendingCall`] that yields those events and can cancel the call. Nothing is shared: events flow over
//! channels, and cancellation is the caller dropping its end of a channel.

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::protocol::{ControlError, ErrorKind, Event, Progress, Reply, Request};

/// The answering half of a call. Cheap to clone; keep a clone to finish the call on a later frame.
#[derive(Debug, Clone)]
pub struct Ticket {
    call: u64,
    events: UnboundedSender<(u64, Event)>,
    cancel: Receiver<()>,
}

impl Ticket {
    /// A ticket whose events carry `call` and flow into `events`; `cancel` reads as cancelled once its sender is gone.
    #[must_use]
    pub const fn new(call: u64, events: UnboundedSender<(u64, Event)>, cancel: Receiver<()>) -> Self {
        Self { call, events, cancel }
    }

    /// Identifier of the call on its connection (0 for in-process calls).
    #[must_use]
    pub const fn call_id(&self) -> u64 {
        self.call
    }

    /// Reports progress (`fraction` is clamped to 0..=1). Ignored once the caller has gone.
    pub fn progress(&self, fraction: f32, message: impl Into<String>) {
        let fraction = if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) };
        let _ = self.events.send((self.call, Event::Progress(Progress { fraction, message: message.into() })));
    }

    /// Whether the caller cancelled (or went away). Long operations poll this between steps.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self.cancel.try_recv(), Err(TryRecvError::Disconnected))
    }

    /// `Err(cancelled)` when the call was cancelled; for use with `?` inside long operations.
    ///
    /// # Errors
    /// [`ErrorKind::Cancelled`] after cancellation.
    pub fn check_cancelled(&self) -> Result<(), ControlError> {
        if self.is_cancelled() { Err(ControlError::cancelled()) } else { Ok(()) }
    }

    /// Delivers the final outcome. Only the first call matters; later ones and calls after the caller left are ignored.
    pub fn finish(&self, outcome: Result<Reply, ControlError>) {
        let _ = self.events.send((self.call, Event::Done(outcome)));
    }
}

/// A request on its way to the session owner.
#[derive(Debug)]
pub struct Incoming {
    /// What to do.
    pub request: Request,
    /// How to answer.
    pub ticket: Ticket,
    /// Who asked (`mcp`, `bridge 127.0.0.1:50000`, ...), recorded in the journal.
    pub origin: String,
}

/// The caller's half of a call.
#[derive(Debug)]
pub struct PendingCall {
    events: UnboundedReceiver<(u64, Event)>,
    cancel: Option<Sender<()>>,
    done: bool,
}

impl PendingCall {
    /// The next event, or `None` after the final one.
    pub async fn next(&mut self) -> Option<Event> {
        if self.done {
            return None;
        }
        let (_, event) = self.events.recv().await?;
        if matches!(event, Event::Done(_)) {
            self.done = true;
        }
        Some(event)
    }

    /// Cancels the call: the ticket reads as cancelled from now on. The final event still arrives.
    pub fn cancel(&mut self) {
        self.cancel = None;
    }

    /// Waits for the outcome, passing progress to `on_progress`.
    ///
    /// # Errors
    /// The call's own error, or [`ErrorKind::Unavailable`] when the session owner dropped the ticket unanswered.
    pub async fn finish(mut self, mut on_progress: impl FnMut(&Progress)) -> Result<Reply, ControlError> {
        while let Some(event) = self.next().await {
            match event {
                Event::Progress(p) => on_progress(&p),
                Event::Done(outcome) => return outcome,
            }
        }
        Err(ControlError::new(ErrorKind::Unavailable, "the editor dropped the request without answering"))
    }

    /// Blocking variant of [`Self::finish`] for threads outside the async runtime.
    ///
    /// # Errors
    /// As [`Self::finish`].
    pub fn wait(mut self, mut on_progress: impl FnMut(&Progress)) -> Result<Reply, ControlError> {
        while let Some((_, event)) = self.events.blocking_recv() {
            match event {
                Event::Progress(p) => on_progress(&p),
                Event::Done(outcome) => return outcome,
            }
        }
        Err(ControlError::new(ErrorKind::Unavailable, "the editor dropped the request without answering"))
    }
}

/// A handle to something that executes requests: a headless session actor or a connection to a running app.
///
/// Clones share nothing but the channel to the actor.
#[derive(Debug, Clone)]
pub struct Backend {
    tx: Sender<Incoming>,
    origin: String,
    description: String,
}

impl Backend {
    /// A backend feeding `tx`; whoever holds the receiving end answers. `description` says what it is
    /// (`headless`, `bridge 127.0.0.1:9877`).
    #[must_use]
    pub fn new(tx: Sender<Incoming>, origin: impl Into<String>, description: impl Into<String>) -> Self {
        Self { tx, origin: origin.into(), description: description.into() }
    }

    /// The same backend, labelling its calls `origin` in the journal.
    #[must_use]
    pub fn with_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = origin.into();
        self
    }

    /// What this backend is, for status output.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Sends a request. The call is already finished with an error when the actor is gone.
    #[must_use]
    pub fn submit(&self, request: Request) -> PendingCall {
        let (events_tx, events_rx) = unbounded_channel();
        let (cancel_tx, cancel_rx) = crossbeam_channel::bounded::<()>(1);
        let ticket = Ticket::new(0, events_tx.clone(), cancel_rx);
        let incoming = Incoming { request, ticket, origin: self.origin.clone() };
        if self.tx.send(incoming).is_err() {
            let _ = events_tx.send((0, Event::Done(Err(ControlError::new(ErrorKind::Unavailable, "the editor session has ended")))));
        }
        PendingCall { events: events_rx, cancel: Some(cancel_tx), done: false }
    }

    /// Runs a request to completion on the calling thread (outside any async runtime).
    ///
    /// # Errors
    /// The request's error.
    pub fn call(&self, request: Request) -> Result<Reply, ControlError> {
        self.submit(request).wait(|_| {})
    }

    /// Like [`Self::call`] but also collects the progress events.
    ///
    /// # Errors
    /// The request's error.
    pub fn call_with_progress(&self, request: Request, progress: &mut Vec<Progress>) -> Result<Reply, ControlError> {
        self.submit(request).wait(|p| progress.push(p.clone()))
    }
}

/// How long actor loops wait for work before they give the host a [`crate::ControlHost::tick`].
pub const TICK: Duration = Duration::from_millis(25);
