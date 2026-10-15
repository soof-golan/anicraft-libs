//! A Model Context Protocol (MCP) server for any [`Session`], built from the command registry.
//!
//! The design in three layers, each usable alone:
//!
//! 1. **Protocol** ([`McpServer::handle`]): a pure function from one JSON-RPC message and the `&mut Session` to an
//!    optional response. It speaks `initialize`, `ping`, `tools/list`, `tools/call`, `resources/list` and
//!    `resources/read`. Every registered command that is not hidden becomes a tool (dots in the id become underscores
//!    in the tool name), with its JSON Schema, a read-only hint for transient commands and `_meta` carrying its menu
//!    path, shortcut and whether it is enabled right now. Tool calls go through [`Session::execute`], so MCP edits are
//!    undoable steps exactly like menu clicks. A built-in `session_status` tool and `anim://session/status` resource
//!    describe the document, dirty state and undo stacks.
//! 2. **Blocking serve loop** ([`McpServer::serve`]): newline-delimited JSON over any `BufRead`/`Write` pair, for
//!    headless tools and tests.
//! 3. **Actor transports** ([`spawn_line_transport`], [`spawn_stdio`], [`listen_tcp`]): a reader thread per
//!    connection forwards each message over a channel to the owner of the session, which calls [`McpInbox::pump`] once
//!    per frame and so keeps all document state on one thread (no locks). The UI is asked to repaint through a
//!    caller-supplied wake function. Applications with tools of their own (a screenshot, say) look at
//!    [`McpRequest::tool_call`] before answering.
//!
//! Over stdio, stdout carries only protocol messages: log to stderr.

use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded, unbounded};
use serde_json::{Map, Value, json};

use crate::command::{CommandSpec, HistoryMode, tool_name};
use crate::history::SizeHint;
use crate::session::Session;

/// Newest protocol revision implemented.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
/// Name of the built-in status tool.
pub const STATUS_TOOL: &str = "session_status";
/// URI of the built-in status resource.
pub const STATUS_URI: &str = "anim://session/status";
/// Longest accepted line (one JSON-RPC message), in bytes.
pub const MAX_MESSAGE_BYTES: u64 = 16 * 1024 * 1024;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn response(id: &Value, result: Value) -> Value {
    let mut message = Map::new();
    message.insert("jsonrpc".into(), json!("2.0"));
    message.insert("id".into(), id.clone());
    message.insert("result".into(), result);
    Value::Object(message)
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A JSON-RPC level failure (as opposed to a tool that ran and reported `isError`).
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn params(message: impl Into<String>) -> Self {
        Self { code: INVALID_PARAMS, message: message.into() }
    }
}

/// The MCP face of an application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    name: String,
    version: String,
    instructions: String,
}

