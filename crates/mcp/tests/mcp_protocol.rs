//! Drives the MCP server over an in-memory transport with raw JSON-RPC against the toy document.
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

use anim_mcp::{Confinement, spawn_headless};
use common::{RawClient, text_of};
use serde_json::{Value, json};

fn confinement(dir: &std::path::Path) -> Confinement {
    Confinement::new(&[dir.to_path_buf()], &[dir.to_path_buf()]).unwrap()
}

async fn start(windowed: bool, export_step_ms: u64, dir: &std::path::Path) -> (RawClient, anim_mcp::HeadlessSession) {
    let confinement = confinement(dir);
    let session = spawn_headless(move || common::host_with(confinement, windowed, export_step_ms)).unwrap();
    let client = RawClient::start(session.backend().clone()).await;
    (client, session)
}

fn tool_names(list: &Value) -> Vec<String> {
    list["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect()
}

#[tokio::test]
async fn initialize_announces_tools_and_resources() {
    let dir = tempfile::tempdir().unwrap();
    let (client, _session) = start(false, 0, dir.path()).await;
    let init = &client.init;
    assert_eq!(init["serverInfo"]["name"], "toy-mcp");
    assert!(init["capabilities"]["tools"].is_object());
    assert!(init["capabilities"]["resources"].is_object());
    assert!(init["instructions"].as_str().unwrap().contains("session_status"));
}

#[tokio::test]
async fn tools_list_has_builtins_commands_and_queries() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    let list = client.request("tools/list", json!({})).await;
    let names = tool_names(&list);
    for builtin in [
        "session_status",
        "list_commands",
        "run_command",
        "run_batch",
        "render_frame",
        "screenshot",
        "export",
        "undo",
        "redo",
        "open_document",
        "save_document",
        "inspect_document",
        "get_property",
        "set_property",
    ] {
        assert!(names.iter().any(|n| n == builtin), "missing builtin {builtin}: {names:?}");
    }
    // One typed tool per command, dots turned into underscores; hidden commands are absent.
    assert!(names.iter().any(|n| n == "layer_add"));
    assert!(names.iter().any(|n| n == "edit_undo"));
    assert!(!names.iter().any(|n| n.contains("secret")));
    assert!(names.iter().any(|n| n == "query_layer_names"));

    let tools = list["tools"].as_array().unwrap();
    let layer_add = tools.iter().find(|t| t["name"] == "layer_add").unwrap();
    assert_eq!(layer_add["inputSchema"]["required"], json!(["name"]));
    assert!(layer_add["description"].as_str().unwrap().contains("Append a layer"));
    assert_eq!(layer_add["annotations"]["readOnlyHint"], false);
    assert_eq!(layer_add["_meta"]["anim/command"], "layer.add");
    assert_eq!(layer_add["_meta"]["anim/shortcut"].as_str().unwrap().to_lowercase().contains("shift"), true);
    let stats = tools.iter().find(|t| t["name"] == "doc_stats").unwrap();
    assert_eq!(stats["annotations"]["readOnlyHint"], true);
    // A command with no schema still gets an object schema.
    let fail = tools.iter().find(|t| t["name"] == "layer_fail").unwrap();
    assert_eq!(fail["inputSchema"]["type"], "object");
    // Disabled commands are listed but flagged.
    let clear = tools.iter().find(|t| t["name"] == "layer_clear").unwrap();
    assert_eq!(clear["_meta"]["anim/enabled"], false);
    assert!(clear["description"].as_str().unwrap().contains("disabled"));
}

