//! [`ControlServer`]: the control channel a running app embeds.
//!
//! The server listens on `127.0.0.1` only. Each connection starts with one `hello` line carrying the token that
//! [`ControlServer::start`] generated (and optionally wrote to a file), then speaks the JSON-lines protocol from
//! [`crate::protocol`]. Connection threads parse requests and push them into the [`ControlInbox`]; the app's UI thread
//! drains the inbox once per frame with [`ControlInbox::pump`], which hands each request to a [`ControlHost`]. Replies
//! flow back over the ticket's channel to the connection's writer thread, so the host can answer immediately or on a
//! later frame. Dropping a connection cancels its in-flight calls.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded, unbounded};
use serde_json::{Value, json};
use tokio::sync::mpsc::unbounded_channel;

use crate::call::{Backend, Incoming, Ticket};
use crate::host::ControlHost;
use crate::protocol::{CONTROL_VERSION, ControlError, ErrorKind, Event, Hello, MAX_REQUEST_BYTES, Request};
use crate::wire::{Line, event_line, read_line, tokens_match, write_line};

/// Most simultaneous control connections.
pub const MAX_CONNECTIONS: usize = 16;
/// Longest a client may take to send its hello line.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// Settings for [`ControlServer::start`].
#[derive(Debug, Clone, Default)]
pub struct ControlConfig {
    /// Port to listen on; 0 picks a free one.
    pub port: u16,
    /// Where to write the token (and, on a second line, `host:port`). Created with owner-only permissions on Unix.
    pub token_file: Option<PathBuf>,
}

/// The receiving end of the control channel, owned by the thread that owns the session.
#[derive(Debug)]
pub struct ControlInbox {
    rx: Receiver<Incoming>,
}

impl ControlInbox {
    /// An inbox fed directly by a [`Backend`] in the same process, with no socket: the app's own `--mcp-stdio` mode
    /// runs the MCP front-end on a thread, backed by this, while the UI thread keeps owning the session. `wake` is
    /// called (from a small helper thread) whenever a request is waiting, so the UI can repaint and pump.
    ///
    /// # Errors
    /// Fails when the helper thread cannot start.
    pub fn in_process<W: Fn() + Send + 'static>(wake: W) -> io::Result<(Self, Backend)> {
        let (tx, rx) = unbounded::<Incoming>();
        let (relay_tx, relay_rx) = unbounded::<Incoming>();
        // A `Backend` can only hold a plain channel, so a relay thread forwards each request and then wakes the UI.
        thread::Builder::new().name("anim-mcp-relay".into()).spawn(move || {
            while let Ok(incoming) = relay_rx.recv() {
                if tx.send(incoming).is_err() {
                    break;
                }
                wake();
            }
        })?;
        Ok((Self { rx }, Backend::new(relay_tx, "in-process", "in-process session")))
    }

    /// Hands every waiting request to `host`. Returns how many were handled. Call once per frame.
    pub fn pump<H: ControlHost>(&self, host: &mut H) -> usize {
        let mut handled = 0_usize;
        while let Ok(incoming) = self.rx.try_recv() {
            host.handle(incoming);
            handled = handled.saturating_add(1);
        }
        host.tick();
        handled
    }

    /// The next waiting request, if any. For apps that answer some requests themselves before forwarding the rest.
    #[must_use]
    pub fn try_next(&self) -> Option<Incoming> {
        self.rx.try_recv().ok()
    }

    /// Waits up to `timeout` for a request (for loops without a frame clock).
    #[must_use]
    pub fn next_timeout(&self, timeout: Duration) -> Option<Incoming> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Whether every connection thread and the server are gone.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        matches!(self.rx.try_recv(), Err(TryRecvError::Disconnected))
    }
}

/// A running control server. Dropping it stops accepting new connections.
#[derive(Debug)]
pub struct ControlServer {
    addr: SocketAddr,
    token: String,
    inbox: ControlInbox,
    stop: Option<Sender<()>>,
    accept: Option<JoinHandle<()>>,
}

