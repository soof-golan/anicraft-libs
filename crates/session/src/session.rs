//! [`Session`]: the open document, its history, its file path and the command registry, wired together.

use std::fs::File;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::command::{Action, CommandError, CommandRegistry, HistoryMode};
use crate::history::{Budget, History, SizeHint};

/// Why saving failed.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// The session has no file path yet; use a save-as call.
    #[error("the document has no file path")]
    NoPath,
    /// Writing failed; the previous file (if any) is intact.
    #[error("could not write {path}: {source}")]
    Io {
        /// Destination.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
}

/// A document under edit.
///
/// All changes go through [`Session::execute`] (commands), [`Session::edit`] (code) or the gesture calls; each
/// records exactly one undo step. [`Session::doc_mut`] bypasses history for state that must not be undoable.
pub struct Session<D> {
    doc: D,
    history: History<D>,
    registry: CommandRegistry<D>,
    path: Option<PathBuf>,
}

impl<D: Clone + SizeHint> Session<D> {
    /// A session over `doc`, clean, with the default history budget.
    #[must_use]
    pub fn new(doc: D, registry: CommandRegistry<D>) -> Self {
        Self::with_budget(doc, registry, Budget::default())
    }

    /// A session with an explicit history budget.
    #[must_use]
    pub const fn with_budget(doc: D, registry: CommandRegistry<D>, budget: Budget) -> Self {
        Self { doc, history: History::new(budget), registry, path: None }
    }

    /// The document.
    #[must_use]
    pub const fn doc(&self) -> &D {
        &self.doc
    }

    /// The document, mutably, **without** recording history. For previews and non-undoable state.
    pub const fn doc_mut(&mut self) -> &mut D {
        &mut self.doc
    }

    /// The command registry.
    #[must_use]
    pub const fn registry(&self) -> &CommandRegistry<D> {
        &self.registry
    }

    /// The history, for menus ("Undo Move Keyframe") and budgets.
    #[must_use]
    pub const fn history(&self) -> &History<D> {
        &self.history
    }

    /// Mutable history access (budget changes).
    pub const fn history_mut(&mut self) -> &mut History<D> {
        &mut self.history
    }

    /// A copy of the document for background work (autosave, export).
    #[must_use]
    pub fn snapshot(&self) -> D {
        self.doc.clone()
    }

    /// Replaces the document (after opening a file): history cleared, state clean.
    pub fn replace_document(&mut self, doc: D, path: Option<PathBuf>) {
        self.doc = doc;
        self.path = path;
        self.history.clear();
    }

    /// The file the document was opened from or last saved to.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Sets the path without saving (for save-as flows that write elsewhere).
    pub fn set_path(&mut self, path: Option<PathBuf>) {
        self.path = path;
    }