#[tokio::test]
async fn command_tools_edit_the_document_and_undo_works() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    let added = client.call_tool("layer_add", json!({ "name": "sun", "x": 4 })).await;
    assert_eq!(added["isError"], false);
    assert_eq!(added["structuredContent"]["result"]["index"], 0);

    let status = client.call_tool("session_status", json!({})).await;
    assert_eq!(status["structuredContent"]["undo_steps"], 1);
    assert_eq!(status["structuredContent"]["dirty"], true);
    assert_eq!(status["structuredContent"]["app"]["frame"], 0);

    let moved = client.call_tool("layer_move", json!({ "index": 0, "dx": 2.5 })).await;
    assert_eq!(moved["structuredContent"]["result"]["x"], 6.5);

    let undone = client.call_tool("undo", json!({})).await;
    assert_eq!(undone["structuredContent"]["undone"], "Move Layer");
    let prop = client.call_tool("get_property", json!({ "path": "/layers/0/x" })).await;
    assert_eq!(prop["structuredContent"]["value"], 4.0);
    let redone = client.call_tool("redo", json!({})).await;
    assert_eq!(redone["structuredContent"]["redone"], "Move Layer");
    assert_eq!(client.call_tool("get_property", json!({ "path": "layers[0].x" })).await["structuredContent"]["value"], 6.5);
}

#[tokio::test]
async fn failures_are_structured_tool_errors_and_leave_the_document_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "keep" })).await;
    let failed = client.call_tool("layer_fail", json!({})).await;
    assert_eq!(failed["isError"], true);
    assert_eq!(failed["structuredContent"]["error"]["kind"], "failed");
    assert!(text_of(&failed).contains("always fails"));
    let names = client.call_tool("query_layer_names", json!({})).await;
    assert_eq!(names["structuredContent"]["names"], json!(["keep"]), "the failed command must be rolled back");

    let bad = client.call_tool("layer_add", json!({})).await;
    assert_eq!(bad["isError"], true);
    assert_eq!(bad["structuredContent"]["error"]["kind"], "invalid_params");

    let disabled = {
        client.call_tool("layer_clear", json!({})).await;
        client.call_tool("layer_clear", json!({})).await
    };
    assert_eq!(disabled["isError"], true);
    assert_eq!(disabled["structuredContent"]["error"]["kind"], "disabled");
}

#[tokio::test]
async fn a_panicking_command_does_not_kill_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    let result = client.call_tool("layer_panic", json!({})).await;
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["error"]["kind"], "internal");
    assert!(text_of(&result).contains("panicked on purpose"));
    let status = client.call_tool("session_status", json!({})).await;
    assert_eq!(status["isError"], false);
}

#[tokio::test]
async fn unknown_tools_and_bad_arguments_are_protocol_errors() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    let unknown = client.request_raw("tools/call", json!({ "name": "no_such_tool", "arguments": {} })).await;
    assert_eq!(unknown["error"]["code"], -32602);
    let bad_args = client.request_raw("tools/call", json!({ "name": "run_command", "arguments": { "id": 5 } })).await;
    assert_eq!(bad_args["error"]["code"], -32602);
    let missing = client.request_raw("tools/call", json!({ "name": "run_batch", "arguments": {} })).await;
    assert!(missing["error"].is_object());
}

#[tokio::test]
async fn run_command_accepts_ids_and_tool_names_and_merge_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("run_command", json!({ "id": "layer.add", "params": { "name": "a" } })).await;
    client.call_tool("run_command", json!({ "id": "layer_move", "params": { "index": 0, "dx": 1 }, "merge_key": "nudge" })).await;
    client.call_tool("run_command", json!({ "id": "layer_move", "params": { "index": 0, "dx": 1 }, "merge_key": "nudge" })).await;
    let status = client.call_tool("session_status", json!({})).await;
    assert_eq!(status["structuredContent"]["undo_steps"], 2, "two nudges with one merge key are one step");
    let unknown = client.call_tool("run_command", json!({ "id": "nope.nothing" })).await;
    assert_eq!(unknown["isError"], true);
    assert_eq!(unknown["structuredContent"]["error"]["kind"], "not_found");
    let hidden = client.call_tool("run_command", json!({ "id": "internal.secret" })).await;
    assert_eq!(hidden["structuredContent"]["error"]["kind"], "not_found");
}