impl ControlServer {
    /// Binds the loopback port, generates the token, optionally writes the token file, and starts accepting.
    /// `wake` is called whenever a request arrives so the UI can repaint and drain the inbox.
    ///
    /// # Errors
    /// Binding, the token file or thread creation can fail.
    pub fn start<W: Fn() + Clone + Send + 'static>(config: &ControlConfig, wake: W) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port))?;
        let addr = listener.local_addr()?;
        let token = generate_token()?;
        if let Some(path) = &config.token_file {
            write_token_file(path, &token, addr)?;
        }
        let (tx, rx) = unbounded::<Incoming>();
        let (stop_tx, stop_rx) = bounded::<()>(1);
        let accept_token = token.clone();
        let accept = thread::Builder::new().name("anim-mcp-accept".into()).spawn(move || accept_loop(&listener, &accept_token, &tx, &stop_rx, &wake))?;
        Ok(Self { addr, token, inbox: ControlInbox { rx }, stop: Some(stop_tx), accept: Some(accept) })
    }

    /// The address clients connect to.
    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The secret clients must present.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The inbox to drain.
    #[must_use]
    pub const fn inbox(&self) -> &ControlInbox {
        &self.inbox
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop = None; // the accept loop sees the channel close after its next accept
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

/// 32 random bytes as hex.
fn generate_token() -> io::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(format!("no random numbers available: {e}")))?;
    let mut token = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

fn write_token_file(path: &Path, token: &str, addr: SocketAddr) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    writeln!(file, "{token}\n{addr}")
}

/// Reads a token file written by [`ControlServer::start`]: `(token, address)`.
///
/// # Errors
/// I/O errors, or a file that is not in the expected format.
pub fn read_token_file(path: &Path) -> io::Result<(String, SocketAddr)> {
    let text = std::fs::read_to_string(path)?;
    let mut lines = text.lines();
    let token = lines.next().unwrap_or_default().trim().to_owned();
    let addr = lines
        .next()
        .unwrap_or_default()
        .trim()
        .parse::<SocketAddr>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("token file {}: bad address: {e}", path.display())))?;
    if token.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("token file {} is empty", path.display())));
    }
    Ok((token, addr))
}

fn accept_loop<W: Fn() + Clone + Send + 'static>(listener: &TcpListener, token: &str, inbox: &Sender<Incoming>, stop: &Receiver<()>, wake: &W) {
    let mut connections: Vec<JoinHandle<()>> = Vec::new();
    for stream in listener.incoming() {
        if matches!(stop.try_recv(), Err(TryRecvError::Disconnected)) {
            break;
        }
        let Ok(mut stream) = stream else { continue };
        connections.retain(|handle| !handle.is_finished());
        if connections.len() >= MAX_CONNECTIONS {
            let error = ControlError::new(ErrorKind::Unavailable, format!("too many control connections (limit {MAX_CONNECTIONS})"));
            let _ = write_line(&mut stream, &json!({ "ok": false, "error": error }));
            continue;
        }
        let (token, inbox, wake) = (token.to_owned(), inbox.clone(), wake.clone());
        let peer = stream.peer_addr().map_or_else(|_| "unknown".to_owned(), |a| a.to_string());
        if let Ok(handle) = thread::Builder::new().name("anim-mcp-conn".into()).spawn(move || {
            let _ = serve_connection(stream, &token, &inbox, &wake, &peer);
        }) {
            connections.push(handle);
        }
    }
}

