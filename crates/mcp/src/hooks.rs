//! The plug-in surface for an application: everything `anim-mcp` cannot know about a document.
//!
//! `anim-mcp` stays generic. It knows commands (from the `CommandRegistry`), undo, files and jobs. The application adds
//! what only it can do by implementing [`AppHooks`]: read-only queries, the document outline and library, property
//! access, rendering, exporting, loading and saving. Every method has a default that answers `unsupported`, so a
//! first version can implement a handful and grow.
//!
//! Methods that may take more than one frame (a screenshot waits for the next frame to be drawn, an export runs on a
//! worker) receive the call's [`Ticket`]. They either return [`HookOutcome::Ready`], or clone the ticket, return
//! [`HookOutcome::Deferred`] and finish the ticket later, reporting [`Ticket::progress`] and honouring
//! [`Ticket::is_cancelled`] on the way.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use anim_session::{Session, SizeHint};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::call::Ticket;
use crate::confine::{Access, default_path_access};
use crate::protocol::{ControlError, Image, Reply};

/// How a hook ended.
#[derive(Debug)]
pub enum HookOutcome {
    /// The answer is ready.
    Ready(Reply),
    /// The hook kept a clone of the ticket and will finish it later.
    Deferred,
}

/// Result of a hook.
pub type HookResult = Result<HookOutcome, ControlError>;

/// A read-only query the application offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuerySpec {
    /// Name used in `query.run` (`snake_case`, becomes the tool `query_<name>`).
    pub name: String,
    /// What it returns.
    pub description: String,
    /// JSON Schema of its parameters.
    pub schema: Value,
}

impl QuerySpec {
    /// A query with an empty-object schema.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self { name: name.into(), description: description.into(), schema: serde_json::json!({ "type": "object", "properties": {} }) }
    }

    /// Sets the parameter schema.
    #[must_use]
    pub fn with_schema(mut self, schema: Value) -> Self {
        self.schema = schema;
        self
    }
}

/// An export format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFormat {
    /// Id used in `export.run` (`gif`, `png-sequence`, `mp4`).
    pub id: String,
    /// Human name.
    pub label: String,
    /// File extension without the dot (empty for folder outputs).
    pub extension: String,
    /// Notes for the agent.
    pub description: String,
    /// JSON Schema of the format's `options`.
    pub schema: Value,
}

impl ExportFormat {
    /// A format with an empty option schema.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>, extension: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            extension: extension.into(),
            description: String::new(),
            schema: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }
}

/// Parameters of a render, already validated by the host.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderRequest {
    /// Frame to render; `None` means the current frame.
    pub frame: Option<i64>,
    /// Scale factor, finite and positive (default 1.0).
    pub scale: f32,
    /// Where to also write the PNG (already confined), for hooks that defer.
    pub save_to: Option<PathBuf>,
    /// App-specific extras.
    pub options: Value,
}

/// Parameters of a screenshot, already validated by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotRequest {
    /// Panel to crop to.
    pub panel: Option<String>,
    /// Where to also write the PNG (already confined). The host writes it for `Ready` replies; a hook that defers must
    /// write it itself, for example with [`save_image`].
    pub save_to: Option<PathBuf>,
}

/// Parameters of an export, already validated by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    /// A format id the hooks listed.
    pub format: String,
    /// Destination (already confined).
    pub path: PathBuf,
    /// Format options.
    pub options: Value,
}

/// What an application provides to the control layer. `D` is its document type.
pub trait AppHooks<D: Clone + SizeHint> {
    /// Extra fields for `status` (current frame, selected tool, ...), merged under `"app"`.
    fn status(&self, session: &Session<D>) -> Value {
        let _ = session;
        Value::Null
    }

    /// Read-only queries offered as `query.run` and as `query_<name>` tools.
    fn queries(&self) -> Vec<QuerySpec> {
        Vec::new()
    }

    /// Runs a query. Must not change the document.
    ///
    /// # Errors
    /// Whatever the query reports.
    fn query(&mut self, session: &Session<D>, name: &str, params: &Value, ticket: &Ticket) -> HookResult {
        let _ = (session, params, ticket);
        Err(ControlError::new(crate::protocol::ErrorKind::NotFound, format!("no query named {name:?}")))
    }

    /// The document tree down to `depth` levels (app default when `None`).
    ///
    /// # Errors
    /// [`crate::ErrorKind::Unsupported`] unless implemented.
    fn outline(&self, doc: &D, depth: Option<u32>) -> Result<Value, ControlError> {
        let _ = (doc, depth);
        Err(ControlError::unsupported("the document outline"))
    }

    /// The asset library.
    ///
    /// # Errors
    /// [`crate::ErrorKind::Unsupported`] unless implemented.
    fn library(&self, doc: &D) -> Result<Value, ControlError> {
        let _ = doc;
        Err(ControlError::unsupported("the asset library"))
    }

