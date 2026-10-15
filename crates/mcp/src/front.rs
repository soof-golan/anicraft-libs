//! [`AnimMcp`]: the MCP server (rmcp) in front of a [`Backend`].
//!
//! The handler owns nothing but a channel to the session owner. Every MCP call becomes a control [`Request`], goes
//! through the backend (an in-process actor or a bridge to a running app) and is answered from the reply. Tools are
//! generated at run time: the built-ins from [`crate::tools`], one typed tool per registered command, and one
//! `query_<name>` tool per application query. Progress events become MCP progress notifications when the client sent a
//! progress token, and a cancelled MCP request cancels the call in the session.

use std::fmt::Write as _;

use rmcp::ErrorData as McpError;
use rmcp::RoleServer;
use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, ProgressNotificationParam, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, ResourceContents,
    ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use serde_json::{Map, Value, json};

use crate::call::Backend;
use crate::protocol::{CommandInfo, ControlError, ErrorKind, Event, Reply, Request};
use crate::tools::{Builtin, builtins};

const STATUS_URI: &str = "anim://session/status";
const OUTLINE_URI: &str = "anim://document/outline";
const LIBRARY_URI: &str = "anim://document/library";
const COMMANDS_URI: &str = "anim://commands";
const JOURNAL_URI: &str = "anim://session/journal";
const PROPERTY_PREFIX: &str = "anim://property/";
const QUERY_TOOL_PREFIX: &str = "query_";

/// The MCP server for one editor session.
#[derive(Debug, Clone)]
pub struct AnimMcp {
    backend: Backend,
    name: String,
    version: String,
    instructions: String,
}

impl AnimMcp {
    /// A server announcing itself as `name` / `version`, answering from `backend`.
    #[must_use]
    pub fn new(backend: Backend, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            backend,
            name: name.into(),
            version: version.into(),
            instructions: "This server drives an animation editor. Call session_status first, then list_commands or inspect_document. Every \
                           editor command is a tool; each edit is one undoable step (undo / redo). run_batch applies several commands atomically \
                           as one step. render_frame and screenshot return images. Paths must be absolute and inside the allowed folders \
                           (session_status shows them)."
                .to_owned(),
        }
    }

    /// Replaces the instructions text clients may show to the model.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    /// The backend, for callers that also want to talk to the session directly.
    #[must_use]
    pub const fn backend(&self) -> &Backend {
        &self.backend
    }

    async fn call(&self, request: Request) -> Result<Reply, ControlError> {
        self.backend.submit(request).finish(|_| {}).await
    }

    /// Every tool this server offers right now.
    ///
    /// # Errors
    /// Fails when the backend cannot list commands.
    pub async fn tools(&self) -> Result<Vec<Tool>, McpError> {
        let mut tools: Vec<Tool> = builtins().iter().map(builtin_tool).collect::<Result<_, _>>()?;
        let taken: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

        let commands = self.call(Request::CommandList { filter: None, enabled_only: false, schemas: true }).await.map_err(|e| internal(&e))?;
        let infos: Vec<CommandInfo> = serde_json::from_value(commands.value.get("commands").cloned().unwrap_or(Value::Null))
            .map_err(|e| McpError::internal_error(format!("bad command list: {e}"), None))?;
        for info in infos {
            if taken.contains(&info.tool) {
                continue;
            }
            tools.push(command_tool(&info)?);
        }

        // Query tools are optional: an app without queries answers with an empty list.
        if let Ok(queries) = self.call(Request::QueryList {}).await {
            let specs: Vec<crate::hooks::QuerySpec> = serde_json::from_value(queries.value.get("queries").cloned().unwrap_or(Value::Null)).unwrap_or_default();
            for spec in specs {
                let name = format!("{QUERY_TOOL_PREFIX}{}", spec.name);
                if taken.contains(&name) {
                    continue;
                }
                let schema = spec.schema.as_object().cloned().unwrap_or_default();
                let tool = to_tool(json!({
                    "name": name,
                    "title": format!("Query: {}", spec.name),
                    "description": spec.description,
                    "inputSchema": ensure_object_schema(schema),
                    "annotations": { "readOnlyHint": true, "destructiveHint": false, "openWorldHint": false },
                    "_meta": { "anim/query": spec.name },
                }))?;
                tools.push(tool);
            }
        }
        Ok(tools)
    }

    /// Turns an MCP tool call into the control request it stands for.
    async fn request_for(&self, name: &str, arguments: Option<Map<String, Value>>) -> Result<Request, McpError> {
        let args = Value::Object(arguments.unwrap_or_default());
        if let Some(builtin) = builtins().into_iter().find(|b| b.name == name) {
            return Request::from_wire(builtin.method, Some(&args)).map_err(|e| McpError::invalid_params(e.message, None));
        }
        if let Some(query) = name.strip_prefix(QUERY_TOOL_PREFIX) {
            let known = self
                .call(Request::QueryList {})
                .await
                .ok()
                .and_then(|r| r.value.get("queries").cloned())
                .and_then(|q| serde_json::from_value::<Vec<crate::hooks::QuerySpec>>(q).ok())
                .unwrap_or_default();
            if known.iter().any(|q| q.name == query) {
                return Ok(Request::Query { name: query.to_owned(), params: args });
            }
        }
        Ok(Request::CommandRun { id: name.to_owned(), params: args, merge_key: None })
    }
}