#[tokio::test]
async fn batches_are_atomic_and_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    let ok = client
        .call_tool(
            "run_batch",
            json!({ "label": "Build scene", "steps": [
                { "id": "layer.add", "params": { "name": "a" } },
                { "id": "layer.add", "params": { "name": "b" } },
                { "id": "layer_move", "params": { "index": 1, "dx": 3 } },
            ] }),
        )
        .await;
    assert_eq!(ok["isError"], false, "{ok}");
    assert_eq!(ok["structuredContent"]["steps"].as_array().unwrap().len(), 3);
    let status = client.call_tool("session_status", json!({})).await;
    assert_eq!(status["structuredContent"]["undo_steps"], 1);
    assert_eq!(status["structuredContent"]["undo"][0], "Build scene");

    // A failing step leaves everything as it was, including the steps that ran before it.
    let failed = client
        .call_tool(
            "run_batch",
            json!({ "steps": [
            { "id": "layer.add", "params": { "name": "c" } },
            { "id": "layer.fail" },
            { "id": "layer.add", "params": { "name": "d" } },
        ] }),
        )
        .await;
    assert_eq!(failed["isError"], true);
    assert_eq!(failed["structuredContent"]["error"]["data"]["step"], 1);
    assert_eq!(failed["structuredContent"]["error"]["data"]["applied"], false);
    let names = client.call_tool("query_layer_names", json!({})).await;
    assert_eq!(names["structuredContent"]["names"], json!(["a", "b"]));
    assert_eq!(client.call_tool("session_status", json!({})).await["structuredContent"]["undo_steps"], 1);

    // One undo reverts the whole successful batch.
    client.call_tool("undo", json!({})).await;
    let names = client.call_tool("query_layer_names", json!({})).await;
    assert_eq!(names["structuredContent"]["names"], json!([]));

    // Undo/redo inside a batch, unknown commands and empty batches are refused.
    for steps in [json!([{ "id": "edit.undo" }]), json!([{ "id": "nope" }]), json!([])] {
        let refused = client.call_tool("run_batch", json!({ "steps": steps })).await;
        assert_eq!(refused["isError"], true);
    }
}

#[tokio::test]
async fn read_only_batches_record_no_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("run_batch", json!({ "steps": [{ "id": "doc.stats" }, { "id": "doc.stats" }] })).await;
    assert_eq!(client.call_tool("session_status", json!({})).await["structuredContent"]["undo_steps"], 0);
}

#[tokio::test]
async fn properties_get_and_set_by_path_as_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "a" })).await;
    let set = client.call_tool("set_property", json!({ "path": "/layers/0/name", "value": "renamed" })).await;
    assert_eq!(set["isError"], false, "{set}");
    assert_eq!(client.call_tool("get_property", json!({ "path": "/layers/0/name" })).await["structuredContent"]["value"], "renamed");
    let status = client.call_tool("session_status", json!({})).await;
    assert_eq!(status["structuredContent"]["undo"][0], "Set /layers/0/name");
    // A value of the wrong type is refused and nothing changes.
    let bad = client.call_tool("set_property", json!({ "path": "/layers/0/x", "value": "text" })).await;
    assert_eq!(bad["isError"], true);
    assert_eq!(client.call_tool("session_status", json!({})).await["structuredContent"]["undo_steps"], 2);
    let missing = client.call_tool("get_property", json!({ "path": "/layers/9/name" })).await;
    assert_eq!(missing["structuredContent"]["error"]["kind"], "not_found");
}

