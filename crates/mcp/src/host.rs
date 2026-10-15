//! [`SessionHost`]: the owner of a [`Session`] that answers control [`Request`]s.
//!
//! The host is deliberately a plain value with `&mut self` methods. Whoever owns the session runs it: the headless
//! actor thread ([`crate::spawn_headless`]) or the application's UI thread, which drains a [`crate::ControlInbox`]
//! once per frame. All document state therefore lives on one thread and nothing is locked. Every request is journaled,
//! file parameters are confined, and a panic inside a command becomes an `internal` error instead of taking the
//! process down.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use anim_session::mcp::status as session_status;
use anim_session::{Action, CommandSpec, HistoryMode, Session, SizeHint, tool_name};
use serde_json::{Value, json};

use crate::call::{Incoming, Ticket};
use crate::confine::{Access, Confinement};
use crate::hooks::{AppHooks, ExportRequest, HookOutcome, RenderRequest, ScreenshotRequest, save_image};
use crate::journal::{Journal, Outcome};
use crate::props;
use crate::protocol::{BatchStep, CONTROL_VERSION, CommandInfo, ControlError, ErrorKind, Reply, Request};

/// Something that can answer requests. Implemented by [`SessionHost`]; applications with their own session wrapper can
/// implement it directly.
pub trait ControlHost {
    /// Handles one request: answer through the ticket now, or keep a clone and answer later.
    fn handle(&mut self, incoming: Incoming);

    /// Called regularly for deferred work.
    fn tick(&mut self) {}
}

/// Name and version the host reports in `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    /// Application name.
    pub name: String,
    /// Application version.
    pub version: String,
}

/// A [`Session`], the application's [`AppHooks`], path confinement and a journal, answering control requests.
pub struct SessionHost<D: Clone + SizeHint, H: AppHooks<D>> {
    session: Session<D>,
    hooks: H,
    confinement: Confinement,
    journal: Journal,
    info: HostInfo,
}

impl<D: Clone + SizeHint, H: AppHooks<D>> SessionHost<D, H> {
    /// A host over `session`. Files are fully confined (nothing readable or writable) until [`Self::with_confinement`].
    #[must_use]
    pub fn new(info: HostInfo, session: Session<D>, hooks: H) -> Self {
        Self { session, hooks, confinement: Confinement::default(), journal: Journal::default(), info }
    }

    /// Sets which folders may be read and written.
    #[must_use]
    pub fn with_confinement(mut self, confinement: Confinement) -> Self {
        self.confinement = confinement;
        self
    }

    /// Replaces the journal (to keep a JSON-lines file, say).
    #[must_use]
    pub fn with_journal(mut self, journal: Journal) -> Self {
        self.journal = journal;
        self
    }

    /// The session, for the application's own UI code.
    #[must_use]
    pub const fn session(&self) -> &Session<D> {
        &self.session
    }

    /// The session, mutably.
    pub const fn session_mut(&mut self) -> &mut Session<D> {
        &mut self.session
    }

    /// The hooks, mutably.
    pub const fn hooks_mut(&mut self) -> &mut H {
        &mut self.hooks
    }

