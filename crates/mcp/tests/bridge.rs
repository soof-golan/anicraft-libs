//! The bridge: a control server embedded in a pretend app, and clients talking to it over a loopback socket.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::missing_const_for_fn,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::items_after_statements,
    clippy::manual_contains,
    clippy::bool_assert_comparison,
    clippy::needless_pass_by_value,
    reason = "test code"
)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::thread::JoinHandle;
use std::time::Duration;

use anim_mcp::{Backend, Confinement, ControlConfig, ControlServer, ErrorKind, Request, connect_bridge, connect_bridge_from_file};
use common::{RawClient, host_with};
use crossbeam_channel::{Sender, bounded};
use serde_json::{Value, json};

/// A pretend app: a thread that owns the session host and pumps the control inbox like a UI frame loop.
struct App {
    server_addr: std::net::SocketAddr,
    token: String,
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl App {
    fn start(dir: &std::path::Path, windowed: bool, export_step_ms: u64, token_file: Option<std::path::PathBuf>) -> Self {
        let server = ControlServer::start(&ControlConfig { port: 0, token_file }, || {}).unwrap();
        let (server_addr, token) = (server.addr(), server.token().to_owned());
        let confinement = Confinement::new(&[dir.to_path_buf()], &[dir.to_path_buf()]).unwrap();
        let (stop_tx, stop_rx) = bounded::<()>(1);
        let thread = std::thread::spawn(move || {
            let mut host = host_with(confinement, windowed, export_step_ms);
            while stop_rx.try_recv() != Err(crossbeam_channel::TryRecvError::Disconnected) {
                server.inbox().pump(&mut host); // a frame
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Self { server_addr, token, stop: Some(stop_tx), thread: Some(thread) }
    }

    fn connect(&self) -> Backend {
        connect_bridge(self.server_addr, &self.token).unwrap()
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

fn run(backend: &Backend, id: &str, params: Value) -> Result<anim_mcp::Reply, anim_mcp::ControlError> {
    backend.call(Request::CommandRun { id: id.into(), params, merge_key: None })
}

#[test]
fn requests_round_trip_over_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    let backend = app.connect();
    let added = run(&backend, "layer.add", json!({ "name": "over the wire", "x": 3 })).unwrap();
    assert_eq!(added.value["result"]["index"], 0);
    let status = backend.call(Request::Status {}).unwrap();
    assert_eq!(status.value["undo_steps"], 1);
    assert_eq!(status.value["name"], "toy");
    let error = run(&backend, "layer.fail", json!({})).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Failed);
    let unknown = run(&backend, "nope", json!({})).unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::NotFound);
    // Images travel as base64 on the wire and come back as bytes.
    let render = backend.call(Request::Render { frame: None, scale: Some(1.0), path: None, options: Value::Null }).unwrap();
    assert!(render.images.first().unwrap().data.starts_with(b"\x89PNG"));
    // The journal on the app side knows the connection.
    let journal = backend.call(Request::Journal { limit: Some(3) }).unwrap();
    assert!(journal.value["entries"].as_array().unwrap().iter().all(|e| e["origin"].as_str().unwrap().starts_with("bridge 127.0.0.1")));
}

#[test]
fn several_calls_can_be_in_flight_on_one_connection() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    let backend = app.connect();
    let pending: Vec<_> =
        (0..20).map(|i| backend.submit(Request::CommandRun { id: "layer.add".into(), params: json!({ "name": format!("l{i}") }), merge_key: None })).collect();
    for call in pending {
        call.wait(|_| {}).unwrap();
    }
    let names = backend.call(Request::Query { name: "layer_names".into(), params: json!({}) }).unwrap();
    assert_eq!(names.value["names"].as_array().unwrap().len(), 20);
}

#[test]
fn a_wrong_token_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    let err = connect_bridge(app.server_addr, "not the token").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{err}");
    // Nothing is served without the hello, even a harmless request.
    let mut raw = TcpStream::connect(app.server_addr).unwrap();
    raw.write_all(b"{\"id\":1,\"method\":\"status\"}\n").unwrap();
    let mut reply = String::new();
    BufReader::new(&raw).read_line(&mut reply).unwrap();
    let parsed: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["error"]["kind"], "denied");
}