    /// Whether there are unsaved changes. Follows undo and redo.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.history.is_clean()
    }

    /// Declares the current state saved (after an export that counts as a save).
    pub fn mark_saved(&mut self) {
        self.history.mark_clean();
    }

    /// Window title: `name* - app`, with `Untitled` for new documents.
    #[must_use]
    pub fn title(&self, app: &str) -> String {
        let name = self.path.as_deref().and_then(Path::file_name).and_then(|n| n.to_str()).unwrap_or("Untitled");
        let star = if self.is_dirty() { "*" } else { "" };
        format!("{name}{star} - {app}")
    }

    /// Writes the document atomically to `path` through `write`, then records the path and marks the state saved.
    /// A failure leaves any existing file untouched and the session dirty.
    ///
    /// # Errors
    /// [`SaveError::Io`] when staging or replacing the file fails.
    pub fn save_as(&mut self, path: impl Into<PathBuf>, write: impl FnOnce(&D, &mut File) -> std::io::Result<()>) -> Result<(), SaveError> {
        let path = path.into();
        let doc = &self.doc;
        craft_atomic::atomic_write_with(&path, |file| write(doc, file)).map_err(|source| SaveError::Io { path: path.clone(), source })?;
        self.path = Some(path);
        self.history.mark_clean();
        Ok(())
    }

    /// Saves to the current path.
    ///
    /// # Errors
    /// [`SaveError::NoPath`] for a new document, or [`SaveError::Io`].
    pub fn save(&mut self, write: impl FnOnce(&D, &mut File) -> std::io::Result<()>) -> Result<(), SaveError> {
        let path = self.path.clone().ok_or(SaveError::NoPath)?;
        self.save_as(path, write)
    }

    /// Whether `id` could run right now.
    #[must_use]
    pub fn is_enabled(&self, id: &str) -> bool {
        self.registry.get(id).is_some_and(|spec| match spec.action {
            Action::Undo => self.history.can_undo() || self.history.is_open(),
            Action::Redo => self.history.can_redo(),
            Action::Run => (spec.enabled)(&self.doc),
        })
    }

    /// Runs the command `id` with `params`.
    ///
    /// Undoable commands are atomic: on `Err` the document is restored to its state before the call and no step is
    /// recorded; on `Ok` exactly one step labelled with the command's label is recorded.
    ///
    /// # Errors
    /// [`CommandError::Unknown`], [`CommandError::Disabled`], or whatever the command returns.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        self.execute_merged(id, params, None)
    }

    /// Like [`Self::execute`], folding consecutive calls with the same `merge_key` into one undo step
    /// (arrow-key nudges, typing).
    ///
    /// # Errors
    /// As [`Self::execute`].
    pub fn execute_merged(&mut self, id: &str, params: &Value, merge_key: Option<&str>) -> Result<Value, CommandError> {
        let spec = *self.registry.get(id).ok_or_else(|| CommandError::Unknown(id.to_owned()))?;
        if !self.is_enabled(id) {
            return Err(CommandError::Disabled(id.to_owned()));
        }
        match spec.action {
            Action::Undo => Ok(json!({ "undone": self.undo() })),
            Action::Redo => Ok(json!({ "redone": self.redo() })),
            Action::Run => match spec.history {
                HistoryMode::Transient => (spec.run)(&mut self.doc, params),
                HistoryMode::Undoable => {
                    let before = self.doc.clone();
                    match (spec.run)(&mut self.doc, params) {
                        Ok(value) => {
                            self.history.record(before, spec.label, merge_key);
                            Ok(value)
                        }
                        Err(e) => {
                            self.doc = before;
                            Err(e)
                        }
                    }
                }
            },
        }
    }

    /// Runs a closure that edits the document and records one undo step called `label`.
    pub fn edit<R>(&mut self, label: &str, merge_key: Option<&str>, f: impl FnOnce(&mut D) -> R) -> R {
        let before = self.doc.clone();
        let out = f(&mut self.doc);
        self.history.record(before, label, merge_key);
        out
    }

    /// Undoes one step. An open gesture is cancelled first (that counts as the step). Returns the step's label.
    pub fn undo(&mut self) -> Option<String> {
        if let Some(base) = self.history.cancel() {
            self.doc = base;
            return Some("Cancelled gesture".to_owned());
        }
        self.history.undo(&mut self.doc)
    }

    /// Redoes one step. Returns the step's label.
    pub fn redo(&mut self) -> Option<String> {
        self.history.redo(&mut self.doc)
    }

    /// Starts a continuous gesture (drag, scrub). Mutate through [`Self::doc_mut`] while it runs.
    pub fn begin_gesture(&mut self, label: &str) {
        self.history.begin(self.doc.clone(), label);
    }

    /// Ends the gesture as one undo step. Returns false when none was open.
    pub fn commit_gesture(&mut self) -> bool {
        self.history.commit()
    }

    /// Abandons the gesture and restores the document to its state at [`Self::begin_gesture`].
    pub fn cancel_gesture(&mut self) -> bool {
        match self.history.cancel() {
            Some(base) => {
                self.doc = base;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandSpec, param};

    #[derive(Debug, Clone, PartialEq, Default)]
    struct Doc {
        n: i64,
        log: Vec<String>,
    }
    impl SizeHint for Doc {
        fn size_hint(&self) -> usize {
            16 + self.log.iter().map(String::len).sum::<usize>()
        }
    }

    fn add(doc: &mut Doc, p: &Value) -> Result<Value, CommandError> {
        let by: i64 = param(p, "by")?;
        doc.n += by;
        doc.log.push(format!("add {by}"));
        Ok(json!(doc.n))
    }
    fn fail_after_change(doc: &mut Doc, _: &Value) -> Result<Value, CommandError> {
        doc.n = 999;
        Err(CommandError::failed("nope"))
    }
    fn read_n(doc: &mut Doc, _: &Value) -> Result<Value, CommandError> {
        Ok(json!(doc.n))
    }
    fn positive(doc: &Doc) -> bool {
        doc.n > 0
    }

    fn session() -> Session<Doc> {
        let mut r = CommandRegistry::with_history_commands();
        r.register_all([
            CommandSpec::new("math.add", "Add", add),
            CommandSpec::new("math.fail", "Fail", fail_after_change),
            CommandSpec::new("math.read", "Read", read_n).transient(),
            CommandSpec::new("math.reset_if_positive", "Reset", |d: &mut Doc, _: &Value| {
                d.n = 0;
                Ok(Value::Null)
            })
            .enabled_when(positive),
        ])
        .unwrap();
        Session::new(Doc::default(), r)
    }

    #[test]
    fn execute_records_undo_and_dirty() {
        let mut s = session();
        assert!(!s.is_dirty());
        assert_eq!(s.execute("math.add", &json!({ "by": 5 })).unwrap(), json!(5));
        assert!(s.is_dirty());
        assert_eq!(s.history().undo_label(), Some("Add"));
        assert_eq!(s.execute("edit.undo", &Value::Null).unwrap(), json!({ "undone": "Add" }));
        assert_eq!(s.doc().n, 0);
        assert!(!s.is_dirty());
        s.execute("edit.redo", &Value::Null).unwrap();
        assert_eq!(s.doc().n, 5);
        assert!(s.is_dirty());
    }

    #[test]
    fn failing_command_rolls_back() {
        let mut s = session();
        s.execute("math.add", &json!({ "by": 1 })).unwrap();
        let err = s.execute("math.fail", &Value::Null).unwrap_err();
        assert_eq!(err, CommandError::failed("nope"));
        assert_eq!(s.doc().n, 1);
        assert_eq!(s.history().undo_len(), 1);
        let err = s.execute("math.add", &json!({})).unwrap_err();
        assert!(matches!(err, CommandError::InvalidParams(_)));
        assert_eq!(s.history().undo_len(), 1);
    }

    #[test]
    fn unknown_and_disabled() {
        let mut s = session();
        assert_eq!(s.execute("nope", &Value::Null), Err(CommandError::Unknown("nope".into())));
        assert_eq!(s.execute("math.reset_if_positive", &Value::Null), Err(CommandError::Disabled("math.reset_if_positive".into())));
        assert!(!s.is_enabled("edit.undo") && !s.is_enabled("edit.redo") && !s.is_enabled("nope"));
        s.execute("math.add", &json!({ "by": 2 })).unwrap();
        assert!(s.is_enabled("math.reset_if_positive") && s.is_enabled("edit.undo"));
        s.execute("math.reset_if_positive", &Value::Null).unwrap();
        assert_eq!(s.doc().n, 0);
    }

    #[test]
    fn transient_commands_make_no_steps() {
        let mut s = session();
        assert_eq!(s.execute("math.read", &Value::Null).unwrap(), json!(0));
        assert_eq!(s.history().undo_len(), 0);
        assert!(!s.is_dirty());
    }

    #[test]
    fn merged_execution_is_one_step() {
        let mut s = session();
        for _ in 0..4 {
            s.execute_merged("math.add", &json!({ "by": 1 }), Some("nudge")).unwrap();
        }
        assert_eq!(s.doc().n, 4);
        assert_eq!(s.history().undo_len(), 1);
        s.undo();
        assert_eq!(s.doc().n, 0);
    }

    #[test]
    fn gestures() {
        let mut s = session();
        s.begin_gesture("Drag");
        for v in 1..=30 {
            s.doc_mut().n = v;
        }
        assert!(s.commit_gesture());
        assert_eq!(s.history().undo_len(), 1);
        assert_eq!(s.history().undo_label(), Some("Drag"));
        s.undo();
        assert_eq!(s.doc().n, 0);

        s.begin_gesture("Drag 2");
        s.doc_mut().n = 77;
        assert!(s.cancel_gesture());
        assert_eq!(s.doc().n, 0);
        assert!(!s.cancel_gesture());

        s.begin_gesture("Drag 3");
        s.doc_mut().n = 5;
        assert!(s.is_enabled("edit.undo"));
        s.undo();
        assert_eq!(s.doc().n, 0);
        assert!(!s.history().is_open());
    }

    #[test]
    fn edit_closure_records() {
        let mut s = session();
        let got = s.edit("Poke", None, |d| {
            d.n = 3;
            d.n * 2
        });
        assert_eq!(got, 6);
        assert_eq!(s.history().undo_label(), Some("Poke"));
    }

    #[test]
    fn save_marks_clean_and_follows_undo() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.json");
        let mut s = session();
        assert!(matches!(s.save(|_, _| Ok(())), Err(SaveError::NoPath)));
        s.execute("math.add", &json!({ "by": 3 })).unwrap();
        assert_eq!(s.title("AniCraft"), "Untitled* - AniCraft");
        let write = |d: &Doc, f: &mut File| {
            use std::io::Write;
            f.write_all(d.n.to_string().as_bytes())
        };
        s.save_as(&file, write).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "3");
        assert!(!s.is_dirty());
        assert_eq!(s.title("AniCraft"), "a.json - AniCraft");
        s.execute("math.add", &json!({ "by": 1 })).unwrap();
        assert!(s.is_dirty());
        s.undo();
        assert!(!s.is_dirty());
        s.save(write).unwrap();
        assert_eq!(s.path(), Some(file.as_path()));
    }

    #[test]
    fn failed_save_keeps_old_file_and_dirty_state() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("keep.txt");
        std::fs::write(&file, "old").unwrap();
        let mut s = session();
        s.execute("math.add", &json!({ "by": 3 })).unwrap();
        let result = s.save_as(&file, |_, f| {
            use std::io::Write;
            f.write_all(b"partial")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(matches!(result, Err(SaveError::Io { .. })));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "old");
        assert!(s.is_dirty());
        assert!(s.path().is_none());
    }

    #[test]
    fn replace_document_resets() {
        let mut s = session();
        s.execute("math.add", &json!({ "by": 3 })).unwrap();
        s.replace_document(Doc { n: 50, log: vec![] }, Some(PathBuf::from("x.fla")));
        assert!(!s.is_dirty() && !s.history().can_undo());
        assert_eq!(s.doc().n, 50);
        assert_eq!(s.snapshot().n, 50);
    }
}
