//! The built-in MCP tools: names, descriptions and JSON Schemas (generated with schemars from the argument types).
//!
//! Each built-in tool maps one-to-one onto a control [`Request`] whose wire parameters have the same names as the
//! tool's arguments, so the front-end converts a call with [`Request::from_wire`]. Command tools (one per registered
//! command) and query tools (`query_<name>`) are generated at run time from the backend and are not listed here.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::protocol::BatchStep;

/// One built-in tool.
#[derive(Debug, Clone, Copy)]
pub struct Builtin {
    /// Tool name.
    pub name: &'static str,
    /// Short title.
    pub title: &'static str,
    /// What it does, written for a model.
    pub description: &'static str,
    /// The control method it calls.
    pub method: &'static str,
    /// Whether it never changes the document.
    pub read_only: bool,
    /// Whether it is destructive (replaces the open document).
    pub destructive: bool,
    schema: fn() -> Value,
}

impl Builtin {
    /// The input schema as a JSON object.
    #[must_use]
    pub fn input_schema(&self) -> Map<String, Value> {
        (self.schema)().as_object().cloned().unwrap_or_default()
    }
}

/// The JSON object schema of `T`, without the `$schema` / `title` noise.
#[must_use]
pub fn schema_of<T: JsonSchema>() -> Value {
    let mut value = serde_json::to_value(schemars::schema_for!(T)).unwrap_or(Value::Null);
    if let Some(map) = value.as_object_mut() {
        map.remove("$schema");
        map.remove("title");
        map.entry("type").or_insert_with(|| Value::String("object".into()));
    }
    value
}

/// No arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoArgs {}

/// Arguments of `list_commands`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListCommandsArgs {
    /// Only commands whose id or label contains this text (case-insensitive).
    pub filter: Option<String>,
    /// Only commands that can run right now.
    pub enabled_only: Option<bool>,
    /// Include each command's parameter schema (large).
    pub schemas: Option<bool>,
}

/// Arguments of `run_command`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunCommandArgs {
    /// Command id such as `timeline.insert_keyframe` (or its tool name `timeline_insert_keyframe`).
    pub id: String,
    /// Parameters object; see `list_commands` with `schemas: true`.
    pub params: Option<Value>,
    /// Folds consecutive calls with the same key into one undo step (nudges, typing).
    pub merge_key: Option<String>,
}

/// Arguments of `run_batch`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunBatchArgs {
    /// Commands to run in order. The batch is atomic: if any step fails nothing changes.
    pub steps: Vec<BatchStep>,
    /// Label of the single undo step the batch creates.
    pub label: Option<String>,
}

/// Arguments of `inspect_document`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectArgs {
    /// Deepest level of the tree to include.
    pub depth: Option<u32>,
}

/// Arguments of `get_property`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPropertyArgs {
    /// Property path, `/layers/0/name` or `layers[0].name`.
    pub path: String,
}

/// Arguments of `set_property`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetPropertyArgs {
    /// Property path, `/layers/0/name` or `layers[0].name`.
    pub path: String,
    /// New value (any JSON).
    pub value: Value,
}

/// Arguments of `query`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryArgs {
    /// Query name from `list_queries`.
    pub name: String,
    /// Parameters object of the query.
    pub params: Option<Value>,
}

/// Arguments of `render_frame`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenderArgs {
    /// Frame index (default: the current frame).
    pub frame: Option<i64>,
    /// Scale factor, 1.0 = document size (default 1.0, at most 16).
    pub scale: Option<f32>,
    /// Also write the PNG to this absolute path (must be inside a write root).
    pub path: Option<String>,
    /// Application-specific options (layer filter, background colour, ...).
    pub options: Option<Value>,
}

/// Arguments of `screenshot`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScreenshotArgs {
    /// Panel to crop to (for example `timeline`); omit for the whole window.
    pub panel: Option<String>,
    /// Also write the PNG to this absolute path (must be inside a write root).
    pub path: Option<String>,
}

/// Arguments of `export`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportArgs {
    /// Format id from `list_export_formats`.
    pub format: String,
    /// Destination, an absolute path inside a write root.
    pub path: String,
    /// Format options; see `list_export_formats`.
    pub options: Option<Value>,
}

/// Arguments of `open_document`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenArgs {
    /// Absolute path of the file to open (inside a read or write root). Replaces the current document.
    pub path: String,
}

/// Arguments of `save_document`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveArgs {
    /// Destination, an absolute path inside a write root. Omit to save to the document's own path.
    pub path: Option<String>,
}

/// Arguments of `journal`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct JournalArgs {
    /// How many of the newest entries to return (default 50).
    pub limit: Option<usize>,
}