    /// The journal.
    #[must_use]
    pub const fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Runs one request to an outcome. This is [`ControlHost::handle`] without the ticket bookkeeping; tests use it.
    ///
    /// # Errors
    /// The request's failure.
    pub fn dispatch(&mut self, request: Request, ticket: &Ticket) -> Result<HookOutcome, ControlError> {
        match request {
            Request::Status {} => Ok(ready(self.status())),
            Request::CommandList { filter, enabled_only, schemas } => Ok(ready(self.command_list(filter.as_deref(), enabled_only, schemas))),
            Request::CommandRun { id, params, merge_key } => self.command_run(&id, &params, merge_key.as_deref()),
            Request::Batch { steps, label } => self.batch(&steps, label.as_deref(), ticket),
            Request::QueryList {} => Ok(ready(json!({ "queries": self.hooks.queries() }))),
            Request::Query { name, params } => {
                self.hooks
                    .queries()
                    .iter()
                    .find(|q| q.name == name)
                    .ok_or_else(|| ControlError::new(ErrorKind::NotFound, format!("no query named {name:?}")))?;
                self.confinement.check_params(&params, &|key| self.hooks.path_access(&name, key))?;
                self.hooks.query(&self.session, &name, &params, ticket)
            }
            Request::Outline { depth } => self.hooks.outline(self.session.doc(), depth).map(ready),
            Request::Library {} => self.hooks.library(self.session.doc()).map(ready),
            Request::PropertyGet { path } => {
                props::parse_path(&path)?;
                Ok(ready(json!({ "path": path, "value": self.hooks.get_property(self.session.doc(), &path)? })))
            }
            Request::PropertySet { path, value } => self.property_set(&path, value),
            Request::Render { frame, scale, path, options } => {
                let scale = scale.unwrap_or(1.0);
                if !scale.is_finite() || scale <= 0.0 || scale > 16.0 {
                    return Err(ControlError::invalid("scale must be a number between 0 and 16"));
                }
                let save_to = path.map(|p| self.confinement.check(&p, Access::Write)).transpose()?;
                let request = RenderRequest { frame, scale, save_to: save_to.clone(), options };
                let outcome = self.hooks.render(self.session.doc(), &request, ticket)?;
                finish_image(outcome, save_to.as_deref())
            }
            Request::Screenshot { panel, path } => {
                let save_to = path.map(|p| self.confinement.check(&p, Access::Write)).transpose()?;
                let request = ScreenshotRequest { panel, save_to: save_to.clone() };
                let outcome = self.hooks.screenshot(&request, ticket)?;
                finish_image(outcome, save_to.as_deref())
            }
            Request::ExportFormats {} => Ok(ready(json!({ "formats": self.hooks.export_formats() }))),
            Request::Export { format, path, options } => {
                if !self.hooks.export_formats().iter().any(|f| f.id == format) {
                    let known: Vec<String> = self.hooks.export_formats().into_iter().map(|f| f.id).collect();
                    return Err(ControlError::new(ErrorKind::NotFound, format!("no export format {format:?}")).with_data(json!({ "formats": known })));
                }
                let path = self.confinement.check(&path, Access::Write)?;
                self.hooks.export(self.session.doc(), &ExportRequest { format, path, options }, ticket)
            }
            Request::Undo {} => self
                .session
                .undo()
                .map(|label| ready(self.history_state("undone", &label)))
                .ok_or_else(|| ControlError::new(ErrorKind::Disabled, "nothing to undo")),
            Request::Redo {} => self
                .session
                .redo()
                .map(|label| ready(self.history_state("redone", &label)))
                .ok_or_else(|| ControlError::new(ErrorKind::Disabled, "nothing to redo")),
            Request::Open { path } => {
                let real = self.confinement.check(&path, Access::Read)?;
                let doc = self.hooks.load(&real)?;
                self.session.replace_document(doc, Some(real.clone()));
                Ok(ready(json!({ "opened": real.display().to_string() })))
            }
            Request::New {} => {
                let doc = self.hooks.new_document()?;
                self.session.replace_document(doc, None);
                Ok(ready(json!({ "created": true })))
            }
            Request::Save { path } => self.save(path.as_deref()),
            Request::Journal { limit } => Ok(ready(json!({ "total": self.journal.total(), "entries": self.journal.tail(limit.unwrap_or(50).min(1000)) }))),
        }
    }

    fn status(&self) -> Value {
        let mut value = session_status(&self.session);
        if let Some(map) = value.as_object_mut() {
            map.insert("app".into(), self.hooks.status(&self.session));
            map.insert("name".into(), json!(self.info.name));
            map.insert("version".into(), json!(self.info.version));
            let (read, write) = self.confinement.roots();
            map.insert(
                "control".into(),
                json!({
                    "version": CONTROL_VERSION,
                    "journal_calls": self.journal.total(),
                    "confined": !self.confinement.is_unrestricted(),
                    "read_roots": read.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    "write_roots": write.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                }),
            );
        }
        value
    }

