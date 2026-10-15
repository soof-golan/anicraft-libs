//! The `anim-mcp` binary as a real MCP stdio server in front of a running (pretend) app, and the in-process mode.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::missing_const_for_fn,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::needless_pass_by_value,
    reason = "test code"
)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use anim_mcp::{Confinement, ControlHost as _, ControlInbox};
use common::{App, RawClient};
use serde_json::{Value, json};

struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    next_id: u64,
}

impl StdioClient {
    fn spawn(args: &[&str]) -> Self {
        let mut child =
            Command::new(env!("CARGO_BIN_EXE_anim-mcp")).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, stdin, lines, next_id: 1 }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self.lines.recv_timeout(Duration::from_secs(20)).expect("no reply from anim-mcp");
            let message: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout must carry only JSON-RPC, got {line:?}: {e}"));
            if message.get("id") == Some(&json!(id)) {
                assert!(message.get("error").is_none(), "{message}");
                return message["result"].clone();
            }
        }
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn the_binary_serves_mcp_over_stdio_in_front_of_a_running_app() {
    let dir = tempfile::tempdir().unwrap();
    let token_file = dir.path().join("control.token");
    let _app = App::start(dir.path(), true, 0, Some(token_file.clone()));
    let mut client = StdioClient::spawn(&["--token-file", token_file.to_str().unwrap(), "--name", "toy-bridge"]);

    let init = client.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }));
    assert_eq!(init["serverInfo"]["name"], "toy-bridge");
    client.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let tools = client.request("tools/list", json!({}));
    assert!(tools["tools"].as_array().unwrap().iter().any(|t| t["name"] == "layer_add"));
    let call = |client: &mut StdioClient, name: &str, args: Value| client.request("tools/call", json!({ "name": name, "arguments": args }));
    assert_eq!(call(&mut client, "layer_add", json!({ "name": "through stdio", "x": 5 }))["isError"], false);
    let render = call(&mut client, "render_frame", json!({}));
    assert!(render["content"].as_array().unwrap().iter().any(|c| c["type"] == "image" && c["mimeType"] == "image/png"));
    let shot = call(&mut client, "screenshot", json!({}));
    assert!(shot["content"].as_array().unwrap().iter().any(|c| c["type"] == "image"));
    let outline = client.request("resources/read", json!({ "uri": "anim://document/outline" }));
    assert!(outline["contents"][0]["text"].as_str().unwrap().contains("through stdio"));
}

#[test]
fn the_binary_reports_a_bad_token_on_stderr_and_exits() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    let output =
        Command::new(env!("CARGO_BIN_EXE_anim-mcp")).args(["--addr", &app.server_addr.to_string(), "--token", "wrong"]).stdin(Stdio::null()).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "nothing may reach stdout before the protocol starts");
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not connect"));
}

#[tokio::test]
async fn in_process_mode_serves_mcp_while_the_ui_thread_owns_the_session() {
    let (inbox, backend) = ControlInbox::in_process(|| {}).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let confinement = Confinement::new(&[dir.path().to_path_buf()], &[dir.path().to_path_buf()]).unwrap();
    let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);
    let ui = std::thread::spawn(move || {
        let mut host = common::host(confinement, false);
        while stop_rx.try_recv() != Err(crossbeam_channel::TryRecvError::Disconnected) {
            inbox.pump(&mut host);
            host.tick();
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    let mut client = RawClient::start(backend).await;
    let added = client.call_tool("layer_add", json!({ "name": "in process" })).await;
    assert_eq!(added["isError"], false);
    let status = client.call_tool("session_status", json!({})).await;
    assert_eq!(status["structuredContent"]["undo_steps"], 1);
    let journal = client.call_tool("journal", json!({})).await;
    assert!(journal["structuredContent"]["entries"].as_array().unwrap().iter().all(|e| e["origin"] == "in-process"));
    drop(stop_tx);
    ui.join().unwrap();
}