#[tokio::test]
async fn render_frame_returns_png_image_content() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "dot", "x": 10 })).await;
    let target = dir.path().join("frame.png");
    let result = client.call_tool("render_frame", json!({ "frame": 3, "scale": 2.0, "path": target.to_str().unwrap() })).await;
    assert_eq!(result["isError"], false, "{result}");
    let image = result["content"].as_array().unwrap().iter().find(|c| c["type"] == "image").expect("image content");
    assert_eq!(image["mimeType"], "image/png");
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD.decode(image["data"].as_str().unwrap()).unwrap();
    assert!(png.starts_with(b"\x89PNG"));
    assert_eq!(result["structuredContent"]["width"], 128);
    assert_eq!(result["structuredContent"]["frame"], 3);
    assert_eq!(std::fs::read(&target).unwrap(), png, "the PNG is also written to the requested file");

    let bad_scale = client.call_tool("render_frame", json!({ "scale": -1.0 })).await;
    assert_eq!(bad_scale["structuredContent"]["error"]["kind"], "invalid_params");
}

#[tokio::test]
async fn screenshot_is_unsupported_headless_and_deferred_in_a_window() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    let headless = client.call_tool("screenshot", json!({})).await;
    assert_eq!(headless["isError"], true);
    assert_eq!(headless["structuredContent"]["error"]["kind"], "unsupported");

    let (mut windowed, _session2) = start(true, 0, dir.path()).await;
    let shot = windowed.call_tool("screenshot", json!({ "panel": "timeline" })).await;
    assert_eq!(shot["isError"], false, "{shot}");
    assert!(shot["content"].as_array().unwrap().iter().any(|c| c["type"] == "image"));
    assert_eq!(shot["structuredContent"]["panel"], "timeline");
}

#[tokio::test]
async fn path_confinement_applies_to_files_commands_and_exports() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;

    let inside = dir.path().join("a.json");
    let allowed = client.call_tool("save_document", json!({ "path": inside.to_str().unwrap() })).await;
    assert_eq!(allowed["isError"], false, "{allowed}");
    assert!(inside.exists());

    let out_path = outside.path().join("b.json");
    let denied = client.call_tool("save_document", json!({ "path": out_path.to_str().unwrap() })).await;
    assert_eq!(denied["structuredContent"]["error"]["kind"], "denied");
    assert!(!out_path.exists());

    // Command parameters that look like paths are checked too, read vs write by command id.
    let read_denied = client.call_tool("file_import", json!({ "path": "/etc/hostname" })).await;
    assert_eq!(read_denied["structuredContent"]["error"]["kind"], "denied");
    let read_ok = client.call_tool("file_import", json!({ "path": inside.to_str().unwrap() })).await;
    assert_eq!(read_ok["isError"], false);
    let write_denied = client.call_tool("file_export_marker", json!({ "path": out_path.to_str().unwrap() })).await;
    assert_eq!(write_denied["structuredContent"]["error"]["kind"], "denied");
    let dotdot = format!("{}/../escape.json", dir.path().display());
    let escape = client.call_tool("export", json!({ "format": "json", "path": dotdot })).await;
    assert_eq!(escape["structuredContent"]["error"]["kind"], "denied");
    let relative = client.call_tool("open_document", json!({ "path": "a.json" })).await;
    assert_eq!(relative["structuredContent"]["error"]["kind"], "denied");
    let render_out = client.call_tool("render_frame", json!({ "path": out_path.to_str().unwrap() })).await;
    assert_eq!(render_out["structuredContent"]["error"]["kind"], "denied");
    let batch_denied = client
        .call_tool(
            "run_batch",
            json!({ "steps": [{ "id": "layer.add", "params": { "name": "x" } }, { "id": "file.import", "params": { "path": "/etc/hostname" } }] }),
        )
        .await;
    assert_eq!(batch_denied["structuredContent"]["error"]["kind"], "denied");
    assert_eq!(client.call_tool("query_layer_names", json!({})).await["structuredContent"]["names"], json!([]));
}

