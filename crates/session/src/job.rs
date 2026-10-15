//! Background jobs as actors.
//!
//! A job is a closure running on its own thread. It owns everything it needs, reports progress over a channel and
//! polls a cancel channel; the UI owns a [`JobHandle`] and calls [`JobHandle::poll`] once per frame. A panic inside
//! the closure is caught and delivered as [`JobEvent::Failed`], never propagated. Dropping a handle cancels the job.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, unbounded};

/// What a job reports to its handle.
#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent<T> {
    /// Progress update; `fraction` is within `0.0..=1.0`.
    Progress {
        /// Completed fraction.
        fraction: f32,
        /// Short status text.
        message: String,
    },
    /// The job finished with a value (terminal).
    Finished(T),
    /// The job failed or panicked (terminal).
    Failed(String),
    /// The job stopped because cancellation was requested (terminal).
    Cancelled,
}

impl<T> JobEvent<T> {
    /// Whether no further events follow.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        !matches!(self, Self::Progress { .. })
    }
}

/// Why a job closure stopped without a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
    /// Cancellation was observed.
    #[error("cancelled")]
    Cancelled,
    /// The job failed with this message.
    #[error("{0}")]
    Failed(String),
}

impl JobError {
    /// Shorthand for [`JobError::Failed`].
    #[must_use]
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}

/// The job's side of the channels, passed to the closure.
pub struct JobContext<T> {
    events: Sender<JobEvent<T>>,
    cancel: Receiver<()>,
    cancelled: bool,
}

impl<T> JobContext<T> {
    /// Reports progress. `fraction` is clamped to `0.0..=1.0` (NaN counts as 0).
    pub fn progress(&self, fraction: f32, message: impl Into<String>) {
        let fraction = if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) };
        // A closed channel means the handle is gone; the next cancellation check will notice.
        let _ = self.events.send(JobEvent::Progress { fraction, message: message.into() });
    }

    /// Whether cancellation was requested (or the handle was dropped). Once true it stays true.
    pub fn is_cancelled(&mut self) -> bool {
        if !self.cancelled {
            self.cancelled = match self.cancel.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => true,
                Err(TryRecvError::Empty) => false,
            };
        }
        self.cancelled
    }

    /// `Err(JobError::Cancelled)` once cancellation was requested, for use with `?` in loops.
    ///
    /// # Errors
    /// [`JobError::Cancelled`] after a cancel request.
    pub fn check_cancelled(&mut self) -> Result<(), JobError> {
        if self.is_cancelled() { Err(JobError::Cancelled) } else { Ok(()) }
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".to_owned())
}

/// The UI's side of a running job.
pub struct JobHandle<T> {
    name: String,
    events: Receiver<JobEvent<T>>,
    cancel: Sender<()>,
    finished: bool,
}

impl<T: Send + 'static> JobHandle<T> {
    /// Starts `work` on a new thread named `name`.
    pub fn spawn<F>(name: impl Into<String>, work: F) -> Self
    where
        F: FnOnce(&mut JobContext<T>) -> Result<T, JobError> + Send + 'static,
    {
        let name = name.into();
        let (event_tx, events) = unbounded();
        let (cancel, cancel_rx) = unbounded();
        let mut ctx = JobContext { events: event_tx.clone(), cancel: cancel_rx, cancelled: false };
        let spawned = thread::Builder::new().name(name.clone()).spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| work(&mut ctx)));
            let event = match outcome {
                Ok(Ok(value)) => JobEvent::Finished(value),
                Ok(Err(JobError::Cancelled)) => JobEvent::Cancelled,
                Ok(Err(JobError::Failed(message))) => JobEvent::Failed(message),
                Err(payload) => JobEvent::Failed(format!("job panicked: {}", panic_message(payload.as_ref()))),
            };
            let _ = event_tx.send(event);
        });
        if let Err(e) = spawned {
            let (tx, rx) = unbounded();
            let _ = tx.send(JobEvent::Failed(format!("could not start thread: {e}")));
            return Self { name, events: rx, cancel, finished: false };
        }
        Self { name, events, cancel, finished: false }
    }

    /// The job's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether a terminal event was already delivered by [`Self::poll`] or [`Self::wait`].
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// Asks the job to stop. It stops the next time it checks; a `Cancelled` event follows.
    pub fn cancel(&self) {
        let _ = self.cancel.send(());
    }

    /// Takes all events that arrived since the last call, without blocking.
    pub fn poll(&mut self) -> Vec<JobEvent<T>> {
        let mut out: Vec<JobEvent<T>> = Vec::new();
        if self.finished {
            return out;
        }
        loop {
            match self.events.try_recv() {
                Ok(event) => {
                    let terminal = event.is_terminal();
                    out.push(event);
                    if terminal {
                        self.finished = true;
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.finished = true;
                    out.push(JobEvent::Failed("worker ended without a result".to_owned()));
                    break;
                }
            }
        }
        out
    }

    /// Blocks until the job ends or `timeout` passes; returns the events received meanwhile.
    pub fn wait(&mut self, timeout: Duration) -> Vec<JobEvent<T>> {
        let deadline = Instant::now().checked_add(timeout);
        let mut out = self.poll();
        while !self.finished {
            let left = deadline.map_or(timeout, |d| d.saturating_duration_since(Instant::now()));
            if left.is_zero() {
                break;
            }
            match self.events.recv_timeout(left) {
                Ok(event) => {
                    self.finished = event.is_terminal();
                    out.push(event);
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    self.finished = true;
                    out.push(JobEvent::Failed("worker ended without a result".to_owned()));
                }
            }
        }
        out
    }
}