#[test]
fn the_token_file_is_written_and_usable() {
    let dir = tempfile::tempdir().unwrap();
    let token_file = dir.path().join("control.token");
    let app = App::start(dir.path(), false, 0, Some(token_file.clone()));
    let text = std::fs::read_to_string(&token_file).unwrap();
    assert!(text.starts_with(&app.token));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(std::fs::metadata(&token_file).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let backend = connect_bridge_from_file(&token_file).unwrap();
    assert!(backend.call(Request::Status {}).is_ok());
}

#[test]
fn malformed_lines_close_the_connection_with_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    for garbage in ["GET / HTTP/1.1", "[1,2,3]", "{\"id\":1}", "42"] {
        let mut raw = TcpStream::connect(app.server_addr).unwrap();
        let hello = json!({ "method": "hello", "params": { "token": app.token } });
        raw.write_all(format!("{hello}\n").as_bytes()).unwrap();
        let mut reader = BufReader::new(raw.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains("\"ok\":true"), "{line}");
        raw.write_all(format!("{garbage}\n").as_bytes()).unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["ok"], false, "{garbage}: {line}");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("closing the connection"));
        line.clear();
        assert_eq!(reader.read_line(&mut line).unwrap(), 0, "the connection must be closed after the error");
    }
}

#[test]
fn unknown_methods_and_bad_params_get_errors_but_keep_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    let mut raw = TcpStream::connect(app.server_addr).unwrap();
    let mut reader = BufReader::new(raw.try_clone().unwrap());
    raw.write_all(format!("{}\n", json!({ "method": "hello", "params": { "token": app.token } })).as_bytes()).unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    for (id, method, params, kind) in [(1, "no.such", json!({}), "unsupported"), (2, "command.run", json!({ "id": 5 }), "invalid_params")] {
        raw.write_all(format!("{}\n", json!({ "id": id, "method": method, "params": params })).as_bytes()).unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["id"], id);
        assert_eq!(parsed["error"]["kind"], kind, "{line}");
    }
    raw.write_all(b"{\"id\":3,\"method\":\"status\"}\n").unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("\"ok\":true"), "{line}");
}

#[test]
fn progress_flows_back_and_cancel_reaches_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 60, None);
    let backend = app.connect();
    let out = dir.path().join("out.json");
    let mut call = backend.submit(Request::Export { format: "json".into(), path: out.to_str().unwrap().into(), options: Value::Null });
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let outcome = runtime.block_on(async {
        let mut progress = 0;
        loop {
            match call.next().await {
                Some(anim_mcp::Event::Progress(_)) => {
                    progress += 1;
                    if progress == 2 {
                        call.cancel();
                    }
                }
                Some(anim_mcp::Event::Done(outcome)) => break (progress, outcome),
                None => panic!("the call ended without a result"),
            }
        }
    });
    assert!(outcome.0 >= 2);
    assert_eq!(outcome.1.unwrap_err().kind, ErrorKind::Cancelled);
    assert!(!out.exists());

    // Uncancelled, the same export finishes with progress and writes the file.
    let mut seen = Vec::new();
    let done =
        backend.call_with_progress(Request::Export { format: "json".into(), path: out.to_str().unwrap().into(), options: Value::Null }, &mut seen).unwrap();
    assert_eq!(done.value["path"], out.to_str().unwrap());
    assert!(seen.len() >= 5);
    assert!(out.exists());
}

