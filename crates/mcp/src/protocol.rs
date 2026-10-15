//! The control protocol: the one vocabulary every backend, the loopback control server and the MCP front-end share.
//!
//! A [`Request`] is what an agent can ask of an editor; a [`Reply`] (a JSON value plus any images) or a
//! [`ControlError`] is what comes back. On the wire (`ControlServer` / `BridgeClient`) a request is one JSON line
//! `{"id": 7, "method": "command.run", "params": {...}}` and the answer is one or more lines carrying the same `id`:
//! `{"id": 7, "progress": {...}}` any number of times, then `{"id": 7, "ok": true, "result": ..., "images": [...]}` or
//! `{"id": 7, "ok": false, "error": {...}}`.

use std::fmt;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Version of the control protocol spoken by this crate; bumped on incompatible changes.
pub const CONTROL_VERSION: u32 = 1;

/// Longest accepted request line, in bytes.
pub const MAX_REQUEST_BYTES: u64 = 8 * 1024 * 1024;

/// Largest image (pixels) [`Reply::with_png_rgba`] will encode.
pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;

/// One step of a batch: a command id (or MCP tool name) and its parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BatchStep {
    /// Command id such as `timeline.insert_keyframe`, or its tool name `timeline_insert_keyframe`.
    pub id: String,
    /// Parameters object; omitted means `{}`.
    #[serde(default)]
    pub params: Value,
}

/// Everything a controller can ask an editor to do.
///
/// The serde form is `{"method": "...", "params": {...}}`, so a request is its own wire message body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    /// Open document, dirty state, undo and redo stacks, app summary.
    #[serde(rename = "status")]
    Status {},
    /// Every command with its schema, menu path, shortcut and enabled state.
    #[serde(rename = "commands.list")]
    CommandList {
        /// Only commands whose id or label contains this text (case-insensitive).
        #[serde(default)]
        filter: Option<String>,
        /// Only commands that can run right now.
        #[serde(default)]
        enabled_only: bool,
        /// Include each command's parameter JSON Schema.
        #[serde(default)]
        schemas: bool,
    },
    /// Runs one command; undoable commands become one undo step.
    #[serde(rename = "command.run")]
    CommandRun {
        /// Command id or tool name.
        id: String,
        /// Parameters object.
        #[serde(default)]
        params: Value,
        /// Folds consecutive calls with the same key into one undo step.
        #[serde(default)]
        merge_key: Option<String>,
    },
    /// Runs several commands atomically: all of them take effect as one undo step, or none does.
    #[serde(rename = "batch.run")]
    Batch {
        /// Steps, in order.
        steps: Vec<BatchStep>,
        /// Undo-step label (default: `Batch of N commands`).
        #[serde(default)]
        label: Option<String>,
    },
    /// The read-only queries the app offers.
    #[serde(rename = "queries.list")]
    QueryList {},
    /// Runs a read-only query.
    #[serde(rename = "query.run")]
    Query {
        /// Query name from `queries.list`.
        name: String,
        /// Parameters object.
        #[serde(default)]
        params: Value,
    },
    /// The document tree.
    #[serde(rename = "document.outline")]
    Outline {
        /// Deepest level to include; omitted means the app's default.
        #[serde(default)]
        depth: Option<u32>,
    },
    /// The asset library (symbols, bitmaps, sounds, rigs).
    #[serde(rename = "document.library")]
    Library {},
    /// Reads a property by path (`layers/0/name` or `layers[0].name`).
    #[serde(rename = "property.get")]
    PropertyGet {
        /// Property path.
        path: String,
    },
    /// Writes a property by path; one undo step.
    #[serde(rename = "property.set")]
    PropertySet {
        /// Property path.
        path: String,
        /// New value.
        value: Value,
    },
    /// Renders one frame of the document, independent of the editor view.
    #[serde(rename = "render.frame")]
    Render {
        /// Frame index; omitted means the current one.
        #[serde(default)]
        frame: Option<i64>,
        /// Scale factor (1.0 = document size).
        #[serde(default)]
        scale: Option<f32>,
        /// Also write the PNG to this file (confined to the write roots).
        #[serde(default)]
        path: Option<String>,
        /// App-specific extras (layer filter, background, ...).
        #[serde(default)]
        options: Value,
    },
    /// A screenshot of the running app window (not available headless).
    #[serde(rename = "screenshot")]
    Screenshot {
        /// Panel or region name to crop to; omitted means the whole window.
        #[serde(default)]
        panel: Option<String>,
        /// Also write the PNG to this file (confined to the write roots).
        #[serde(default)]
        path: Option<String>,
    },
    /// The export formats the app supports.
    #[serde(rename = "export.formats")]
    ExportFormats {},
    /// Exports to a file; long jobs report progress and can be cancelled.
    #[serde(rename = "export.run")]
    Export {
        /// Format id from `export.formats`.
        format: String,
        /// Destination file (confined to the write roots).
        path: String,
        /// Format-specific options.
        #[serde(default)]
        options: Value,
    },
    /// Undoes one step.
    #[serde(rename = "history.undo")]
    Undo {},
    /// Redoes one step.
    #[serde(rename = "history.redo")]
    Redo {},
    /// Replaces the document by one loaded from a file (confined to the read roots).
    #[serde(rename = "document.open")]
    Open {
        /// File to load.
        path: String,
    },
    /// Replaces the document by an empty new one.
    #[serde(rename = "document.new")]
    New {},
    /// Saves the document (confined to the write roots).
    #[serde(rename = "document.save")]
    Save {
        /// Destination; omitted means the document's own path.
        #[serde(default)]
        path: Option<String>,
    },
    /// The newest journal entries (every call is journaled).
    #[serde(rename = "journal.tail")]
    Journal {
        /// How many entries (default 50).
        #[serde(default)]
        limit: Option<usize>,
    },
}