fn serve_connection<W: Fn() + Clone + Send + 'static>(stream: TcpStream, token: &str, inbox: &Sender<Incoming>, wake: &W, peer: &str) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HELLO_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    // Handshake: the first line must be the hello with the right token. Nothing else is accepted before it.
    let hello = match read_line(&mut reader, 4096)? {
        Line::Text(text) => {
            serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("params").cloned().or(Some(v))).and_then(|v| serde_json::from_value::<Hello>(v).ok())
        }
        _ => None,
    };
    let Some(hello) = hello.filter(|h| tokens_match(&h.token, token)) else {
        let error = ControlError::new(ErrorKind::Denied, "bad or missing token; read it from the token file");
        write_line(&mut writer, &json!({ "ok": false, "error": error }))?;
        return Ok(());
    };
    reader.get_ref().set_read_timeout(None)?;
    write_line(&mut writer, &json!({ "ok": true, "result": { "protocol": CONTROL_VERSION, "client_version": hello.version } }))?;

    let (events_tx, mut events_rx) = unbounded_channel::<(u64, Event)>();
    let (finished_tx, finished_rx) = unbounded::<u64>();
    let writer_thread = thread::Builder::new().name("anim-mcp-conn-writer".into()).spawn(move || {
        while let Some((id, event)) = events_rx.blocking_recv() {
            let done = matches!(event, Event::Done(_));
            if write_line(&mut writer, &event_line(id, &event)).is_err() {
                break;
            }
            if done {
                let _ = finished_tx.send(id);
            }
        }
    })?;

    let mut inflight: HashMap<u64, Sender<()>> = HashMap::new();
    loop {
        while let Ok(id) = finished_rx.try_recv() {
            inflight.remove(&id);
        }
        let text = match read_line(&mut reader, MAX_REQUEST_BYTES)? {
            Line::Eof => break,
            Line::Text(text) if text.trim().is_empty() => continue,
            Line::Text(text) => text,
            Line::TooLong => {
                reject(&events_tx, 0, ControlError::invalid("request line too long; closing the connection"));
                break;
            }
            Line::NotUtf8 => {
                reject(&events_tx, 0, ControlError::invalid("request is not valid UTF-8; closing the connection"));
                break;
            }
        };
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(&text) else {
            reject(&events_tx, 0, ControlError::invalid("every line must be a JSON object; closing the connection"));
            break;
        };
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            reject(&events_tx, 0, ControlError::invalid("missing string `method`; closing the connection"));
            break;
        };
        if method == "cancel" {
            let target = message.get("params").and_then(|p| p.get("id")).and_then(Value::as_u64);
            if let Some(cancel) = target.and_then(|id| inflight.remove(&id)) {
                drop(cancel); // dropping the sender is what cancels the ticket
            }
            continue;
        }
        let id = message.get("id").and_then(Value::as_u64).unwrap_or(0);
        let request = match Request::from_wire(method, message.get("params")) {
            Ok(request) => request,
            Err(error) => {
                reject(&events_tx, id, error);
                continue;
            }
        };
        let (cancel_tx, cancel_rx) = bounded::<()>(1);
        let ticket = Ticket::new(id, events_tx.clone(), cancel_rx);
        inflight.insert(id, cancel_tx);
        if inbox.send(Incoming { request, ticket: ticket.clone(), origin: format!("bridge {peer}") }).is_err() {
            ticket.finish(Err(ControlError::new(ErrorKind::Unavailable, "the application is shutting down")));
            break;
        }
        wake();
    }
    drop(inflight); // cancels whatever is still running for this connection
    drop(events_tx);
    let _ = writer_thread.join();
    Ok(())
}

fn reject(events: &tokio::sync::mpsc::UnboundedSender<(u64, Event)>, id: u64, error: ControlError) {
    let _ = events.send((id, Event::Done(Err(error))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control.token");
        let addr: SocketAddr = "127.0.0.1:4242".parse().unwrap();
        write_token_file(&path, "secret", addr).unwrap();
        assert_eq!(read_token_file(&path).unwrap(), ("secret".to_owned(), addr));
        std::fs::write(&path, "\n").unwrap();
        assert!(read_token_file(&path).is_err());
    }

    #[test]
    fn tokens_are_random_hex() {
        let a = generate_token().unwrap();
        let b = generate_token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