    /// Reads a property by path; see [`crate::props`] for helpers over JSON-shaped data.
    ///
    /// # Errors
    /// [`crate::ErrorKind::NotFound`] for an unknown path.
    fn get_property(&self, doc: &D, path: &str) -> Result<Value, ControlError> {
        let _ = (doc, path);
        Err(ControlError::unsupported("reading properties"))
    }

    /// Writes a property by path. The host runs it on a copy and records one undo step on success.
    ///
    /// # Errors
    /// The failure; the document is left unchanged.
    fn set_property(&self, doc: &mut D, path: &str, value: Value) -> Result<Value, ControlError> {
        let _ = (doc, path, value);
        Err(ControlError::unsupported("writing properties"))
    }

    /// Renders a frame.
    ///
    /// # Errors
    /// [`crate::ErrorKind::Unsupported`] unless implemented.
    fn render(&mut self, doc: &D, request: &RenderRequest, ticket: &Ticket) -> HookResult {
        let _ = (doc, request, ticket);
        Err(ControlError::unsupported("rendering"))
    }

    /// Captures the application window. Headless sessions keep this default.
    ///
    /// # Errors
    /// [`crate::ErrorKind::Unsupported`] unless implemented.
    fn screenshot(&mut self, request: &ScreenshotRequest, ticket: &Ticket) -> HookResult {
        let _ = (request, ticket);
        Err(ControlError::unsupported("screenshots (this session has no window)"))
    }

    /// The export formats.
    fn export_formats(&self) -> Vec<ExportFormat> {
        Vec::new()
    }

    /// Exports. Long exports report progress and check for cancellation through the ticket.
    ///
    /// # Errors
    /// The failure, or [`crate::ErrorKind::Cancelled`].
    fn export(&mut self, doc: &D, request: &ExportRequest, ticket: &Ticket) -> HookResult {
        let _ = (doc, request, ticket);
        Err(ControlError::unsupported("exporting"))
    }

    /// Loads a document from a file the host has already confined.
    ///
    /// # Errors
    /// The failure.
    fn load(&mut self, path: &Path) -> Result<D, ControlError> {
        let _ = path;
        Err(ControlError::unsupported("opening files"))
    }

    /// A fresh empty document.
    ///
    /// # Errors
    /// [`crate::ErrorKind::Unsupported`] unless implemented.
    fn new_document(&mut self) -> Result<D, ControlError> {
        Err(ControlError::unsupported("creating documents"))
    }

    /// Writes the document to `file` (the host stages it and replaces the target atomically).
    ///
    /// # Errors
    /// I/O errors from writing.
    fn save(&mut self, doc: &D, file: &mut File) -> io::Result<()> {
        let _ = (doc, file);
        Err(io::Error::new(io::ErrorKind::Unsupported, "saving is not supported by this application"))
    }

    /// Which parameter keys name files, so the host can confine them. The default treats keys such as `path`, `file`,
    /// `folder` or `*_path` as file names (writes for commands that save or export).
    fn path_access(&self, command: &str, key: &str) -> Option<Access> {
        default_path_access(command, key)
    }

    /// Commands that must not run while paths are confined (they run code, open other programs, ...).
    fn refused_while_confined(&self, command: &str) -> bool {
        let _ = command;
        false
    }

    /// Called regularly by the owner of the session (once per frame in an app) for deferred work.
    fn tick(&mut self, session: &mut Session<D>) {
        let _ = session;
    }
}

/// Runs `work` on its own thread and finishes the call with its result: the way for a hook to do a long job (an export,
/// a render sequence) without blocking the session owner. Returns [`HookOutcome::Deferred`].
///
/// `work` must own its data (clone the document first); it reports through [`Ticket::progress`] and stops early when
/// [`Ticket::is_cancelled`] turns true. A panic inside it becomes an `internal` error. Other requests keep being served
/// meanwhile, so the document may change under the job: that is why it works on a copy.
///
/// # Errors
/// [`crate::ErrorKind::Internal`] when the thread cannot be started.
pub fn run_job<F>(ticket: &Ticket, name: &str, work: F) -> HookResult
where
    F: FnOnce(&Ticket) -> Result<Reply, ControlError> + Send + 'static,
{
    let ticket = ticket.clone();
    std::thread::Builder::new()
        .name(format!("anim-mcp-job-{name}"))
        .spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&ticket)))
                .unwrap_or_else(|_| Err(ControlError::new(crate::protocol::ErrorKind::Internal, "the background job panicked")));
            ticket.finish(outcome);
        })
        .map_err(|e| ControlError::new(crate::protocol::ErrorKind::Internal, format!("could not start a background job: {e}")))?;
    Ok(HookOutcome::Deferred)
}

/// Writes the first image of a reply (or any image) to `path` atomically.
///
/// # Errors
/// [`crate::ErrorKind::Failed`] when the file cannot be written.
pub fn save_image(path: &Path, image: &Image) -> Result<(), ControlError> {
    anim_session::craft_atomic::atomic_write(path, &image.data).map_err(|e| ControlError::failed(format!("could not write {}: {e}", path.display())))
}