/// Identifies a job inside a [`JobSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// What the UI shows for a running job.
#[derive(Debug, Clone, PartialEq)]
pub struct JobStatus {
    /// The job.
    pub id: JobId,
    /// Its name.
    pub name: String,
    /// Latest progress fraction.
    pub fraction: f32,
    /// Latest progress message.
    pub message: String,
}

struct Tracked<T> {
    id: JobId,
    handle: JobHandle<T>,
    fraction: f32,
    message: String,
}

/// All jobs of one kind (exports, imports) owned by the UI, polled once per frame.
pub struct JobSet<T> {
    jobs: Vec<Tracked<T>>,
    next: u64,
}

impl<T> Default for JobSet<T> {
    fn default() -> Self {
        Self { jobs: Vec::new(), next: 1 }
    }
}

impl<T: Send + 'static> JobSet<T> {
    /// An empty set.
    #[must_use]
    pub const fn new() -> Self {
        Self { jobs: Vec::new(), next: 1 }
    }

    /// Starts a job and returns its id.
    pub fn spawn<F>(&mut self, name: impl Into<String>, work: F) -> JobId
    where
        F: FnOnce(&mut JobContext<T>) -> Result<T, JobError> + Send + 'static,
    {
        let id = JobId(self.next);
        self.next = self.next.saturating_add(1);
        self.jobs.push(Tracked { id, handle: JobHandle::spawn(name, work), fraction: 0.0, message: String::new() });
        id
    }

    /// Requests cancellation of one job.
    pub fn cancel(&self, id: JobId) {
        if let Some(job) = self.jobs.iter().find(|j| j.id == id) {
            job.handle.cancel();
        }
    }

    /// Requests cancellation of every job.
    pub fn cancel_all(&self) {
        self.jobs.iter().for_each(|j| j.handle.cancel());
    }

    /// Collects new events of all jobs and forgets the ones that ended.
    pub fn poll(&mut self) -> Vec<(JobId, JobEvent<T>)> {
        let mut out = Vec::new();
        for job in &mut self.jobs {
            for event in job.handle.poll() {
                if let JobEvent::Progress { fraction, message } = &event {
                    job.fraction = *fraction;
                    job.message.clone_from(message);
                }
                out.push((job.id, event));
            }
        }
        self.jobs.retain(|j| !j.handle.is_finished());
        out
    }

    /// Running jobs with their latest progress (as of the last [`Self::poll`]).
    #[must_use]
    pub fn running(&self) -> Vec<JobStatus> {
        self.jobs.iter().map(|j| JobStatus { id: j.id, name: j.handle.name().to_owned(), fraction: j.fraction, message: j.message.clone() }).collect()
    }

    /// Whether any job is running.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        !self.jobs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Duration = Duration::from_secs(10);

    #[test]
    fn finishes_with_value_and_progress() {
        let mut job = JobHandle::spawn("sum", |ctx| {
            ctx.progress(0.5, "half");
            ctx.progress(2.0, "clamped");
            ctx.progress(f32::NAN, "nan");
            Ok(42_u32)
        });
        let events = job.wait(LONG);
        assert_eq!(
            events,
            vec![
                JobEvent::Progress { fraction: 0.5, message: "half".into() },
                JobEvent::Progress { fraction: 1.0, message: "clamped".into() },
                JobEvent::Progress { fraction: 0.0, message: "nan".into() },
                JobEvent::Finished(42),
            ]
        );
        assert!(job.is_finished());
        assert!(job.poll().is_empty());
        assert_eq!(job.name(), "sum");
    }

    #[test]
    fn cancel_stops_a_loop() {
        let mut job = JobHandle::<u32>::spawn("spin", |ctx| {
            loop {
                ctx.check_cancelled()?;
                thread::sleep(Duration::from_millis(2));
            }
        });
        job.cancel();
        let events = job.wait(LONG);
        assert_eq!(events.last(), Some(&JobEvent::Cancelled));
    }

    #[test]
    fn cancellation_is_sticky() {
        let mut job = JobHandle::<bool>::spawn("sticky", |ctx| {
            while !ctx.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Ok(ctx.is_cancelled() && ctx.is_cancelled())
        });
        job.cancel();
        assert_eq!(job.wait(LONG).last(), Some(&JobEvent::Finished(true)));
    }

    #[test]
    fn panic_becomes_failure() {
        let mut job = JobHandle::<()>::spawn("boom", |_| panic!("kaput"));
        let events = job.wait(LONG);
        assert_eq!(events, vec![JobEvent::Failed("job panicked: kaput".to_owned())]);
        let mut job = JobHandle::<()>::spawn("boom2", |_| panic!("{}", String::from("formatted")));
        assert_eq!(job.wait(LONG), vec![JobEvent::Failed("job panicked: formatted".to_owned())]);
    }

    #[test]
    fn error_is_reported() {
        let mut job = JobHandle::<()>::spawn("bad", |_| Err(JobError::failed("disk full")));
        assert_eq!(job.wait(LONG), vec![JobEvent::Failed("disk full".into())]);
    }

    #[test]
    fn dropping_the_handle_cancels() {
        let (done_tx, done_rx) = unbounded();
        let job = JobHandle::<()>::spawn("orphan", move |ctx| {
            while !ctx.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            let _ = done_tx.send(());
            Err(JobError::Cancelled)
        });
        drop(job);
        assert!(done_rx.recv_timeout(LONG).is_ok());
    }

    #[test]
    fn wait_times_out_without_finishing() {
        let mut job = JobHandle::<()>::spawn("slow", |ctx| {
            while !ctx.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Err(JobError::Cancelled)
        });
        let events = job.wait(Duration::from_millis(20));
        assert!(events.is_empty() && !job.is_finished());
        job.cancel();
        assert_eq!(job.wait(LONG), vec![JobEvent::Cancelled]);
    }

    #[test]
    fn job_set_tracks_and_forgets() {
        let mut set = JobSet::<u8>::new();
        assert!(!set.is_busy());
        let (go_tx, go_rx) = unbounded::<()>();
        let a = set.spawn("a", move |ctx| {
            ctx.progress(0.25, "quarter");
            let _ = go_rx.recv_timeout(LONG);
            Ok(1)
        });
        let b = set.spawn("b", |ctx| {
            while !ctx.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Err(JobError::Cancelled)
        });
        assert_ne!(a, b);
        assert!(set.is_busy());
        set.cancel(b);
        let mut seen = Vec::new();
        let start = Instant::now();
        while start.elapsed() < LONG {
            seen.extend(set.poll());
            let progressed = set.running().iter().any(|s| s.id == a && (s.fraction - 0.25).abs() < f32::EPSILON);
            if progressed && seen.iter().any(|(id, e)| *id == b && *e == JobEvent::Cancelled) {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        let running = set.running();
        assert_eq!(running.len(), 1);
        assert_eq!(running.first().map(|s| s.message.as_str()), Some("quarter"));
        go_tx.send(()).unwrap();
        while set.is_busy() && start.elapsed() < LONG {
            seen.extend(set.poll());
            thread::sleep(Duration::from_millis(2));
        }
        assert!(seen.contains(&(a, JobEvent::Finished(1))));
        set.cancel_all();
    }
}