    fn history_state(&self, key: &str, label: &str) -> Value {
        let history = self.session.history();
        json!({ key: label, "can_undo": history.can_undo(), "can_redo": history.can_redo(), "dirty": self.session.is_dirty() })
    }

    fn spec(&self, id: &str) -> Result<CommandSpec<D>, ControlError> {
        let registry = self.session.registry();
        registry
            .get(id)
            .or_else(|| registry.get_by_tool_name(id))
            .filter(|spec| spec.mcp)
            .copied()
            .ok_or_else(|| ControlError::new(ErrorKind::NotFound, format!("unknown command {id:?}; list them with commands.list")))
    }

    fn command_info(&self, spec: &CommandSpec<D>, schema: bool) -> CommandInfo {
        CommandInfo {
            id: spec.id.to_owned(),
            tool: tool_name(spec.id),
            label: spec.label.to_owned(),
            description: spec.description.to_owned(),
            menu: spec.menu.iter().map(|s| (*s).to_owned()).collect(),
            shortcut: self.session.registry().shortcut_of(spec.id).map(|s| ToString::to_string(&s)),
            enabled: self.session.is_enabled(spec.id),
            read_only: spec.history == HistoryMode::Transient && spec.action == Action::Run,
            schema: schema.then(|| spec.schema_value()),
        }
    }

    fn command_list(&self, filter: Option<&str>, enabled_only: bool, schemas: bool) -> Value {
        let needle = filter.map(str::to_lowercase);
        let commands: Vec<CommandInfo> = self
            .session
            .registry()
            .iter()
            .filter(|spec| spec.mcp)
            .filter(|spec| needle.as_ref().is_none_or(|n| spec.id.to_lowercase().contains(n) || spec.label.to_lowercase().contains(n)))
            .map(|spec| self.command_info(spec, schemas))
            .filter(|info| !enabled_only || info.enabled)
            .collect();
        json!({ "commands": commands })
    }

    fn check_command_paths(&self, command: &str, params: &Value) -> Result<(), ControlError> {
        if !self.confinement.is_unrestricted() && self.hooks.refused_while_confined(command) {
            return Err(ControlError::new(ErrorKind::Denied, format!("{command} is refused while file access is confined")));
        }
        self.confinement.check_params(params, &|key| self.hooks.path_access(command, key))
    }

    fn command_run(&mut self, id: &str, params: &Value, merge_key: Option<&str>) -> Result<HookOutcome, ControlError> {
        let spec = self.spec(id)?;
        if !params.is_null() && !params.is_object() {
            return Err(ControlError::invalid("params must be an object"));
        }
        self.check_command_paths(spec.id, params)?;
        let value = self.session.execute_merged(spec.id, params, merge_key)?;
        Ok(ready(json!({ "command": spec.id, "result": value, "dirty": self.session.is_dirty(), "undo_steps": self.session.history().undo_len() })))
    }

    fn batch(&mut self, steps: &[BatchStep], label: Option<&str>, ticket: &Ticket) -> Result<HookOutcome, ControlError> {
        if steps.is_empty() {
            return Err(ControlError::invalid("a batch needs at least one step"));
        }
        let mut scratch = self.session.snapshot();
        let mut results = Vec::with_capacity(steps.len());
        let mut changes = false;
        let total = f32_from_len(steps.len());
        for (index, step) in steps.iter().enumerate() {
            ticket.check_cancelled()?;
            ticket.progress(f32_from_len(index) / total, step.id.clone());
            let at = |e: ControlError| {
                let data = json!({ "step": index, "command": step.id, "applied": false });
                ControlError { data: Some(data), message: format!("step {index} ({}): {}", step.id, e.message), ..e }
            };
            let spec = self.spec(&step.id).map_err(at)?;
            if spec.action != Action::Run {
                return Err(at(ControlError::invalid("undo and redo cannot run inside a batch")));
            }
            if !(spec.enabled)(&scratch) {
                return Err(at(ControlError::new(ErrorKind::Disabled, format!("command {:?} is not enabled right now", spec.id))));
            }
            if !step.params.is_null() && !step.params.is_object() {
                return Err(at(ControlError::invalid("params must be an object")));
            }
            self.check_command_paths(spec.id, &step.params).map_err(at)?;
            let result = (spec.run)(&mut scratch, &step.params).map_err(|e| at(e.into()))?;
            changes |= spec.history == HistoryMode::Undoable;
            results.push(json!({ "command": spec.id, "result": result }));
        }
        let default_label;
        let label = if let Some(label) = label {
            label
        } else {
            default_label = format!("Batch of {} commands", steps.len());
            &default_label
        };
        if changes {
            self.session.edit(label, None, |doc| *doc = scratch);
        }
        Ok(ready(json!({ "steps": results, "undo_steps": self.session.history().undo_len(), "dirty": self.session.is_dirty() })))
    }