#[tokio::test]
async fn open_save_new_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "persisted" })).await;
    let path = dir.path().join("doc.json");
    client.call_tool("save_document", json!({ "path": path.to_str().unwrap() })).await;
    let status = client.call_tool("session_status", json!({})).await;
    assert_eq!(status["structuredContent"]["dirty"], false);
    assert_eq!(status["structuredContent"]["path"], path.to_str().unwrap());
    client.call_tool("new_document", json!({})).await;
    assert_eq!(client.call_tool("query_layer_names", json!({})).await["structuredContent"]["names"], json!([]));
    let opened = client.call_tool("open_document", json!({ "path": path.to_str().unwrap() })).await;
    assert_eq!(opened["isError"], false, "{opened}");
    assert_eq!(client.call_tool("query_layer_names", json!({})).await["structuredContent"]["names"], json!(["persisted"]));
    // Saving again with no path uses the document's own.
    client.call_tool("layer_add", json!({ "name": "more" })).await;
    assert_eq!(client.call_tool("save_document", json!({})).await["isError"], false);
    assert!(std::fs::read_to_string(&path).unwrap().contains("more"));
    // Opening garbage reports a failure and keeps the document.
    let junk = dir.path().join("junk.json");
    std::fs::write(&junk, "not json").unwrap();
    let bad = client.call_tool("open_document", json!({ "path": junk.to_str().unwrap() })).await;
    assert_eq!(bad["isError"], true);
    assert_eq!(client.call_tool("query_layer_names", json!({})).await["structuredContent"]["names"], json!(["persisted", "more"]));
}

#[tokio::test]
async fn export_reports_progress_notifications() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 5, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "a" })).await;
    let out = dir.path().join("out.json");
    let response = client
        .request(
            "tools/call",
            json!({ "name": "export", "arguments": { "format": "json", "path": out.to_str().unwrap() }, "_meta": { "progressToken": "export-1" } }),
        )
        .await;
    assert_eq!(response["isError"], false, "{response}");
    assert!(out.exists());
    let progress: Vec<&Value> = client.notifications.iter().filter(|n| n["method"] == "notifications/progress").collect();
    assert!(progress.len() >= 3, "expected progress notifications, got {:?}", client.notifications);
    assert!(progress.iter().all(|n| n["params"]["progressToken"] == "export-1"));
    let fractions: Vec<f64> = progress.iter().map(|n| n["params"]["progress"].as_f64().unwrap()).collect();
    assert!(fractions.windows(2).all(|w| w[0] <= w[1]), "progress must not go backwards: {fractions:?}");
    assert!(progress.iter().any(|n| n["params"]["message"].as_str().is_some_and(|m| m.starts_with("writing part"))));
}

#[tokio::test]
async fn cancelling_a_long_export_stops_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 100, dir.path()).await;
    let out = dir.path().join("never.json");
    // Send the call by hand so we can cancel it before it answers.
    let id = 777;
    let call = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": "export", "arguments": { "format": "json", "path": out.to_str().unwrap() } } });
    client.send_raw(&call).await;
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    client.notify("notifications/cancelled", json!({ "requestId": id, "reason": "changed my mind" })).await;
    // The export must stop: the journal shows an error (cancelled) well before the 1 s the export would take.
    let mut journal = Value::Null;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        journal = client.call_tool("journal", json!({ "limit": 5 })).await;
        if journal["structuredContent"]["entries"]
            .as_array()
            .is_some_and(|e| e.iter().any(|x| x["request"].as_str().is_some_and(|r| r.starts_with("export.run"))))
        {
            break;
        }
    }
    let entries = journal["structuredContent"]["entries"].as_array().unwrap();
    let export = entries.iter().find(|e| e["request"].as_str().unwrap().starts_with("export.run")).expect("the export was journaled");
    assert_eq!(export["outcome"], "error");
    assert!(export["error"].as_str().unwrap().contains("cancelled"), "{export}");
    assert!(!out.exists(), "a cancelled export writes nothing");
}

