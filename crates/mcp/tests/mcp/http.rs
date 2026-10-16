//! MCP over streamable HTTP on loopback (feature `http`), with a minimal hand-written HTTP/1.1 client.
#![cfg(feature = "http")]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::missing_const_for_fn,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::needless_pass_by_value,
    clippy::string_slice,
    clippy::format_push_string,
    clippy::match_same_arms,
    reason = "test code"
)]

use crate::common;

use std::time::Duration;

use anim_mcp::http::{HttpListener, serve_http};
use anim_mcp::{AnimMcp, Confinement, spawn_headless};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct Response {
    status: u16,
    headers: String,
    body: String,
}

impl Response {
    fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|l| {
            let (key, value) = l.split_once(':')?;
            key.trim().eq_ignore_ascii_case(name).then(|| value.trim().to_owned())
        })
    }

    /// The first JSON-RPC message in the body (plain JSON or an SSE `data:` event).
    fn json(&self) -> Value {
        let start = self.body.find("{\"").unwrap_or_else(|| panic!("no JSON in {:?}", self.body));
        serde_json::Deserializer::from_str(&self.body[start..]).into_iter::<Value>().next().unwrap().unwrap()
    }
}

async fn post(addr: std::net::SocketAddr, token: Option<&str>, host: &str, session: Option<&str>, body: Value) -> Response {
    let body = body.to_string();
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(session) = session {
        request.push_str(&format!("Mcp-Session-Id: {session}\r\nMcp-Protocol-Version: 2025-06-18\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(&body);
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    // Read until the peer closes or the reply has been quiet for a moment (SSE streams may stay open).
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0_u8; 8192];
        match tokio::time::timeout(Duration::from_millis(600), stream.read(&mut chunk)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => raw.extend_from_slice(&chunk[..n]),
            Ok(Err(_)) => break,
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let headers = head.split_once("\r\n").map_or(String::new(), |(_, rest)| rest.to_owned());
    Response { status, headers, body: body.to_owned() }
}

async fn start(dir: &std::path::Path) -> (std::net::SocketAddr, String, anim_mcp::HeadlessSession) {
    let confinement = Confinement::new(&[dir.to_path_buf()], &[dir.to_path_buf()]).unwrap();
    let session = spawn_headless(move || common::host(confinement, false)).unwrap();
    let mcp = AnimMcp::new(session.backend().clone(), "toy-http", "0.0.1");
    let listener = HttpListener::bind(0).await.unwrap();
    let addr = listener.addr();
    let token = anim_mcp::generate_token().unwrap();
    tokio::spawn(serve_http(mcp, listener, token.clone(), std::future::pending()));
    (addr, token, session)
}

#[tokio::test]
async fn http_requires_the_bearer_token() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, token, _session) = start(dir.path()).await;
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } } });
    let none = post(addr, None, &addr.to_string(), None, init.clone()).await;
    assert_eq!(none.status, 401);
    let wrong = post(addr, Some("nope"), &addr.to_string(), None, init.clone()).await;
    assert_eq!(wrong.status, 401);
    let ok = post(addr, Some(&token), &addr.to_string(), None, init.clone()).await;
    assert_eq!(ok.status, 200, "{}", ok.body);
    // DNS rebinding protection: a non-loopback Host header is refused even with the right token.
    let rebinding = post(addr, Some(&token), "evil.example.com", None, init).await;
    assert_eq!(rebinding.status, 403, "{}", rebinding.body);
}

#[tokio::test]
async fn http_serves_the_same_tools() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, token, _session) = start(dir.path()).await;
    let host = addr.to_string();
    let init = post(addr, Some(&token), &host, None, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } } })).await;
    assert_eq!(init.status, 200, "{}", init.body);
    assert_eq!(init.json()["result"]["serverInfo"]["name"], "toy-http");
    let session = init.header("mcp-session-id");
    let session = session.as_deref();
    let initialized = post(addr, Some(&token), &host, session, json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
    assert!(initialized.status == 202 || initialized.status == 200, "{}", initialized.status);

    let tools = post(addr, Some(&token), &host, session, json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} })).await;
    let names: Vec<String> = tools.json()["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect();
    assert!(names.iter().any(|n| n == "layer_add") && names.iter().any(|n| n == "render_frame"));

    let added = post(
        addr,
        Some(&token),
        &host,
        session,
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "layer_add", "arguments": { "name": "http" } } }),
    )
    .await;
    assert_eq!(added.json()["result"]["isError"], false, "{}", added.body);
    let render = post(
        addr,
        Some(&token),
        &host,
        session,
        json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "render_frame", "arguments": {} } }),
    )
    .await;
    let result = &render.json()["result"];
    assert!(result["content"].as_array().unwrap().iter().any(|c| c["type"] == "image"), "{}", render.body);
}