impl McpServer {
    /// A server announcing itself as `name` / `version`.
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            instructions: "Every tool is an editor command. Call session_status first to see the open document, then \
                           run commands; each edit is one undoable step (edit_undo / edit_redo)."
                .to_owned(),
        }
    }

    /// Replaces the instructions text clients may show to the model.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    /// Descriptor of one command as an MCP tool.
    fn tool_descriptor<D: Clone + SizeHint>(session: &Session<D>, spec: &CommandSpec<D>) -> Value {
        let mut description = if spec.description.is_empty() { spec.label.to_owned() } else { spec.description.to_owned() };
        if !spec.menu.is_empty() {
            let _ = write!(description, " (menu: {} > {})", spec.menu.join(" > "), spec.label);
        }
        let read_only = spec.history == HistoryMode::Transient && spec.action == crate::command::Action::Run;
        let mut meta = Map::new();
        meta.insert("anim/command".into(), json!(spec.id));
        meta.insert("anim/enabled".into(), json!(session.is_enabled(spec.id)));
        if !spec.menu.is_empty() {
            meta.insert("anim/menu".into(), json!(spec.menu));
        }
        if let Some(shortcut) = session.registry().shortcut_of(spec.id) {
            meta.insert("anim/shortcut".into(), json!(shortcut.to_string()));
        }
        json!({
            "name": tool_name(spec.id),
            "title": spec.label,
            "description": description,
            "inputSchema": spec.schema_value(),
            "annotations": {
                "readOnlyHint": read_only,
                "destructiveHint": false,
                "openWorldHint": false,
            },
            "_meta": meta,
        })
    }

    /// All tools: the built-in status tool, then every command exposed to MCP.
    #[must_use]
    pub fn tools<D: Clone + SizeHint>(&self, session: &Session<D>) -> Vec<Value> {
        let mut tools = vec![json!({
            "name": STATUS_TOOL,
            "title": "Session status",
            "description": "Describe the open document: file path, unsaved changes, undo and redo history.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true, "destructiveHint": false, "openWorldHint": false },
        })];
        tools.extend(session.registry().iter().filter(|s| s.mcp).map(|s| Self::tool_descriptor(session, s)));
        tools
    }

    /// Runs a tool by name. Tool failures come back as `Ok` with `isError`; only unknown tools or malformed
    /// arguments are protocol errors.
    fn call_tool<D: Clone + SizeHint>(session: &mut Session<D>, name: &str, arguments: &Value) -> Result<Value, RpcError> {
        let args = match arguments {
            Value::Null => Value::Object(Map::new()),
            Value::Object(_) => arguments.clone(),
            _ => return Err(RpcError::params("arguments must be an object")),
        };
        if name == STATUS_TOOL {
            return Ok(tool_result(&status(session), false));
        }
        let id = session.registry().get_by_tool_name(name).filter(|s| s.mcp).map(|s| s.id).ok_or_else(|| RpcError::params(format!("unknown tool {name:?}")))?;
        Ok(match session.execute(id, &args) {
            Ok(value) => tool_result(&value, false),
            Err(e) => tool_result(&Value::String(e.to_string()), true),
        })
    }

    /// Handles one JSON-RPC message. Returns `None` for notifications and responses (nothing to send back).
    pub fn handle<D: Clone + SizeHint>(&self, session: &mut Session<D>, message: &Value) -> Option<Value> {
        let Some(object) = message.as_object() else {
            return Some(error_response(&Value::Null, INVALID_REQUEST, "a message must be a JSON object"));
        };
        let id = object.get("id");
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            if object.contains_key("result") || object.contains_key("error") {
                return None; // a reply to a request we never send
            }
            return Some(error_response(id.unwrap_or(&Value::Null), INVALID_REQUEST, "missing method"));
        };
        let id = id?; // no id: a notification, never answered
        let params = object.get("params").unwrap_or(&Value::Null);
        let outcome = self.dispatch(session, method, params);
        Some(match outcome {
            Ok(result) => response(id, result),
            Err(e) => error_response(id, e.code, &e.message),
        })
    }

    fn dispatch<D: Clone + SizeHint>(&self, session: &mut Session<D>, method: &str, params: &Value) -> Result<Value, RpcError> {
        match method {
            "initialize" => {
                let wanted = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                let version = SUPPORTED_VERSIONS.iter().find(|v| **v == wanted).copied().unwrap_or(PROTOCOL_VERSION);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false }, "resources": { "subscribe": false, "listChanged": false } },
                    "serverInfo": { "name": self.name, "version": self.version },
                    "instructions": self.instructions,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tools(session) })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or_else(|| RpcError::params("tools/call needs a string \"name\""))?;
                Self::call_tool(session, name, params.get("arguments").unwrap_or(&Value::Null))
            }
            "resources/list" => Ok(json!({ "resources": [{
                "uri": STATUS_URI,
                "name": "Session status",
                "description": "Open document, unsaved changes and undo history.",
                "mimeType": "application/json",
            }] })),
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or_default();
                if uri != STATUS_URI {
                    return Err(RpcError::params(format!("unknown resource {uri:?}")));
                }
                Ok(json!({ "contents": [{
                    "uri": STATUS_URI,
                    "mimeType": "application/json",
                    "text": status(session).to_string(),
                }] }))
            }
            other => Err(RpcError { code: METHOD_NOT_FOUND, message: format!("method {other:?} not found") }),
        }
    }

    /// Parses one line of text and handles it (a parse failure produces the JSON-RPC parse error).
    pub fn handle_text<D: Clone + SizeHint>(&self, session: &mut Session<D>, text: &str) -> Option<Value> {
        match serde_json::from_str::<Value>(text) {
            Ok(message) => self.handle(session, &message),
            Err(e) => Some(error_response(&Value::Null, PARSE_ERROR, &format!("parse error: {e}"))),
        }
    }

    /// Reads newline-delimited messages until end of input, writing one line per response.
    ///
    /// # Errors
    /// I/O errors from the reader or writer.
    pub fn serve<D: Clone + SizeHint, R: BufRead, W: Write>(&self, session: &mut Session<D>, mut reader: R, mut writer: W) -> io::Result<()> {
        loop {
            match read_line_bounded(&mut reader)? {
                Line::Eof => return Ok(()),
                Line::TooLong => {
                    write_line(&mut writer, &error_response(&Value::Null, INVALID_REQUEST, "message too large"))?;
                    return Ok(());
                }
                Line::Text(text) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    if let Some(reply) = self.handle_text(session, &text) {
                        write_line(&mut writer, &reply)?;
                    }
                }
            }
        }
    }
}