/// Every built-in tool, in listing order.
#[must_use]
pub fn builtins() -> Vec<Builtin> {
    macro_rules! tool {
        ($name:literal, $title:literal, $method:literal, ro: $ro:literal, destructive: $d:literal, $args:ty, $desc:literal) => {
            Builtin { name: $name, title: $title, description: $desc, method: $method, read_only: $ro, destructive: $d, schema: schema_of::<$args> }
        };
    }
    vec![
        tool!("session_status", "Session status", "status", ro: true, destructive: false, NoArgs, "Describe the open document: file path, unsaved changes, undo and redo stacks, confinement roots and app-specific state. Call this first."),
        tool!("list_commands", "List commands", "commands.list", ro: true, destructive: false, ListCommandsArgs, "List every editor command with its menu path, shortcut, enabled state and (optionally) parameter schema. Each command is also available as its own typed tool."),
        tool!("run_command", "Run a command", "command.run", ro: false, destructive: false, RunCommandArgs, "Run any editor command by id with a params object. Undoable commands become one undo step."),
        tool!("run_batch", "Run a batch", "batch.run", ro: false, destructive: false, RunBatchArgs, "Run several commands atomically as ONE undo step: if any step fails or is cancelled, the document is left exactly as it was."),
        tool!("inspect_document", "Inspect document", "document.outline", ro: true, destructive: false, InspectArgs, "Return the document tree (timelines, layers, symbols, rigs) down to a depth. Paths in it work with get_property."),
        tool!("get_library", "Get library", "document.library", ro: true, destructive: false, NoArgs, "Return the asset library: symbols, bitmaps, sounds, puppets."),
        tool!("get_property", "Get property", "property.get", ro: true, destructive: false, GetPropertyArgs, "Read a document property by path, for example `/layers/0/name`."),
        tool!("set_property", "Set property", "property.set", ro: false, destructive: false, SetPropertyArgs, "Write a document property by path. One undo step."),
        tool!("list_queries", "List queries", "queries.list", ro: true, destructive: false, NoArgs, "List the application's read-only queries. Each is also a `query_<name>` tool."),
        tool!("query", "Run a query", "query.run", ro: true, destructive: false, QueryArgs, "Run a named read-only query from list_queries."),
        tool!("render_frame", "Render a frame", "render.frame", ro: true, destructive: false, RenderArgs, "Render one frame of the document to a PNG image, independent of the editor view. Returns the image."),
        tool!("screenshot", "Screenshot", "screenshot", ro: true, destructive: false, ScreenshotArgs, "Capture the running application window as a PNG image (not available in headless sessions)."),
        tool!("list_export_formats", "List export formats", "export.formats", ro: true, destructive: false, NoArgs, "List export formats with their option schemas."),
        tool!("export", "Export", "export.run", ro: false, destructive: false, ExportArgs, "Export the document to a file. Long exports report progress and can be cancelled."),
        tool!("undo", "Undo", "history.undo", ro: false, destructive: false, NoArgs, "Undo the last step."),
        tool!("redo", "Redo", "history.redo", ro: false, destructive: false, NoArgs, "Redo the last undone step."),
        tool!("open_document", "Open document", "document.open", ro: false, destructive: true, OpenArgs, "Open a file, replacing the current document (unsaved changes are discarded)."),
        tool!("new_document", "New document", "document.new", ro: false, destructive: true, NoArgs, "Replace the current document by an empty new one (unsaved changes are discarded)."),
        tool!("save_document", "Save document", "document.save", ro: false, destructive: false, SaveArgs, "Save the document atomically, to its own path or to `path`."),
        tool!("journal", "Call journal", "journal.tail", ro: true, destructive: false, JournalArgs, "Show the newest journaled calls: who asked what, whether it worked, how long it took."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Request;
    use serde_json::json;

    #[test]
    fn every_builtin_has_an_object_schema_and_a_known_method() {
        for tool in builtins() {
            let schema = tool.input_schema();
            assert_eq!(schema.get("type"), Some(&json!("object")), "{}", tool.name);
            let params = json!({ "id": "x", "steps": [{"id": "a"}], "name": "n", "path": "/p", "value": 1, "format": "gif" });
            // Required-parameter tools need params to parse; the others parse from nothing.
            let parsed = Request::from_wire(tool.method, Some(&params)).or_else(|_| Request::from_wire(tool.method, None));
            assert!(parsed.is_ok(), "{} -> {}: {parsed:?}", tool.name, tool.method);
        }
    }

    #[test]
    fn tool_names_are_unique_and_valid() {
        let tools = builtins();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), tools.len());
        assert!(tools.iter().all(|t| t.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')));
    }

    #[test]
    fn schemas_describe_fields() {
        let schema = schema_of::<RunCommandArgs>();
        assert!(schema["properties"]["id"]["description"].as_str().unwrap().contains("Command id"));
        assert_eq!(schema["required"], json!(["id"]));
    }
}