#[tokio::test]
async fn resources_expose_status_outline_library_commands_and_journal() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "a" })).await;
    let list = client.request("resources/list", json!({})).await;
    let uris: Vec<&str> = list["resources"].as_array().unwrap().iter().map(|r| r["uri"].as_str().unwrap()).collect();
    for uri in ["anim://session/status", "anim://document/outline", "anim://document/library", "anim://commands", "anim://session/journal"] {
        assert!(uris.contains(&uri), "{uri} missing from {uris:?}");
    }
    let outline = client.request("resources/read", json!({ "uri": "anim://document/outline" })).await;
    let text = outline["contents"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["layers"][0]["name"], "a");
    assert_eq!(outline["contents"][0]["mimeType"], "application/json");

    let templates = client.request("resources/templates/list", json!({})).await;
    assert!(templates["resourceTemplates"][0]["uriTemplate"].as_str().unwrap().starts_with("anim://property/"));
    let property = client.request("resources/read", json!({ "uri": "anim://property/layers/0/name" })).await;
    assert_eq!(serde_json::from_str::<Value>(property["contents"][0]["text"].as_str().unwrap()).unwrap()["value"], "a");

    let commands = client.request("resources/read", json!({ "uri": "anim://commands" })).await;
    assert!(commands["contents"][0]["text"].as_str().unwrap().contains("layer.add"));
    let unknown = client.request_raw("resources/read", json!({ "uri": "anim://nothing" })).await;
    assert!(unknown["error"].is_object());
}

#[tokio::test]
async fn every_call_is_journaled() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "a" })).await;
    client.call_tool("layer_fail", json!({})).await;
    client.call_tool("render_frame", json!({})).await;
    let journal = client.call_tool("journal", json!({})).await;
    let entries = journal["structuredContent"]["entries"].as_array().unwrap();
    let requests: Vec<&str> = entries.iter().map(|e| e["request"].as_str().unwrap()).collect();
    // Command tools journal the name they were called by (`layer_add`), run_command the command id.
    assert!(requests.iter().any(|r| *r == "command.run layer_add"), "{requests:?}");
    assert!(requests.iter().any(|r| *r == "command.run layer_fail"));
    assert!(requests.iter().any(|r| r.starts_with("render.frame")));
    let failed = entries.iter().find(|e| e["request"] == "command.run layer_fail").unwrap();
    assert_eq!(failed["outcome"], "error");
    assert!(entries.iter().all(|e| e["origin"] == "headless"));
    assert!(entries.windows(2).all(|w| w[0]["seq"].as_u64() < w[1]["seq"].as_u64()));
}

#[tokio::test]
async fn list_commands_filters_and_includes_schemas() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    let all = client.call_tool("list_commands", json!({ "filter": "layer", "schemas": true })).await;
    let commands = all["structuredContent"]["commands"].as_array().unwrap();
    assert!(commands.iter().all(|c| c["id"].as_str().unwrap().contains("layer") || c["label"].as_str().unwrap().to_lowercase().contains("layer")));
    let add = commands.iter().find(|c| c["id"] == "layer.add").unwrap();
    assert_eq!(add["tool"], "layer_add");
    assert_eq!(add["menu"], json!(["Layer"]));
    assert!(add["schema"]["properties"]["name"].is_object());
    let enabled = client.call_tool("list_commands", json!({ "enabled_only": true })).await;
    assert!(!enabled["structuredContent"]["commands"].as_array().unwrap().iter().any(|c| c["id"] == "layer.clear"));
}

#[tokio::test]
async fn inspect_document_and_queries() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "alpha" })).await;
    client.call_tool("layer_add", json!({ "name": "beta" })).await;
    let shallow = client.call_tool("inspect_document", json!({ "depth": 0 })).await;
    assert_eq!(shallow["structuredContent"]["layers"], 2);
    let deep = client.call_tool("inspect_document", json!({})).await;
    assert_eq!(deep["structuredContent"]["layers"][1]["path"], "/layers/1");
    let filtered = client.call_tool("query", json!({ "name": "layer_names", "params": { "prefix": "b" } })).await;
    assert_eq!(filtered["structuredContent"]["names"], json!(["beta"]));
    let listed = client.call_tool("list_queries", json!({})).await;
    assert_eq!(listed["structuredContent"]["queries"][0]["name"], "layer_names");
    let unknown = client.call_tool("query", json!({ "name": "nope" })).await;
    assert_eq!(unknown["structuredContent"]["error"]["kind"], "not_found");
    assert_eq!(client.call_tool("get_library", json!({})).await["isError"], false);
    assert_eq!(client.call_tool("list_export_formats", json!({})).await["structuredContent"]["formats"][0]["id"], "json");
}