    fn property_set(&mut self, path: &str, value: Value) -> Result<HookOutcome, ControlError> {
        props::parse_path(path)?;
        let mut scratch = self.session.snapshot();
        let result = self.hooks.set_property(&mut scratch, path, value)?;
        self.session.edit(&format!("Set {path}"), None, |doc| *doc = scratch);
        Ok(ready(json!({ "path": path, "result": result, "dirty": self.session.is_dirty() })))
    }

    fn save(&mut self, path: Option<&str>) -> Result<HookOutcome, ControlError> {
        let hooks = &mut self.hooks;
        let write = |doc: &D, file: &mut std::fs::File| hooks.save(doc, file);
        let saved = if let Some(path) = path {
            let real = self.confinement.check(path, Access::Write)?;
            self.session.save_as(real, write)
        } else {
            let current = self.session.path().ok_or_else(|| ControlError::invalid("the document has no file path yet; pass `path`"))?;
            self.confinement.check(&current.display().to_string(), Access::Write)?;
            self.session.save(write)
        };
        saved.map_err(|e| ControlError::failed(e.to_string()))?;
        let saved_to = self.session.path().map(|p| p.display().to_string());
        Ok(ready(json!({ "saved": saved_to, "dirty": self.session.is_dirty() })))
    }
}

/// `usize` to `f32` for progress fractions (precision loss is irrelevant there).
fn f32_from_len(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

const fn ready(value: Value) -> HookOutcome {
    HookOutcome::Ready(Reply::value(value))
}

/// Writes the first image of a ready reply to `save_to` and records the path in the reply.
fn finish_image(outcome: HookOutcome, save_to: Option<&std::path::Path>) -> Result<HookOutcome, ControlError> {
    match (outcome, save_to) {
        (HookOutcome::Ready(mut reply), Some(path)) => {
            let image = reply.images.first().ok_or_else(|| ControlError::new(ErrorKind::Internal, "the renderer returned no image"))?;
            save_image(path, image)?;
            if let Some(map) = reply.value.as_object_mut() {
                map.insert("saved_to".into(), json!(path.display().to_string()));
            } else if reply.value.is_null() {
                reply.value = json!({ "saved_to": path.display().to_string() });
            }
            Ok(HookOutcome::Ready(reply))
        }
        (other, _) => Ok(other),
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".to_owned())
}

impl<D: Clone + SizeHint, H: AppHooks<D>> ControlHost for SessionHost<D, H> {
    fn handle(&mut self, incoming: Incoming) {
        let Incoming { request, ticket, origin } = incoming;
        let summary = request.summary();
        let started = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(|| self.dispatch(request, &ticket))).unwrap_or_else(|payload| {
            Err(ControlError::new(
                ErrorKind::Internal,
                format!("internal error: {} (the document may be inconsistent; undo restores the last step)", panic_text(payload.as_ref())),
            ))
        });
        let took_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match outcome {
            Ok(HookOutcome::Ready(reply)) => {
                self.journal.record(&origin, summary, Outcome::Ok, None, took_ms);
                ticket.finish(Ok(reply));
            }
            Ok(HookOutcome::Deferred) => self.journal.record(&origin, summary, Outcome::Deferred, None, took_ms),
            Err(error) => {
                self.journal.record(&origin, summary, Outcome::Error, Some(error.to_string()), took_ms);
                ticket.finish(Err(error));
            }
        }
    }

    fn tick(&mut self) {
        self.hooks.tick(&mut self.session);
    }
}
