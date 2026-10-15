//! The bridge backend: a [`Backend`] that forwards requests to a running app's [`crate::ControlServer`].
//!
//! An actor thread owns the TCP stream and the table of calls in flight. It multiplexes: any number of requests can be
//! outstanding on the one connection, answers are routed back to their tickets by id, progress lines are forwarded, and
//! a call whose ticket reads as cancelled sends a `cancel` line to the app. If the connection breaks, every call in
//! flight fails with a `transport` error and later calls fail with `unavailable`.

use std::collections::HashMap;
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, unbounded};
use serde_json::{Value, json};

use crate::call::{Backend, Incoming, TICK, Ticket};
use crate::protocol::{CONTROL_VERSION, ControlError, ErrorKind, Event};
use crate::server::read_token_file;
use crate::wire::{Line, parse_event_line, read_line, write_line};

/// Longest accepted reply line (renders and screenshots travel as base64), in bytes.
const MAX_REPLY_BYTES: u64 = 256 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

enum FromApp {
    Line(Value),
    Closed(String),
}

struct Waiting {
    ticket: Ticket,
    cancel_sent: bool,
}

/// Connects to a control server and returns a backend that talks to it.
///
/// # Errors
/// Connecting, the handshake (wrong token, protocol mismatch) or thread creation can fail.
pub fn connect_bridge(addr: SocketAddr, token: &str) -> io::Result<Backend> {
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    write_line(&mut writer, &json!({ "method": "hello", "params": { "token": token, "version": CONTROL_VERSION } }))?;
    match read_line(&mut reader, 64 * 1024)? {
        Line::Text(text) => {
            let reply: Value = serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad handshake reply: {e}")))?;
            if reply.get("ok").and_then(Value::as_bool) != Some(true) {
                let why = reply.get("error").and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or("handshake refused");
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, why.to_owned()));
            }
        }
        _ => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the app closed the connection during the handshake")),
    }
    reader.get_ref().set_read_timeout(None)?;

    let (lines_tx, lines_rx) = unbounded::<FromApp>();
    thread::Builder::new().name("anim-mcp-bridge-reader".into()).spawn(move || {
        loop {
            let message = match read_line(&mut reader, MAX_REPLY_BYTES) {
                Ok(Line::Text(text)) => match serde_json::from_str::<Value>(&text) {
                    Ok(value) => FromApp::Line(value),
                    Err(e) => FromApp::Closed(format!("the app sent invalid JSON: {e}")),
                },
                Ok(Line::Eof) => FromApp::Closed("the app closed the connection".to_owned()),
                Ok(Line::TooLong) => FromApp::Closed("the app sent a line over the size limit".to_owned()),
                Ok(Line::NotUtf8) => FromApp::Closed("the app sent invalid UTF-8".to_owned()),
                Err(e) => FromApp::Closed(format!("connection error: {e}")),
            };
            let closed = matches!(message, FromApp::Closed(_));
            if lines_tx.send(message).is_err() || closed {
                break;
            }
        }
    })?;

    let (calls_tx, calls_rx) = unbounded::<Incoming>();
    thread::Builder::new().name("anim-mcp-bridge".into()).spawn(move || run_actor(&calls_rx, &lines_rx, writer))?;
    Ok(Backend::new(calls_tx, "mcp", format!("bridge to {addr}")))
}

/// Connects using a token file written by the app (`token`, then `host:port`).
///
/// # Errors
/// As [`connect_bridge`], plus problems reading the file.
pub fn connect_bridge_from_file(token_file: &Path) -> io::Result<Backend> {
    let (token, addr) = read_token_file(token_file)?;
    connect_bridge(addr, &token)
}

fn run_actor(calls: &Receiver<Incoming>, lines: &Receiver<FromApp>, mut writer: TcpStream) {
    let mut waiting: HashMap<u64, Waiting> = HashMap::new();
    let mut next_id = 1_u64;
    let mut closed: Option<String> = None;
    loop {
        if let Some(why) = &closed {
            // The connection is gone: refuse everything until the front-end lets go.
            match calls.recv() {
                Ok(incoming) => incoming.ticket.finish(Err(ControlError::new(ErrorKind::Unavailable, why.clone()))),
                Err(_) => return,
            }
            continue;
        }
        // Requests from the front-end.
        loop {
            match calls.try_recv() {
                Ok(incoming) => {
                    let Incoming { request, ticket, .. } = incoming;
                    if let Some(why) = &closed {
                        ticket.finish(Err(ControlError::new(ErrorKind::Unavailable, why.clone())));
                        continue;
                    }
                    let id = next_id;
                    next_id = next_id.saturating_add(1);
                    let mut line = serde_json::to_value(&request).unwrap_or(Value::Null);
                    if let Some(map) = line.as_object_mut() {
                        map.insert("id".into(), json!(id));
                    }
                    if let Err(e) = write_line(&mut writer, &line) {
                        let why = format!("could not send to the app: {e}");
                        ticket.finish(Err(ControlError::new(ErrorKind::Transport, why.clone())));
                        closed = Some(why);
                    } else {
                        waiting.insert(id, Waiting { ticket, cancel_sent: false });
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => return,
            }
        }
        // Answers from the app.
        match lines.recv_timeout(TICK) {
            Ok(FromApp::Line(value)) => route(&mut waiting, &value),
            Ok(FromApp::Closed(why)) => closed = Some(why),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => closed = closed.or_else(|| Some("the bridge reader stopped".to_owned())),
        }
        if let Some(why) = &closed {
            for (_, call) in waiting.drain() {
                call.ticket.finish(Err(ControlError::new(ErrorKind::Transport, why.clone())));
            }
        }
        // Cancellations.
        let to_cancel: Vec<u64> = waiting.iter().filter(|(_, w)| !w.cancel_sent && w.ticket.is_cancelled()).map(|(id, _)| *id).collect();
        for id in to_cancel {
            if let Some(call) = waiting.get_mut(&id) {
                call.cancel_sent = true;
            }
            let _ = write_line(&mut writer, &json!({ "method": "cancel", "params": { "id": id } }));
        }
    }
}

fn route(waiting: &mut HashMap<u64, Waiting>, line: &Value) {
    let Some((id, event)) = parse_event_line(line) else {
        return;
    };
    match event {
        Event::Progress(p) => {
            if let Some(call) = waiting.get(&id) {
                call.ticket.progress(p.fraction, p.message);
            }
        }
        Event::Done(outcome) => {
            if let Some(call) = waiting.remove(&id) {
                call.ticket.finish(outcome);
            }
        }
    }
}
