//! The headless backend: an actor thread that owns a [`ControlHost`] (usually a [`crate::SessionHost`]) and answers the
//! requests sent through its [`Backend`].

use std::io;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{RecvTimeoutError, unbounded};

use crate::call::{Backend, TICK};
use crate::host::ControlHost;

/// A running headless session.
#[derive(Debug)]
pub struct HeadlessSession {
    backend: Backend,
    thread: JoinHandle<()>,
}

impl HeadlessSession {
    /// The handle to send requests through; clone it freely.
    #[must_use]
    pub const fn backend(&self) -> &Backend {
        &self.backend
    }

    /// Stops the actor once every [`Backend`] clone is gone, and waits for it. The host is dropped on its own thread.
    pub fn shutdown(self) {
        let Self { backend, thread } = self;
        drop(backend);
        let _ = thread.join();
    }
}

/// Starts the actor. `make_host` runs on the new thread, so the host (and the document) need not be `Send`.
///
/// # Errors
/// Fails when the thread cannot be spawned.
pub fn spawn_headless<F, H>(make_host: F) -> io::Result<HeadlessSession>
where
    F: FnOnce() -> H + Send + 'static,
    H: ControlHost,
{
    let (tx, rx) = unbounded();
    let thread = thread::Builder::new().name("anim-mcp-session".into()).spawn(move || {
        let mut host = make_host();
        loop {
            match rx.recv_timeout(TICK) {
                Ok(incoming) => host.handle(incoming),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            host.tick();
        }
    })?;
    Ok(HeadlessSession { backend: Backend::new(tx, "headless", "headless in-process session"), thread })
}