fn internal(e: &ControlError) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

/// Deserialises a tool from the JSON form (the model types are `non_exhaustive`).
fn to_tool(value: Value) -> Result<Tool, McpError> {
    serde_json::from_value(value).map_err(|e| McpError::internal_error(format!("bad tool definition: {e}"), None))
}

fn ensure_object_schema(mut schema: Map<String, Value>) -> Map<String, Value> {
    schema.entry("type").or_insert_with(|| Value::String("object".into()));
    schema.remove("$schema");
    schema
}

fn builtin_tool(builtin: &Builtin) -> Result<Tool, McpError> {
    to_tool(json!({
        "name": builtin.name,
        "title": builtin.title,
        "description": builtin.description,
        "inputSchema": builtin.input_schema(),
        "annotations": { "readOnlyHint": builtin.read_only, "destructiveHint": builtin.destructive, "openWorldHint": false },
        "_meta": { "anim/builtin": true },
    }))
}

fn command_tool(info: &CommandInfo) -> Result<Tool, McpError> {
    let mut description = if info.description.is_empty() { info.label.clone() } else { info.description.clone() };
    if !info.menu.is_empty() {
        let _ = write!(description, " (menu: {} > {})", info.menu.join(" > "), info.label);
    }
    if !info.enabled {
        description.push_str(" [currently disabled]");
    }
    let schema = info.schema.as_ref().and_then(Value::as_object).cloned().unwrap_or_default();
    to_tool(json!({
        "name": info.tool,
        "title": info.label,
        "description": description,
        "inputSchema": ensure_object_schema(schema),
        "annotations": { "readOnlyHint": info.read_only, "destructiveHint": false, "openWorldHint": false },
        "_meta": { "anim/command": info.id, "anim/enabled": info.enabled, "anim/menu": info.menu, "anim/shortcut": info.shortcut },
    }))
}