/// JSON describing the session, returned by the status tool and resource.
#[must_use]
pub fn status<D: Clone + SizeHint>(session: &Session<D>) -> Value {
    let history = session.history();
    json!({
        "path": session.path().map(|p| p.display().to_string()),
        "title": session.title(""),
        "dirty": session.is_dirty(),
        "can_undo": history.can_undo(),
        "can_redo": history.can_redo(),
        "undo": history.undo_labels().take(20).collect::<Vec<_>>(),
        "redo": history.redo_labels().take(20).collect::<Vec<_>>(),
        "undo_steps": history.undo_len(),
        "redo_steps": history.redo_len(),
        "history_bytes": history.bytes(),
        "gesture_open": history.is_open(),
        "commands": session.registry().len(),
    })
}

/// Builds a `tools/call` result: the value as text, plus `structuredContent` when it is a JSON object.
fn tool_result(value: &Value, is_error: bool) -> Value {
    let text = match value {
        Value::Null => "ok".to_owned(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let mut result = Map::new();
    result.insert("content".into(), json!([{ "type": "text", "text": text }]));
    if let (Value::Object(_), false) = (value, is_error) {
        result.insert("structuredContent".into(), value.clone());
    }
    result.insert("isError".into(), Value::Bool(is_error));
    Value::Object(result)
}

enum Line {
    Eof,
    Text(String),
    TooLong,
}

fn read_line_bounded<R: BufRead>(reader: &mut R) -> io::Result<Line> {
    let mut buf = Vec::new();
    let read = Read::take(&mut *reader, MAX_MESSAGE_BYTES.saturating_add(1)).read_until(b'\n', &mut buf)?;
    if read == 0 {
        return Ok(Line::Eof);
    }
    if !buf.ends_with(b"\n") && u64::try_from(buf.len()).unwrap_or(u64::MAX) > MAX_MESSAGE_BYTES {
        return Ok(Line::TooLong);
    }
    Ok(Line::Text(String::from_utf8_lossy(&buf).into_owned()))
}

fn write_line<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let mut text = value.to_string();
    text.push('\n');
    writer.write_all(text.as_bytes())?;
    writer.flush()
}

/// One message waiting for the session's owner.
#[derive(Debug)]
pub struct McpRequest {
    /// The parsed JSON-RPC message.
    pub message: Value,
    reply: Sender<Option<Value>>,
}

impl McpRequest {
    /// For a `tools/call`, the tool name and arguments; applications use it to serve their own tools first.
    #[must_use]
    pub fn tool_call(&self) -> Option<(&str, &Value)> {
        if self.message.get("method").and_then(Value::as_str) != Some("tools/call") {
            return None;
        }
        let params = self.message.get("params")?;
        let name = params.get("name").and_then(Value::as_str)?;
        Some((name, params.get("arguments").unwrap_or(&Value::Null)))
    }

    /// The request id, if it has one.
    #[must_use]
    pub fn id(&self) -> Option<&Value> {
        self.message.get("id")
    }

    /// A successful `tools/call` result with text content, for applications' own tools.
    #[must_use]
    pub fn tool_ok(&self, value: &Value) -> Value {
        response(self.id().unwrap_or(&Value::Null), tool_result(value, false))
    }

    /// A `tools/call` result with `isError`, for applications' own tools.
    #[must_use]
    pub fn tool_error(&self, message: &str) -> Value {
        response(self.id().unwrap_or(&Value::Null), tool_result(&Value::String(message.to_owned()), true))
    }

    /// Sends the reply (`None` for notifications).
    pub fn respond(self, reply: Option<Value>) {
        // The connection may have closed meanwhile; nobody is left to tell.
        let _ = self.reply.send(reply);
    }
}

/// The session owner's end of the transports.
#[derive(Debug)]
pub struct McpInbox {
    requests: Receiver<McpRequest>,
    local_addr: Option<SocketAddr>,
    _stop: Option<Sender<()>>,
}

impl McpInbox {
    /// The next waiting request, if any.
    #[must_use]
    pub fn try_next(&self) -> Option<McpRequest> {
        self.requests.try_recv().ok()
    }

    /// Waits up to `timeout` for a request.
    #[must_use]
    pub fn next_timeout(&self, timeout: Duration) -> Option<McpRequest> {
        self.requests.recv_timeout(timeout).ok()
    }

    /// Answers every waiting request through `server`. Returns how many were handled.
    pub fn pump<D: Clone + SizeHint>(&self, server: &McpServer, session: &mut Session<D>) -> usize {
        let mut handled = 0_usize;
        while let Some(request) = self.try_next() {
            let reply = server.handle(session, &request.message);
            request.respond(reply);
            handled = handled.saturating_add(1);
        }
        handled
    }

    /// Whether every transport has ended and nothing is queued (the client went away).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        matches!(self.requests.try_recv(), Err(TryRecvError::Disconnected))
    }

    /// The bound address of a TCP listener.
    #[must_use]
    pub const fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }
}

