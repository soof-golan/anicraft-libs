//! A toy document and the hooks to drive it, shared by the integration tests.
#![allow(
    dead_code,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    clippy::missing_const_for_fn,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::unnecessary_wraps,
    clippy::collapsible_if,
    clippy::needless_pass_by_value,
    clippy::panic_in_result_fn,
    reason = "test support code"
)]

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use anim_mcp::props;
use anim_mcp::{
    AppHooks, Confinement, ControlError, ErrorKind, ExportFormat, ExportRequest, HookOutcome, HookResult, HostInfo, QuerySpec, RenderRequest, Reply,
    ScreenshotRequest, SessionHost, Ticket,
};
use anim_session::{CommandError, CommandRegistry, CommandSpec, Session, SizeHint, param, param_opt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Toy {
    pub title: String,
    pub frame: i64,
    pub layers: Vec<Layer>,
}

impl SizeHint for Toy {
    fn size_hint(&self) -> usize {
        64 + self.layers.len() * 48
    }
}

impl Default for Toy {
    fn default() -> Self {
        Self { title: "Untitled".into(), frame: 0, layers: Vec::new() }
    }
}

fn layer_add(doc: &mut Toy, params: &Value) -> Result<Value, CommandError> {
    let name: String = param(params, "name")?;
    let x = param_opt::<f64>(params, "x")?.unwrap_or(0.0);
    doc.layers.push(Layer { name, x, y: 0.0 });
    Ok(json!({ "index": doc.layers.len() - 1 }))
}

fn layer_move(doc: &mut Toy, params: &Value) -> Result<Value, CommandError> {
    let index: usize = param(params, "index")?;
    let dx: f64 = param(params, "dx")?;
    let layer = doc.layers.get_mut(index).ok_or_else(|| CommandError::failed(format!("no layer {index}")))?;
    layer.x += dx;
    Ok(json!({ "x": layer.x }))
}

fn layer_fail(doc: &mut Toy, _params: &Value) -> Result<Value, CommandError> {
    doc.layers.clear(); // damage first: the session must roll it back
    Err(CommandError::failed("this command always fails"))
}

fn layer_panic(doc: &mut Toy, _params: &Value) -> Result<Value, CommandError> {
    doc.title.push('!');
    panic!("the toy command panicked on purpose");
}

fn doc_stats(doc: &mut Toy, _params: &Value) -> Result<Value, CommandError> {
    Ok(json!({ "layers": doc.layers.len(), "frame": doc.frame }))
}

fn file_import(_doc: &mut Toy, params: &Value) -> Result<Value, CommandError> {
    let path: String = param(params, "path")?;
    Ok(json!({ "would_import": path }))
}

fn file_export_marker(_doc: &mut Toy, params: &Value) -> Result<Value, CommandError> {
    let path: String = param(params, "path")?;
    Ok(json!({ "would_write": path }))
}

fn has_layers(doc: &Toy) -> bool {
    !doc.layers.is_empty()
}

fn layer_clear(doc: &mut Toy, _params: &Value) -> Result<Value, CommandError> {
    doc.layers.clear();
    Ok(Value::Null)
}

pub fn registry() -> CommandRegistry<Toy> {
    let mut registry = CommandRegistry::with_history_commands();
    registry
        .register_all([
            CommandSpec::new("layer.add", "Add Layer", layer_add)
                .in_menu(&["Layer"])
                .with_shortcut("Cmd+Shift+N")
                .describe("Append a layer to the document.")
                .with_schema(r#"{"type":"object","properties":{"name":{"type":"string"},"x":{"type":"number"}},"required":["name"]}"#),
            CommandSpec::new("layer.move", "Move Layer", layer_move)
                .in_menu(&["Layer"])
                .with_schema(r#"{"type":"object","properties":{"index":{"type":"integer","minimum":0},"dx":{"type":"number"}},"required":["index","dx"]}"#),
            CommandSpec::new("layer.fail", "Fail", layer_fail),
            CommandSpec::new("layer.panic", "Panic", layer_panic),
            CommandSpec::new("layer.clear", "Clear Layers", layer_clear).enabled_when(has_layers),
            CommandSpec::new("doc.stats", "Statistics", doc_stats).transient(),
            CommandSpec::new("file.import", "Import", file_import),
            CommandSpec::new("file.export_marker", "Export Marker", file_export_marker).transient(),
            CommandSpec::new("internal.secret", "Secret", doc_stats).transient().hidden_from_mcp(),
        ])
        .unwrap();
    registry
}

/// Hooks for the toy. `windowed` makes screenshots deferred (answered on the next tick), like an app that must wait
/// for a frame.
#[derive(Default)]
pub struct ToyHooks {
    pub windowed: bool,
    pub pending_shot: Option<(Ticket, ScreenshotRequest)>,
    pub export_step_ms: u64,
}

fn gradient(width: u32, height: u32, doc: &Toy) -> Vec<u8> {
    let mut rgba = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let on_layer = doc.layers.iter().any(|l| (l.x as i64 - i64::from(x)).abs() < 3 && (l.y as i64 - i64::from(y)).abs() < 3);
            if on_layer {
                rgba.extend_from_slice(&[255, 80, 0, 255]);
            } else {
                rgba.extend_from_slice(&[(x * 255 / width.max(1)) as u8, (y * 255 / height.max(1)) as u8, 128, 255]);
            }
        }
    }
    rgba
}

impl AppHooks<Toy> for ToyHooks {
    fn status(&self, session: &Session<Toy>) -> Value {
        json!({ "frame": session.doc().frame, "windowed": self.windowed })
    }

    fn queries(&self) -> Vec<QuerySpec> {
        vec![QuerySpec::new("layer_names", "Names of all layers.").with_schema(json!({ "type": "object", "properties": { "prefix": { "type": "string" } } }))]
    }

    fn query(&mut self, session: &Session<Toy>, name: &str, params: &Value, _ticket: &Ticket) -> HookResult {
        let prefix = params.get("prefix").and_then(Value::as_str).unwrap_or("");
        match name {
            "layer_names" => {
                let names: Vec<&str> = session.doc().layers.iter().map(|l| l.name.as_str()).filter(|n| n.starts_with(prefix)).collect();
                Ok(HookOutcome::Ready(Reply::value(json!({ "names": names }))))
            }
            other => Err(ControlError::new(ErrorKind::NotFound, format!("no query {other}"))),
        }
    }

    fn outline(&self, doc: &Toy, depth: Option<u32>) -> Result<Value, ControlError> {
        if depth == Some(0) {
            return Ok(json!({ "title": doc.title, "layers": doc.layers.len() }));
        }
        Ok(
            json!({ "title": doc.title, "frame": doc.frame, "layers": doc.layers.iter().enumerate().map(|(i, l)| json!({ "path": format!("/layers/{i}"), "name": l.name })).collect::<Vec<_>>() }),
        )
    }

    fn library(&self, _doc: &Toy) -> Result<Value, ControlError> {
        Ok(json!({ "symbols": [], "bitmaps": [] }))
    }

    fn get_property(&self, doc: &Toy, path: &str) -> Result<Value, ControlError> {
        let value = serde_json::to_value(doc).map_err(|e| ControlError::failed(e.to_string()))?;
        props::get(&value, &props::parse_path(path)?).cloned()
    }

    fn set_property(&self, doc: &mut Toy, path: &str, new: Value) -> Result<Value, ControlError> {
        let mut value = serde_json::to_value(&*doc).map_err(|e| ControlError::failed(e.to_string()))?;
        props::set(&mut value, &props::parse_path(path)?, new)?;
        *doc = serde_json::from_value(value).map_err(|e| ControlError::invalid(format!("that value does not fit: {e}")))?;
        Ok(Value::Null)
    }

    fn render(&mut self, doc: &Toy, request: &RenderRequest, _ticket: &Ticket) -> HookResult {
        let width = (64.0 * request.scale).round() as u32;
        let height = (36.0 * request.scale).round() as u32;
        let reply = Reply::value(json!({ "frame": request.frame.unwrap_or(doc.frame), "width": width, "height": height })).with_png_rgba(
            width,
            height,
            &gradient(width, height, doc),
        )?;
        Ok(HookOutcome::Ready(reply))
    }

    fn screenshot(&mut self, request: &ScreenshotRequest, ticket: &Ticket) -> HookResult {
        if !self.windowed {
            return Err(ControlError::unsupported("screenshots (this session has no window)"));
        }
        self.pending_shot = Some((ticket.clone(), request.clone()));
        Ok(HookOutcome::Deferred)
    }

    fn export_formats(&self) -> Vec<ExportFormat> {
        vec![ExportFormat::new("json", "JSON", "json")]
    }

    fn export(&mut self, doc: &Toy, request: &ExportRequest, ticket: &Ticket) -> HookResult {
        let steps = 10_u32;
        for step in 0..steps {
            ticket.check_cancelled()?;
            ticket.progress(step as f32 / steps as f32, format!("writing part {step}"));
            std::thread::sleep(Duration::from_millis(self.export_step_ms));
        }
        ticket.check_cancelled()?;
        let text = serde_json::to_string_pretty(doc).map_err(|e| ControlError::failed(e.to_string()))?;
        anim_session::craft_atomic::atomic_write(&request.path, text.as_bytes()).map_err(|e| ControlError::failed(e.to_string()))?;
        Ok(HookOutcome::Ready(Reply::value(json!({ "path": request.path.display().to_string(), "bytes": text.len() }))))
    }

    fn load(&mut self, path: &Path) -> Result<Toy, ControlError> {
        let text = std::fs::read_to_string(path).map_err(|e| ControlError::failed(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| ControlError::failed(format!("not a toy document: {e}")))
    }

    fn new_document(&mut self) -> Result<Toy, ControlError> {
        Ok(Toy::default())
    }

    fn save(&mut self, doc: &Toy, file: &mut File) -> io::Result<()> {
        file.write_all(serde_json::to_string_pretty(doc).map_err(io::Error::other)?.as_bytes())
    }

    fn tick(&mut self, session: &mut Session<Toy>) {
        if let Some((ticket, request)) = self.pending_shot.take() {
            let doc = session.doc();
            let reply = Reply::value(json!({ "panel": request.panel })).with_png_rgba(32, 18, &gradient(32, 18, doc));
            if let (Ok(reply), Some(path)) = (&reply, &request.save_to) {
                if let Some(image) = reply.images.first() {
                    let _ = anim_mcp::save_image(path, image);
                }
            }
            ticket.finish(reply);
        }
    }
}

/// A hand-written JSON-RPC client over an in-memory duplex stream, talking to `AnimMcp`.
pub struct RawClient {
    reader: tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    next_id: u64,
    /// Notifications received while waiting for responses.
    pub notifications: Vec<Value>,
    pub server: tokio::task::JoinHandle<Result<(), anim_mcp::ServeError>>,
    pub init: Value,
}

impl RawClient {
    pub async fn start(backend: anim_mcp::Backend) -> Self {
        let (client_end, server_end) = tokio::io::duplex(1 << 20);
        let mcp = anim_mcp::AnimMcp::new(backend, "toy-mcp", "0.0.1");
        let server = tokio::spawn(anim_mcp::serve_io(mcp, server_end));
        let (read, write) = tokio::io::split(client_end);
        let mut client = Self { reader: tokio::io::BufReader::new(read), writer: write, next_id: 1, notifications: Vec::new(), server, init: Value::Null };
        let init = client
            .request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test-client", "version": "0" } }))
            .await;
        client.init = init;
        client.notify("notifications/initialized", json!({})).await;
        client
    }

    pub async fn send_raw(&mut self, message: &Value) {
        self.send(message).await;
    }

    async fn send(&mut self, message: &Value) {
        use tokio::io::AsyncWriteExt as _;
        let mut text = message.to_string();
        text.push('\n');
        self.writer.write_all(text.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    pub async fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params })).await;
    }

    /// Sends a request and returns the whole response object (with `result` or `error`).
    pub async fn request_raw(&mut self, method: &str, params: Value) -> Value {
        use tokio::io::AsyncBufReadExt as _;
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await;
        loop {
            let mut line = String::new();
            let read =
                tokio::time::timeout(Duration::from_secs(20), self.reader.read_line(&mut line)).await.expect("timed out waiting for the server").unwrap();
            assert!(read > 0, "the server closed the stream");
            let message: Value = serde_json::from_str(line.trim()).unwrap();
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
            self.notifications.push(message);
        }
    }

    /// Like `request_raw` but returns `result` and panics on a protocol error.
    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let response = self.request_raw(method, params).await;
        assert!(response.get("error").is_none(), "protocol error: {response}");
        response["result"].clone()
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({ "name": name, "arguments": arguments })).await
    }
}