/// A reply as an MCP tool result: pretty JSON text, `structuredContent` for objects, image content for images.
fn reply_result(reply: &Reply) -> CallToolResult {
    let text = match &reply.value {
        Value::Null => "ok".to_owned(),
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    let mut content = vec![ContentBlock::text(text)];
    content.extend(reply.images.iter().map(|image| ContentBlock::image(image.base64(), image.mime.clone())));
    let mut result = CallToolResult::success(content);
    if reply.value.is_object() {
        result.structured_content = Some(reply.value.clone());
    }
    result
}

/// A failure as an MCP tool result (`isError`), with the structured error attached.
fn error_result(error: &ControlError) -> CallToolResult {
    let mut result = CallToolResult::error(vec![ContentBlock::text(error.to_string())]);
    result.structured_content = Some(json!({ "error": error }));
    result
}

impl ServerHandler for AnimMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(Implementation::new(self.name.clone(), self.version.clone()))
            .with_instructions(self.instructions.clone())
    }

    async fn list_tools(&self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.tools().await?))
    }

    async fn call_tool(&self, request: CallToolRequestParams, context: RequestContext<RoleServer>) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        let control = self.request_for(&name, request.arguments).await?;
        let is_command = matches!(control, Request::CommandRun { .. }) && !builtins().iter().any(|b| b.name == name);
        let progress_token = context.meta.get_progress_token();

        let mut pending = self.backend.submit(control);
        let outcome = loop {
            tokio::select! {
                () = context.ct.cancelled() => {
                    pending.cancel();
                    return Err(McpError::internal_error("the request was cancelled", None));
                }
                event = pending.next() => match event {
                    Some(Event::Progress(progress)) => {
                        if let Some(token) = &progress_token {
                            let note = ProgressNotificationParam::new(token.clone(), f64::from(progress.fraction)).with_total(1.0).with_message(progress.message);
                            let _ = context.peer.notify_progress(note).await;
                        }
                    }
                    Some(Event::Done(outcome)) => break outcome,
                    None => break Err(ControlError::new(ErrorKind::Unavailable, "the editor dropped the request without answering")),
                },
            }
        };
        match outcome {
            Ok(reply) => Ok(reply_result(&reply).into()),
            Err(error) if is_command && error.kind == ErrorKind::NotFound => Err(McpError::invalid_params(format!("unknown tool {name:?}"), None)),
            Err(error) => Ok(error_result(&error).into()),
        }
    }

    #[expect(clippy::unused_async_trait_impl, reason = "the trait method is async")]
    async fn list_resources(&self, _request: Option<PaginatedRequestParams>, _context: RequestContext<RoleServer>) -> Result<ListResourcesResult, McpError> {
        let resource = |uri: &str, name: &str, description: &str| -> Result<rmcp::model::Resource, McpError> {
            serde_json::from_value(json!({ "uri": uri, "name": name, "description": description, "mimeType": "application/json" }))
                .map_err(|e| McpError::internal_error(e.to_string(), None))
        };
        Ok(ListResourcesResult::with_all_items(vec![
            resource(STATUS_URI, "Session status", "Open document, unsaved changes, undo history and file access roots.")?,
            resource(OUTLINE_URI, "Document outline", "The document tree: timelines, layers, symbols.")?,
            resource(LIBRARY_URI, "Asset library", "Symbols, bitmaps, sounds and rigs in the document.")?,
            resource(COMMANDS_URI, "Commands", "Every editor command with menu path, shortcut and enabled state.")?,
            resource(JOURNAL_URI, "Call journal", "The newest journaled calls.")?,
        ]))
    }

    #[expect(clippy::unused_async_trait_impl, reason = "the trait method is async")]
    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let template: rmcp::model::ResourceTemplate = serde_json::from_value(json!({
            "uriTemplate": format!("{PROPERTY_PREFIX}{{path}}"),
            "name": "Document property",
            "description": "A document property by path, for example anim://property/layers/0/name.",
            "mimeType": "application/json",
        }))
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(ListResourceTemplatesResult::with_all_items(vec![template]))
    }

    async fn read_resource(&self, request: ReadResourceRequestParams, _context: RequestContext<RoleServer>) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri;
        let control = match uri.as_str() {
            STATUS_URI => Request::Status {},
            OUTLINE_URI => Request::Outline { depth: None },
            LIBRARY_URI => Request::Library {},
            COMMANDS_URI => Request::CommandList { filter: None, enabled_only: false, schemas: false },
            JOURNAL_URI => Request::Journal { limit: None },
            other => match other.strip_prefix(PROPERTY_PREFIX) {
                Some(path) => Request::PropertyGet { path: format!("/{path}") },
                None => return Err(McpError::invalid_params(format!("unknown resource {other:?}"), None)),
            },
        };
        let reply = self.call(control).await.map_err(|e| match e.kind {
            ErrorKind::NotFound | ErrorKind::Unsupported | ErrorKind::InvalidParams => McpError::invalid_params(e.message, None),
            _ => internal(&e),
        })?;
        let text = serde_json::to_string_pretty(&reply.value).unwrap_or_default();
        let contents: ResourceContents = serde_json::from_value(json!({ "uri": uri, "mimeType": "application/json", "text": text }))
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![contents]).into())
    }
}