impl Request {
    /// The wire method name, for example `command.run`.
    #[must_use]
    pub fn method(&self) -> String {
        serde_json::to_value(self).ok().and_then(|v| v.get("method").and_then(Value::as_str).map(str::to_owned)).unwrap_or_default()
    }

    /// A short human description for logs and the journal (`command.run edit.rename`).
    #[must_use]
    pub fn summary(&self) -> String {
        let method = self.method();
        match self {
            Self::CommandRun { id, .. } => format!("{method} {id}"),
            Self::Batch { steps, .. } => format!("{method} ({} steps)", steps.len()),
            Self::Query { name, .. } => format!("{method} {name}"),
            Self::PropertyGet { path } | Self::PropertySet { path, .. } | Self::Open { path } => format!("{method} {path}"),
            Self::Export { format, path, .. } => format!("{method} {format} -> {path}"),
            Self::Render { frame, .. } => frame.map_or(method.clone(), |f| format!("{method} frame {f}")),
            _ => method,
        }
    }

    /// Parses a wire `method` / `params` pair.
    ///
    /// # Errors
    /// [`ErrorKind::Unsupported`] for an unknown method, [`ErrorKind::InvalidParams`] when the parameters do not fit.
    pub fn from_wire(method: &str, params: Option<&Value>) -> Result<Self, ControlError> {
        let params = match params {
            None | Some(Value::Null) => Value::Object(Map::new()),
            Some(other) => other.clone(),
        };
        let mut envelope = Map::new();
        envelope.insert("method".into(), Value::String(method.to_owned()));
        envelope.insert("params".into(), params);
        serde_json::from_value(Value::Object(envelope)).map_err(|e| {
            let text = e.to_string();
            if text.contains("unknown variant") {
                ControlError::new(ErrorKind::Unsupported, format!("unknown method {method:?}"))
            } else {
                ControlError::new(ErrorKind::InvalidParams, format!("{method}: {text}"))
            }
        })
    }