pub fn text_of(result: &Value) -> String {
    result["content"].as_array().unwrap().iter().filter(|c| c["type"] == "text").map(|c| c["text"].as_str().unwrap().to_owned()).collect::<Vec<_>>().join("\n")
}

/// A pretend app: a thread that owns the session host and pumps the control inbox like a UI frame loop.
pub struct App {
    pub server_addr: std::net::SocketAddr,
    pub token: String,
    stop: Option<crossbeam_channel::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl App {
    pub fn start(dir: &Path, windowed: bool, export_step_ms: u64, token_file: Option<std::path::PathBuf>) -> Self {
        let server = anim_mcp::ControlServer::start(&anim_mcp::ControlConfig { port: 0, token_file }, || {}).unwrap();
        let (server_addr, token) = (server.addr(), server.token().to_owned());
        let confinement = Confinement::new(&[dir.to_path_buf()], &[dir.to_path_buf()]).unwrap();
        let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);
        let thread = std::thread::spawn(move || {
            let mut host = host_with(confinement, windowed, export_step_ms);
            while stop_rx.try_recv() != Err(crossbeam_channel::TryRecvError::Disconnected) {
                server.inbox().pump(&mut host); // a frame
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Self { server_addr, token, stop: Some(stop_tx), thread: Some(thread) }
    }

    pub fn connect(&self) -> anim_mcp::Backend {
        anim_mcp::connect_bridge(self.server_addr, &self.token).unwrap()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.stop = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn host(confinement: Confinement, windowed: bool) -> SessionHost<Toy, ToyHooks> {
    let session = Session::new(Toy::default(), registry());
    SessionHost::new(HostInfo { name: "toy".into(), version: "0.0.1".into() }, session, ToyHooks { windowed, pending_shot: None, export_step_ms: 0 })
        .with_confinement(confinement)
}

pub fn host_with(confinement: Confinement, windowed: bool, export_step_ms: u64) -> SessionHost<Toy, ToyHooks> {
    let session = Session::new(Toy::default(), registry());
    SessionHost::new(HostInfo { name: "toy".into(), version: "0.0.1".into() }, session, ToyHooks { windowed, pending_shot: None, export_step_ms })
        .with_confinement(confinement)
}