#[tokio::test]
async fn prompts_are_listed_and_filled_in() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    assert!(client.init["capabilities"]["prompts"].is_object());
    let list = client.request("prompts/list", json!({})).await;
    let names: Vec<&str> = list["prompts"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"getting_started") && names.contains(&"make_change"));
    let prompt = client.request("prompts/get", json!({ "name": "make_change", "arguments": { "goal": "move the sun right" } })).await;
    let text = prompt["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.contains("move the sun right") && text.contains("run_batch"));
    assert_eq!(prompt["messages"][0]["role"], "user");
    let missing = client.request_raw("prompts/get", json!({ "name": "make_change" })).await;
    assert!(missing["error"].is_object());
    let unknown = client.request_raw("prompts/get", json!({ "name": "nope" })).await;
    assert!(unknown["error"].is_object());
}

#[tokio::test]
async fn ping_works() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start(false, 0, dir.path()).await;
    let pong = client.request_raw("ping", json!({})).await;
    assert!(pong.get("error").is_none(), "{pong}");
}

async fn start_threaded(step_ms: u64, dir: &std::path::Path) -> (RawClient, anim_mcp::HeadlessSession) {
    let confinement = confinement(dir);
    let session = spawn_headless(move || common::host_threaded(confinement, step_ms)).unwrap();
    let client = RawClient::start(session.backend().clone()).await;
    (client, session)
}

#[tokio::test]
async fn background_jobs_keep_the_session_responsive_and_report_progress() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start_threaded(60, dir.path()).await;
    client.call_tool("layer_add", json!({ "name": "a" })).await;
    let out = dir.path().join("job.json");
    let call = json!({ "jsonrpc": "2.0", "id": 900, "method": "tools/call", "params": {
        "name": "export", "arguments": { "format": "json", "path": out.to_str().unwrap() }, "_meta": { "progressToken": 5 } } });
    client.send_raw(&call).await;
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    // While the export runs on its worker, other requests are answered at once and the document can still change.
    let started = std::time::Instant::now();
    let status = client.call_tool("session_status", json!({})).await;
    assert!(started.elapsed() < std::time::Duration::from_millis(400), "status waited for the export");
    assert_eq!(status["isError"], false);
    client.call_tool("layer_add", json!({ "name": "b" })).await;
    let answer = client.wait_for_response(900).await;
    assert_eq!(answer["result"]["isError"], false, "{answer}");
    assert!(out.exists());
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(saved["layers"].as_array().unwrap().len(), 1, "the job worked on a copy taken when it started");
    let progress = client.notifications.iter().filter(|n| n["method"] == "notifications/progress" && n["params"]["progressToken"] == 5).count();
    assert!(progress >= 3, "{:?}", client.notifications);
    let journal = client.call_tool("journal", json!({})).await;
    let entries = journal["structuredContent"]["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["request"].as_str().unwrap().starts_with("export.run") && e["outcome"] == "deferred"));
}

#[tokio::test]
async fn cancelling_a_background_job_stops_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut client, _session) = start_threaded(100, dir.path()).await;
    let out = dir.path().join("cancelled.json");
    let call = json!({ "jsonrpc": "2.0", "id": 901, "method": "tools/call", "params": { "name": "export", "arguments": { "format": "json", "path": out.to_str().unwrap() } } });
    client.send_raw(&call).await;
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    client.notify("notifications/cancelled", json!({ "requestId": 901 })).await;
    tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
    assert!(!out.exists(), "a cancelled job must not finish its write");
    assert_eq!(client.call_tool("session_status", json!({})).await["isError"], false);
}