    /// Whether the request can only read (used for the MCP read-only hint and for the journal).
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::Status {}
                | Self::CommandList { .. }
                | Self::QueryList {}
                | Self::Query { .. }
                | Self::Outline { .. }
                | Self::Library {}
                | Self::PropertyGet { .. }
                | Self::Render { .. }
                | Self::Screenshot { .. }
                | Self::ExportFormats {}
                | Self::Journal { .. }
        )
    }
}

/// Machine-readable class of a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// No such command, query, export format or property.
    NotFound,
    /// The command exists but cannot run on the current document.
    Disabled,
    /// The parameters had the wrong shape.
    InvalidParams,
    /// The operation ran and failed; the document is unchanged.
    Failed,
    /// A path was outside the allowed roots, or the operation is refused while confined.
    Denied,
    /// The caller cancelled the call.
    Cancelled,
    /// The app does not offer this operation (for example screenshots in a headless session).
    Unsupported,
    /// The editor is busy or went away.
    Unavailable,
    /// The control connection broke.
    Transport,
    /// A bug in the server; the session stays usable.
    Internal,
}

impl ErrorKind {
    /// The `snake_case` name used on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Disabled => "disabled",
            Self::InvalidParams => "invalid_params",
            Self::Failed => "failed",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Transport => "transport",
            Self::Internal => "internal",
        }
    }
}

/// A structured failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub struct ControlError {
    /// Class of failure.
    pub kind: ErrorKind,
    /// Human-readable explanation.
    pub message: String,
    /// Optional extra detail (the failing batch step, the allowed roots, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl ControlError {
    /// An error of `kind`.
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), data: None }
    }

    /// Attaches detail.
    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// [`ErrorKind::Unsupported`].
    #[must_use]
    pub fn unsupported(what: &str) -> Self {
        Self::new(ErrorKind::Unsupported, format!("{what} is not supported by this application"))
    }

    /// [`ErrorKind::Failed`].
    #[must_use]
    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Failed, message)
    }

    /// [`ErrorKind::InvalidParams`].
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidParams, message)
    }

    /// [`ErrorKind::Cancelled`].
    #[must_use]
    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "cancelled by the caller")
    }
}

impl From<anim_session::CommandError> for ControlError {
    fn from(e: anim_session::CommandError) -> Self {
        use anim_session::CommandError as C;
        let kind = match &e {
            C::Unknown(_) => ErrorKind::NotFound,
            C::Disabled(_) => ErrorKind::Disabled,
            C::InvalidParams(_) => ErrorKind::InvalidParams,
            C::Failed(_) => ErrorKind::Failed,
        };
        Self::new(kind, e.to_string())
    }
}

/// An encoded image travelling with a reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    /// Media type, for example `image/png`.
    pub mime: String,
    /// The encoded bytes (base64 on the wire).
    #[serde(with = "b64")]
    pub data: Vec<u8>,
}

/// Base64 (standard alphabet) serde for byte vectors.
mod b64 {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD.decode(text.as_bytes()).map_err(serde::de::Error::custom)
    }
}

impl Image {
    /// The bytes as standard base64 text (what MCP image content carries).
    #[must_use]
    pub fn base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
}

/// A successful answer: a JSON value and any number of images.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Reply {
    /// Structured result.
    pub value: Value,
    /// Images (renders, screenshots), in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Image>,
}

impl Reply {
    /// A reply carrying only `value`.
    #[must_use]
    pub const fn value(value: Value) -> Self {
        Self { value, images: Vec::new() }
    }

    /// Adds an image.
    #[must_use]
    pub fn with_image(mut self, image: Image) -> Self {
        self.images.push(image);
        self
    }

    /// Adds a PNG image encoded from straight-alpha RGBA8 pixels.
    ///
    /// # Errors
    /// [`ErrorKind::InvalidParams`] when the buffer does not match `width * height * 4` or the image is larger than
    /// [`MAX_IMAGE_PIXELS`]; [`ErrorKind::Failed`] when encoding fails.
    pub fn with_png_rgba(self, width: u32, height: u32, rgba: &[u8]) -> Result<Self, ControlError> {
        let png = encode_png_rgba(width, height, rgba)?;
        Ok(self.with_image(Image { mime: "image/png".to_owned(), data: png }))
    }
}