fn serve_lines<R: BufRead, W: Write>(mut reader: R, mut writer: W, tx: &Sender<McpRequest>, wake: &impl Fn()) {
    loop {
        match read_line_bounded(&mut reader) {
            Ok(Line::Eof) | Err(_) => return,
            Ok(Line::TooLong) => {
                let _ = write_line(&mut writer, &error_response(&Value::Null, INVALID_REQUEST, "message too large"));
                return;
            }
            Ok(Line::Text(text)) => {
                if text.trim().is_empty() {
                    continue;
                }
                let reply = match serde_json::from_str::<Value>(&text) {
                    Err(e) => Some(error_response(&Value::Null, PARSE_ERROR, &format!("parse error: {e}"))),
                    Ok(message) => {
                        let (reply_tx, reply_rx) = bounded(1);
                        if tx.send(McpRequest { message, reply: reply_tx }).is_err() {
                            return;
                        }
                        wake();
                        match reply_rx.recv() {
                            Ok(reply) => reply,
                            Err(_) => return,
                        }
                    }
                };
                if let Some(reply) = reply
                    && write_line(&mut writer, &reply).is_err()
                {
                    return;
                }
            }
        }
    }
}

/// Starts a reader thread over any reader/writer pair. `wake` runs after each queued request (ask the UI to repaint).
pub fn spawn_line_transport<R, W, F>(reader: R, writer: W, wake: F) -> McpInbox
where
    R: BufRead + Send + 'static,
    W: Write + Send + 'static,
    F: Fn() + Send + 'static,
{
    let (tx, requests) = unbounded();
    let _ = thread::Builder::new().name("mcp-line".into()).spawn(move || serve_lines(reader, writer, &tx, &wake));
    McpInbox { requests, local_addr: None, _stop: None }
}

/// Serves MCP on this process's standard input and output (the transport MCP clients use when they launch a server).
pub fn spawn_stdio<F: Fn() + Send + 'static>(wake: F) -> McpInbox {
    spawn_line_transport(BufReader::new(io::stdin()), io::stdout(), wake)
}