#[test]
fn dropping_the_connection_cancels_what_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 200, None);
    let out = dir.path().join("abandoned.json");
    {
        let backend = app.connect();
        let call = backend.submit(Request::Export { format: "json".into(), path: out.to_str().unwrap().into(), options: Value::Null });
        std::thread::sleep(Duration::from_millis(300));
        drop(call); // the caller goes away mid-export
        drop(backend);
    }
    let other = app.connect();
    // The actor is single-threaded: this call is served once the abandoned export has stopped (well under 2 s).
    let started = std::time::Instant::now();
    let journal = other.call(Request::Journal { limit: Some(10) }).unwrap();
    assert!(started.elapsed() < Duration::from_millis(1500), "the abandoned export kept running");
    let export = journal.value["entries"].as_array().unwrap().iter().find(|e| e["request"].as_str().unwrap().starts_with("export.run")).unwrap();
    assert_eq!(export["outcome"], "error");
    assert!(!out.exists());
}

#[test]
fn deferred_screenshots_complete_on_a_later_frame() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), true, 0, None);
    let backend = app.connect();
    let target = dir.path().join("shot.png");
    let reply = backend.call(Request::Screenshot { panel: Some("stage".into()), path: Some(target.to_str().unwrap().into()) }).unwrap();
    assert_eq!(reply.value["panel"], "stage");
    assert!(reply.images.first().unwrap().data.starts_with(b"\x89PNG"));
    assert!(target.exists(), "the app wrote the file when it finished the deferred call");
    // Confinement still applies to the requested file.
    let denied = backend.call(Request::Screenshot { panel: None, path: Some("/tmp/anim-mcp-not-allowed.png".into()) }).unwrap_err();
    assert_eq!(denied.kind, ErrorKind::Denied);
}

#[test]
fn the_connection_limit_is_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    let held: Vec<TcpStream> = (0..anim_mcp::server::MAX_CONNECTIONS).map(|_| TcpStream::connect(app.server_addr).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(100));
    let mut extra = TcpStream::connect(app.server_addr).unwrap();
    extra.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut line = String::new();
    BufReader::new(&mut extra).read_line(&mut line).unwrap();
    assert!(line.contains("too many control connections"), "{line}");
    drop(held);
}

#[test]
fn a_dead_app_fails_calls_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let backend = {
        let app = App::start(dir.path(), false, 0, None);
        let backend = app.connect();
        assert!(backend.call(Request::Status {}).is_ok());
        backend
        // the app (server, inbox, host) goes away here
    };
    let error = backend.call(Request::Status {}).unwrap_err();
    assert!(matches!(error.kind, ErrorKind::Transport | ErrorKind::Unavailable), "{error}");
    let again = backend.call(Request::Status {}).unwrap_err();
    assert!(matches!(again.kind, ErrorKind::Transport | ErrorKind::Unavailable), "{again}");
}

#[tokio::test]
async fn mcp_works_end_to_end_over_the_bridge() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), true, 0, None);
    let backend = app.connect();
    let mut client = RawClient::start(backend).await;
    let tools = client.request("tools/list", json!({})).await;
    assert!(tools["tools"].as_array().unwrap().iter().any(|t| t["name"] == "layer_add"));
    let added = client.call_tool("layer_add", json!({ "name": "bridged", "x": 8 })).await;
    assert_eq!(added["isError"], false);
    let render = client.call_tool("render_frame", json!({ "scale": 1.0 })).await;
    assert!(render["content"].as_array().unwrap().iter().any(|c| c["type"] == "image"));
    let shot = client.call_tool("screenshot", json!({})).await;
    assert!(shot["content"].as_array().unwrap().iter().any(|c| c["type"] == "image"), "{shot}");
    let status = client.call_tool("session_status", json!({})).await;
    assert_eq!(status["structuredContent"]["app"]["windowed"], true);
    let batch = client.call_tool("run_batch", json!({ "steps": [{ "id": "layer.add", "params": { "name": "b2" } }, { "id": "layer.fail" }] })).await;
    assert_eq!(batch["structuredContent"]["error"]["data"]["step"], 1);
    let resource = client.request("resources/read", json!({ "uri": "anim://document/outline" })).await;
    assert!(resource["contents"][0]["text"].as_str().unwrap().contains("bridged"));
}