/// Encodes straight-alpha RGBA8 pixels as a PNG.
///
/// # Errors
/// [`ErrorKind::InvalidParams`] for a buffer of the wrong size or an image over [`MAX_IMAGE_PIXELS`];
/// [`ErrorKind::Failed`] when the encoder fails.
pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, ControlError> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0 || height == 0 || pixels > MAX_IMAGE_PIXELS {
        return Err(ControlError::invalid(format!("image size {width}x{height} is outside 1..={MAX_IMAGE_PIXELS} pixels")));
    }
    let expected = pixels.saturating_mul(4);
    if u64::try_from(rgba.len()).ok() != Some(expected) {
        return Err(ControlError::invalid(format!("expected {expected} bytes of RGBA for {width}x{height}, got {}", rgba.len())));
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| ControlError::failed(format!("png: {e}")))?;
    writer.write_image_data(rgba).map_err(|e| ControlError::failed(format!("png: {e}")))?;
    writer.finish().map_err(|e| ControlError::failed(format!("png: {e}")))?;
    Ok(out)
}

/// Progress of a long call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// 0.0 to 1.0.
    pub fraction: f32,
    /// What is happening.
    #[serde(default)]
    pub message: String,
}

/// What a running call reports back, in order: any number of progress events, then exactly one `Done`.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A progress update.
    Progress(Progress),
    /// The final outcome.
    Done(Result<Reply, ControlError>),
}

/// One command as listed by `commands.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInfo {
    /// Command id.
    pub id: String,
    /// MCP tool name.
    pub tool: String,
    /// Menu label.
    pub label: String,
    /// Longer description.
    #[serde(default)]
    pub description: String,
    /// Menu path.
    #[serde(default)]
    pub menu: Vec<String>,
    /// Keyboard shortcut text.
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Whether it can run now.
    pub enabled: bool,
    /// Whether it changes nothing (no undo step).
    pub read_only: bool,
    /// Parameter JSON Schema (when requested).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
}

/// The first line of a bridge conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Shared secret read from the token file.
    pub token: String,
    /// Protocol version the client speaks.
    #[serde(default)]
    pub version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requests_round_trip_through_the_wire_form() {
        let request = Request::CommandRun { id: "edit.rename".into(), params: json!({"name": "x"}), merge_key: None };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["method"], "command.run");
        let back = Request::from_wire("command.run", value.get("params")).unwrap();
        assert_eq!(back, request);
    }

    #[test]
    fn unit_like_requests_parse_without_params() {
        assert_eq!(Request::from_wire("status", None).unwrap(), Request::Status {});
        assert_eq!(Request::from_wire("history.undo", Some(&json!({}))).unwrap(), Request::Undo {});
    }

    #[test]
    fn unknown_method_and_bad_params_are_distinguished() {
        assert_eq!(Request::from_wire("nope", None).unwrap_err().kind, ErrorKind::Unsupported);
        assert_eq!(Request::from_wire("command.run", Some(&json!({"id": 5}))).unwrap_err().kind, ErrorKind::InvalidParams);
    }

    #[test]
    fn png_encoding_checks_sizes() {
        assert!(encode_png_rgba(2, 2, &[0; 16]).unwrap().starts_with(b"\x89PNG"));
        assert_eq!(encode_png_rgba(2, 2, &[0; 15]).unwrap_err().kind, ErrorKind::InvalidParams);
        assert_eq!(encode_png_rgba(0, 2, &[]).unwrap_err().kind, ErrorKind::InvalidParams);
    }

    #[test]
    fn replies_carry_images_as_base64() {
        let reply = Reply::value(json!({"a": 1})).with_png_rgba(1, 1, &[1, 2, 3, 4]).unwrap();
        let text = serde_json::to_string(&reply).unwrap();
        let back: Reply = serde_json::from_str(&text).unwrap();
        assert_eq!(back, reply);
    }
}