/// Serves newline-delimited MCP on a loopback TCP port so a client can attach to a running application
/// (use port 0 to let the system choose; read it back with [`McpInbox::local_addr`]).
///
/// # Errors
/// `InvalidInput` for a non-loopback address; otherwise the bind error.
pub fn listen_tcp<F>(addr: SocketAddr, wake: F) -> io::Result<McpInbox>
where
    F: Fn() + Send + Clone + 'static,
{
    if !addr.ip().is_loopback() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "MCP over TCP only listens on loopback addresses"));
    }
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let (tx, requests) = unbounded::<McpRequest>();
    let (stop, stop_rx) = bounded::<()>(1);
    thread::Builder::new().name("mcp-accept".into()).spawn(move || {
        while !matches!(stop_rx.try_recv(), Err(TryRecvError::Disconnected)) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let Ok(writer) = stream.try_clone() else { continue };
                    let tx = tx.clone();
                    let wake = wake.clone();
                    let _ = thread::Builder::new().name("mcp-conn".into()).spawn(move || serve_lines(BufReader::new(stream), writer, &tx, &wake));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
    })?;
    Ok(McpInbox { requests, local_addr: Some(local_addr), _stop: Some(stop) })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::net::TcpStream;

    use super::*;
    use crate::command::{CommandError, CommandRegistry, CommandSpec, param};

    #[derive(Debug, Clone, Default)]
    struct Doc {
        n: i64,
    }
    impl SizeHint for Doc {
        fn size_hint(&self) -> usize {
            8
        }
    }

    fn add(doc: &mut Doc, p: &Value) -> Result<Value, CommandError> {
        doc.n += param::<i64>(p, "by")?;
        Ok(json!({ "n": doc.n }))
    }
    fn get(doc: &mut Doc, _: &Value) -> Result<Value, CommandError> {
        Ok(json!({ "n": doc.n }))
    }
    fn positive(doc: &Doc) -> bool {
        doc.n > 0
    }

    fn session() -> Session<Doc> {
        let mut r = CommandRegistry::with_history_commands();
        r.register_all([
            CommandSpec::new("math.add", "Add", add)
                .in_menu(&["Math"])
                .with_shortcut("Cmd+Plus")
                .describe("Add a number to the counter.")
                .with_schema(r#"{"type":"object","properties":{"by":{"type":"integer"}},"required":["by"]}"#),
            CommandSpec::new("math.get", "Get", get).transient(),
            CommandSpec::new("math.secret", "Secret", get).hidden_from_mcp(),
            CommandSpec::new("math.only_positive", "Only positive", get).enabled_when(positive),
        ])
        .unwrap();
        Session::new(Doc::default(), r)
    }

    fn server() -> McpServer {
        McpServer::new("test", "0.0.1")
    }

    fn rpc(id: i64, method: &str, params: &Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    fn call(s: &mut Session<Doc>, name: &str, args: &Value) -> Value {
        server().handle(s, &rpc(1, "tools/call", &json!({ "name": name, "arguments": args }))).unwrap()
    }

    #[test]
    fn initialize_negotiates_version() {
        let mut s = session();
        let r = server().handle(&mut s, &rpc(1, "initialize", &json!({ "protocolVersion": "2025-03-26" }))).unwrap();
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "test");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        let r = server().handle(&mut s, &rpc(2, "initialize", &json!({ "protocolVersion": "1999-01-01" }))).unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    #[test]
    fn notifications_and_replies_get_no_answer() {
        let mut s = session();
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(server().handle(&mut s, &note).is_none());
        let reply = json!({ "jsonrpc": "2.0", "id": 4, "result": {} });
        assert!(server().handle(&mut s, &reply).is_none());
        let ping = server().handle(&mut s, &rpc(9, "ping", &Value::Null)).unwrap();
        assert_eq!(ping["result"], json!({}));
    }

    #[test]
    fn lists_commands_as_tools() {
        let mut s = session();
        let r = server().handle(&mut s, &rpc(1, "tools/list", &Value::Null)).unwrap();
        let tools = r["result"]["tools"].as_array().unwrap();
        let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["session_status", "edit_undo", "edit_redo", "math_add", "math_get", "math_only_positive"], "hidden commands are not offered");
        let add = tools.iter().find(|t| t["name"] == "math_add").unwrap();
        assert_eq!(add["inputSchema"]["required"][0], "by");
        assert_eq!(add["annotations"]["readOnlyHint"], false);
        assert_eq!(add["_meta"]["anim/shortcut"], "Cmd+Plus");
        assert_eq!(add["_meta"]["anim/enabled"], true);
        assert!(add["description"].as_str().unwrap().contains("Math > Add"));
        let get = tools.iter().find(|t| t["name"] == "math_get").unwrap();
        assert_eq!(get["annotations"]["readOnlyHint"], true);
        let undo = tools.iter().find(|t| t["name"] == "edit_undo").unwrap();
        assert_eq!(undo["annotations"]["readOnlyHint"], false);
        assert_eq!(undo["_meta"]["anim/enabled"], false);
        let only = tools.iter().find(|t| t["name"] == "math_only_positive").unwrap();
        assert_eq!(only["_meta"]["anim/enabled"], false);
        assert_eq!(only["inputSchema"], json!({ "type": "object" }));
    }

    #[test]
    fn tool_calls_run_commands_with_undo() {
        let mut s = session();
        let r = call(&mut s, "math_add", &json!({ "by": 5 }));
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(r["result"]["structuredContent"], json!({ "n": 5 }));
        assert_eq!(r["result"]["content"][0]["type"], "text");
        assert_eq!(s.doc().n, 5);
        assert!(s.is_dirty());

        let status = call(&mut s, "session_status", &json!({}));
        let st = &status["result"]["structuredContent"];
        assert_eq!(st["dirty"], true);
        assert_eq!(st["undo"], json!(["Add"]));
        assert_eq!(st["can_undo"], true);

        let r = call(&mut s, "edit_undo", &Value::Null);
        assert_eq!(r["result"]["structuredContent"], json!({ "undone": "Add" }));
        assert_eq!(s.doc().n, 0);
        assert!(!s.is_dirty());
    }

    #[test]
    fn tool_failures_are_results_not_protocol_errors() {
        let mut s = session();
        let r = call(&mut s, "math_add", &json!({}));
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("missing parameter"));
        assert!(r.get("error").is_none());
        let r = call(&mut s, "math_only_positive", &json!({}));
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("not enabled"));
        assert_eq!(s.history().undo_len(), 0, "failed calls record nothing");
    }

    #[test]
    fn protocol_errors() {
        let mut s = session();
        let r = call(&mut s, "nope", &json!({}));
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let r = call(&mut s, "math_secret", &json!({}));
        assert_eq!(r["error"]["code"], INVALID_PARAMS, "hidden commands cannot be called");
        let r = server().handle(&mut s, &rpc(1, "tools/call", &json!({ "name": "math_get", "arguments": 3 }))).unwrap();
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let r = server().handle(&mut s, &rpc(1, "tools/call", &json!({}))).unwrap();
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let r = server().handle(&mut s, &rpc(7, "does/not/exist", &Value::Null)).unwrap();
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(r["id"], 7);
        let r = server().handle(&mut s, &json!([1, 2])).unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        let r = server().handle(&mut s, &json!({ "id": 3 })).unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        assert_eq!(r["id"], 3);
        let r = server().handle_text(&mut s, "{not json").unwrap();
        assert_eq!(r["error"]["code"], PARSE_ERROR);
    }

    #[test]
    fn status_resource() {
        let mut s = session();
        let r = server().handle(&mut s, &rpc(1, "resources/list", &Value::Null)).unwrap();
        assert_eq!(r["result"]["resources"][0]["uri"], STATUS_URI);
        let r = server().handle(&mut s, &rpc(2, "resources/read", &json!({ "uri": STATUS_URI }))).unwrap();
        let text = r["result"]["contents"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["dirty"], false);
        let r = server().handle(&mut s, &rpc(3, "resources/read", &json!({ "uri": "anim://other" }))).unwrap();
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn serve_loop_over_text() {
        let mut s = session();
        let input = [
            rpc(1, "initialize", &json!({})).to_string(),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
            String::new(),
            "garbage".to_owned(),
            rpc(2, "tools/call", &json!({ "name": "math_add", "arguments": { "by": 2 } })).to_string(),
        ]
        .join("\n");
        let mut out = Vec::new();
        server().serve(&mut s, Cursor::new(input), &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 3, "the notification and blank line produce nothing");
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[1]["error"]["code"], PARSE_ERROR);
        assert_eq!(lines[2]["result"]["structuredContent"], json!({ "n": 2 }));
    }

    #[test]
    fn oversized_lines_are_refused() {
        let mut s = session();
        let big = vec![b'a'; usize::try_from(MAX_MESSAGE_BYTES).unwrap() + 10];
        let mut out = Vec::new();
        server().serve(&mut s, Cursor::new(big), &mut out).unwrap();
        let reply: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(reply["error"]["code"], INVALID_REQUEST);
    }

    struct ChanWriter(Sender<Vec<u8>>);
    impl Write for ChanWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let _ = self.0.send(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn actor_transport_hands_requests_to_the_session_owner() {
        let mut s = session();
        let srv = server();
        let input = [
            rpc(1, "tools/call", &json!({ "name": "math_add", "arguments": { "by": 4 } })).to_string(),
            "not json".to_owned(),
            rpc(2, "tools/call", &json!({ "name": "screenshot", "arguments": {} })).to_string(),
        ]
        .join("\n");
        let (out_tx, out_rx) = unbounded();
        let (wake_tx, wake_rx) = unbounded();
        let inbox = spawn_line_transport(Cursor::new(input.into_bytes()), ChanWriter(out_tx), move || {
            let _ = wake_tx.send(());
        });
        let mut handled = 0;
        while handled < 2 {
            let Some(request) = inbox.next_timeout(Duration::from_secs(10)) else { break };
            // An application tool first, otherwise the standard handler.
            if let Some(("screenshot", _)) = request.tool_call() {
                let reply = request.tool_ok(&json!({ "saved": "x.png" }));
                request.respond(Some(reply));
            } else {
                let reply = srv.handle(&mut s, &request.message);
                request.respond(reply);
            }
            handled += 1;
        }
        assert_eq!(handled, 2);
        assert!(wake_rx.try_recv().is_ok(), "the UI was woken");
        assert_eq!(s.doc().n, 4);
        let mut text = String::new();
        while let Ok(chunk) = out_rx.recv_timeout(Duration::from_secs(10)) {
            text.push_str(&String::from_utf8(chunk).unwrap());
            if text.matches('\n').count() >= 3 {
                break;
            }
        }
        let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["result"]["structuredContent"]["n"], 4);
        assert_eq!(lines[1]["error"]["code"], PARSE_ERROR);
        assert_eq!(lines[2]["result"]["structuredContent"]["saved"], "x.png");
        assert!(inbox.is_closed() || inbox.try_next().is_none());
    }

    #[test]
    fn tcp_transport_serves_a_client_and_rejects_public_addresses() {
        assert!(listen_tcp("0.0.0.0:0".parse().unwrap(), || {}).is_err());
        let inbox = listen_tcp("127.0.0.1:0".parse().unwrap(), || {}).unwrap();
        let addr = inbox.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let msg = rpc(1, "tools/call", &json!({ "name": "math_add", "arguments": { "by": 9 } }));
            writeln!(stream, "{msg}").unwrap();
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        });
        let mut s = session();
        let srv = server();
        let mut handled = 0;
        let start = std::time::Instant::now();
        while handled == 0 && start.elapsed() < Duration::from_secs(10) {
            handled += inbox.pump(&srv, &mut s);
            thread::sleep(Duration::from_millis(5));
        }
        let reply = client.join().unwrap();
        assert_eq!(reply["result"]["structuredContent"]["n"], 9);
        assert_eq!(s.doc().n, 9);
    }
}
