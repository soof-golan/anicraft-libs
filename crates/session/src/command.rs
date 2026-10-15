//! The command registry: everything the UI (and the MCP interface) can do is a named command.
//!
//! A [`CommandSpec`] is plain data plus two function pointers. The registry indexes specs by id, builds the
//! menu tree and maps shortcuts. The UI is a pure function of state that emits command ids and parameters;
//! [`Session::execute`](crate::Session::execute) runs them against the document and records undo.

use std::collections::HashMap;
use std::fmt;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::shortcut::{Shortcut, ShortcutError};

/// A command failed or could not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// No command has this id.
    #[error("unknown command {0:?}")]
    Unknown(String),
    /// The command exists but its `enabled` predicate is false for the current document.
    #[error("command {0:?} is not enabled right now")]
    Disabled(String),
    /// The parameters did not have the shape the command wants.
    #[error("invalid parameters: {0}")]
    InvalidParams(String),
    /// The command ran and failed; the document is left as it was.
    #[error("{0}")]
    Failed(String),
}

impl CommandError {
    /// Shorthand for [`CommandError::InvalidParams`].
    #[must_use]
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::InvalidParams(message.into())
    }

    /// Shorthand for [`CommandError::Failed`].
    #[must_use]
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}

/// Deserialises a command's whole parameter object. `null` counts as an empty object.
///
/// # Errors
/// [`CommandError::InvalidParams`] when the value does not fit `T`.
pub fn parse_params<T: DeserializeOwned>(params: &Value) -> Result<T, CommandError> {
    let value = if params.is_null() { Value::Object(serde_json::Map::new()) } else { params.clone() };
    serde_json::from_value(value).map_err(|e| CommandError::InvalidParams(e.to_string()))
}

/// Reads one named parameter.
///
/// # Errors
/// [`CommandError::InvalidParams`] when the key is missing or has the wrong type.
pub fn param<T: DeserializeOwned>(params: &Value, name: &str) -> Result<T, CommandError> {
    let value = params.get(name).ok_or_else(|| CommandError::InvalidParams(format!("missing parameter {name:?}")))?;
    serde_json::from_value(value.clone()).map_err(|e| CommandError::InvalidParams(format!("parameter {name:?}: {e}")))
}

/// Reads one optional named parameter (`None` when absent or null).
///
/// # Errors
/// [`CommandError::InvalidParams`] when the key is present with the wrong type.
pub fn param_opt<T: DeserializeOwned>(params: &Value, name: &str) -> Result<Option<T>, CommandError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|e| CommandError::InvalidParams(format!("parameter {name:?}: {e}"))),
    }
}

/// Whether running a command creates an undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryMode {
    /// The document is snapshotted first; a successful run becomes one undo step labelled with the command.
    #[default]
    Undoable,
    /// The command changes no document content (selection, zoom, queries): no undo step.
    Transient,
}

/// What a command does when executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Action {
    /// Call the spec's `run` function on the document.
    #[default]
    Run,
    /// Undo one step (handled by the session; `run` is not called).
    Undo,
    /// Redo one step (handled by the session; `run` is not called).
    Redo,
}

/// Enabled predicate.
pub type EnabledFn<D> = fn(&D) -> bool;
/// Command body: mutate the document and return a JSON result.
pub type RunFn<D> = fn(&mut D, &Value) -> Result<Value, CommandError>;

const fn always<D>(_: &D) -> bool {
    true
}

#[allow(clippy::unnecessary_wraps, reason = "must match the RunFn signature")]
const fn no_op<D>(_: &mut D, _: &Value) -> Result<Value, CommandError> {
    Ok(Value::Null)
}

/// One command: identity, presentation, and behaviour.
pub struct CommandSpec<D> {
    /// Stable machine id, for example `"edit.undo"` or `"timeline.insert_keyframe"`. Also the MCP tool name.
    pub id: &'static str,
    /// Menu label and MCP title.
    pub label: &'static str,
    /// Menu path of parent menus, for example `&["Edit"]` or `&["File", "Export"]`. Empty: not in a menu.
    pub menu: &'static [&'static str],
    /// Shortcut text such as `"Cmd+Shift+Z"`; parsed when the spec is registered.
    pub shortcut: Option<&'static str>,
    /// Draw a separator above this item in its menu.
    pub separator_before: bool,
    /// Whether the command can run on this document right now.
    pub enabled: EnabledFn<D>,
    /// The command body.
    pub run: RunFn<D>,
    /// Longer description, shown to MCP clients.
    pub description: &'static str,
    /// JSON Schema (as JSON text) describing the parameter object; `None` means any object.
    pub input_schema: Option<&'static str>,
    /// Whether a run creates an undo step.
    pub history: HistoryMode,
    /// What executing does; only the session's built-in undo/redo use anything but [`Action::Run`].
    pub action: Action,
    /// Offer the command to MCP clients.
    pub mcp: bool,
}

impl<D> Clone for CommandSpec<D> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<D> Copy for CommandSpec<D> {}

impl<D> fmt::Debug for CommandSpec<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandSpec").field("id", &self.id).field("label", &self.label).finish_non_exhaustive()
    }
}

impl<D> CommandSpec<D> {
    /// A command that is always enabled, undoable, exposed over MCP and not in any menu.
    #[must_use]
    pub const fn new(id: &'static str, label: &'static str, run: RunFn<D>) -> Self {
        Self {
            id,
            label,
            menu: &[],
            shortcut: None,
            separator_before: false,
            enabled: always::<D>,
            run,
            description: "",
            input_schema: None,
            history: HistoryMode::Undoable,
            action: Action::Run,
            mcp: true,
        }
    }

    /// The session's undo command (`edit.undo`, Edit menu, `Cmd+Z`).
    #[must_use]
    pub const fn undo() -> Self {
        let mut spec = Self::new("edit.undo", "Undo", no_op::<D>);
        spec.menu = &["Edit"];
        spec.shortcut = Some("Cmd+Z");
        spec.history = HistoryMode::Transient;
        spec.action = Action::Undo;
        spec.description = "Undo the most recent edit.";
        spec
    }

    /// The session's redo command (`edit.redo`, Edit menu, `Cmd+Shift+Z`).
    #[must_use]
    pub const fn redo() -> Self {
        let mut spec = Self::new("edit.redo", "Redo", no_op::<D>);
        spec.menu = &["Edit"];
        spec.shortcut = Some("Cmd+Shift+Z");
        spec.history = HistoryMode::Transient;
        spec.action = Action::Redo;
        spec.description = "Redo the most recently undone edit.";
        spec
    }

    /// Places the command in a menu.
    #[must_use]
    pub const fn in_menu(mut self, menu: &'static [&'static str]) -> Self {
        self.menu = menu;
        self
    }

    /// Sets the shortcut text.
    #[must_use]
    pub const fn with_shortcut(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Sets the enabled predicate.
    #[must_use]
    pub const fn enabled_when(mut self, enabled: EnabledFn<D>) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the description.
    #[must_use]
    pub const fn describe(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }

    /// Sets the JSON Schema of the parameters.
    #[must_use]
    pub const fn with_schema(mut self, schema: &'static str) -> Self {
        self.input_schema = Some(schema);
        self
    }

    /// Marks the command as not creating undo steps.
    #[must_use]
    pub const fn transient(mut self) -> Self {
        self.history = HistoryMode::Transient;
        self
    }

    /// Draws a separator above the item.
    #[must_use]
    pub const fn separated(mut self) -> Self {
        self.separator_before = true;
        self
    }

    /// Hides the command from MCP clients.
    #[must_use]
    pub const fn hidden_from_mcp(mut self) -> Self {
        self.mcp = false;
        self
    }

    /// The parameter schema as a JSON value (`{"type":"object"}` when none or unparsable).
    #[must_use]
    pub fn schema_value(&self) -> Value {
        self.input_schema.and_then(|text| serde_json::from_str::<Value>(text).ok()).unwrap_or_else(|| serde_json::json!({ "type": "object" }))
    }
}

/// The MCP tool name of a command id: dots become underscores, because many clients only accept
/// `[A-Za-z0-9_-]` in tool names (`timeline.insert_keyframe` becomes `timeline_insert_keyframe`).
#[must_use]
pub fn tool_name(id: &str) -> String {
    id.replace('.', "_")
}

/// Why a spec could not be registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// The id is empty or has characters other than letters, digits, `.`, `_` and `-`.
    #[error("invalid command id {0:?}")]
    InvalidId(&'static str),
    /// Another command already has this id.
    #[error("command id {0:?} is already registered")]
    DuplicateId(&'static str),
    /// The shortcut text did not parse.
    #[error("command {id:?}: {source}")]
    BadShortcut {
        /// The command.
        id: &'static str,
        /// The parse error.
        source: ShortcutError,
    },
    /// Another command already uses this shortcut.
    #[error("shortcut {shortcut} of {id:?} is already used by {existing:?}")]
    ShortcutConflict {
        /// The command being registered.
        id: &'static str,
        /// The command that owns the shortcut.
        existing: &'static str,
        /// The shortcut.
        shortcut: Shortcut,
    },
}

/// One node of the menu bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuNode {
    /// A submenu.
    Menu {
        /// Menu title.
        name: &'static str,
        /// Entries in registration order.
        children: Vec<Self>,
    },
    /// A command entry.
    Item {
        /// Command id to execute.
        id: &'static str,
        /// Label.
        label: &'static str,
        /// Shortcut to display, if any.
        shortcut: Option<Shortcut>,
        /// Separator above.
        separator_before: bool,
    },
}

/// Commands by id, with the menu tree and the shortcut map.
pub struct CommandRegistry<D> {
    specs: Vec<CommandSpec<D>>,
    shortcuts_of: Vec<Option<Shortcut>>,
    by_id: HashMap<&'static str, usize>,
    by_shortcut: HashMap<Shortcut, usize>,
}

impl<D> Default for CommandRegistry<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D> fmt::Debug for CommandRegistry<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.specs.iter().map(|s| s.id)).finish()
    }
}

impl<D> CommandRegistry<D> {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self { specs: Vec::new(), shortcuts_of: Vec::new(), by_id: HashMap::new(), by_shortcut: HashMap::new() }
    }

    /// A registry holding the session's built-in `edit.undo` and `edit.redo`.
    #[must_use]
    pub fn with_history_commands() -> Self {
        let mut registry = Self::new();
        // The ids and shortcuts are distinct constants, so these cannot fail on an empty registry.
        let _ = registry.register(CommandSpec::undo());
        let _ = registry.register(CommandSpec::redo());
        registry
    }

    /// Adds a command.
    ///
    /// # Errors
    /// [`RegistryError`] for a malformed or duplicate id, or an unparsable or already used shortcut.
    pub fn register(&mut self, spec: CommandSpec<D>) -> Result<(), RegistryError> {
        let valid_id = !spec.id.is_empty() && spec.id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !valid_id {
            return Err(RegistryError::InvalidId(spec.id));
        }
        if self.by_id.contains_key(spec.id) || self.specs.iter().any(|s| tool_name(s.id) == tool_name(spec.id)) {
            return Err(RegistryError::DuplicateId(spec.id));
        }
        if spec.id.len() > 64 {
            return Err(RegistryError::InvalidId(spec.id));
        }
        let shortcut = match spec.shortcut {
            Some(text) => Some(text.parse::<Shortcut>().map_err(|source| RegistryError::BadShortcut { id: spec.id, source })?),
            None => None,
        };
        let index = self.specs.len();
        if let Some(shortcut) = &shortcut {
            if let Some(existing) = self.by_shortcut.get(shortcut).and_then(|i| self.specs.get(*i)) {
                return Err(RegistryError::ShortcutConflict { id: spec.id, existing: existing.id, shortcut: shortcut.clone() });
            }
            self.by_shortcut.insert(shortcut.clone(), index);
        }
        self.by_id.insert(spec.id, index);
        self.specs.push(spec);
        self.shortcuts_of.push(shortcut);
        Ok(())
    }

    /// Adds several commands, stopping at the first error.
    ///
    /// # Errors
    /// The first [`RegistryError`] from [`Self::register`].
    pub fn register_all(&mut self, specs: impl IntoIterator<Item = CommandSpec<D>>) -> Result<(), RegistryError> {
        specs.into_iter().try_for_each(|spec| self.register(spec))
    }

    /// Looks a command up by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&CommandSpec<D>> {
        self.by_id.get(id).and_then(|i| self.specs.get(*i))
    }

    /// Looks a command up by its MCP tool name (see [`tool_name`]).
    #[must_use]
    pub fn get_by_tool_name(&self, name: &str) -> Option<&CommandSpec<D>> {
        self.specs.iter().find(|s| tool_name(s.id) == name)
    }

    /// All commands in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec<D>> {
        self.specs.iter()
    }

    /// Number of commands.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether no commands are registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// The command bound to a key chord.
    #[must_use]
    pub fn lookup_shortcut(&self, shortcut: &Shortcut) -> Option<&CommandSpec<D>> {
        self.by_shortcut.get(shortcut).and_then(|i| self.specs.get(*i))
    }

    /// The parsed shortcut of a command.
    #[must_use]
    pub fn shortcut_of(&self, id: &str) -> Option<&Shortcut> {
        self.by_id.get(id).and_then(|i| self.shortcuts_of.get(*i)).and_then(Option::as_ref)
    }

    /// The menu bar: top-level menus in order of first appearance, entries in registration order.
    #[must_use]
    pub fn menu_tree(&self) -> Vec<MenuNode> {
        let mut roots: Vec<MenuNode> = Vec::new();
        for (spec, shortcut) in self.specs.iter().zip(&self.shortcuts_of) {
            if spec.menu.is_empty() {
                continue;
            }
            let item = MenuNode::Item { id: spec.id, label: spec.label, shortcut: shortcut.clone(), separator_before: spec.separator_before };
            insert_at(&mut roots, spec.menu, item);
        }
        roots
    }
}

fn insert_at(level: &mut Vec<MenuNode>, path: &[&'static str], item: MenuNode) {
    let Some((first, rest)) = path.split_first() else {
        level.push(item);
        return;
    };
    let position = level.iter().position(|n| matches!(n, MenuNode::Menu { name, .. } if name == first));
    let index = position.unwrap_or_else(|| {
        level.push(MenuNode::Menu { name: first, children: Vec::new() });
        level.len().saturating_sub(1)
    });
    if let Some(MenuNode::Menu { children, .. }) = level.get_mut(index) {
        insert_at(children, rest, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct P {
        n: i32,
    }

    fn ok(_: &mut i32, _: &Value) -> Result<Value, CommandError> {
        Ok(Value::Null)
    }

    #[test]
    fn lookup_and_duplicates() {
        let mut r = CommandRegistry::<i32>::new();
        r.register(CommandSpec::new("a.b", "A", ok)).unwrap();
        assert!(r.get("a.b").is_some());
        assert!(r.get("nope").is_none());
        assert_eq!(r.register(CommandSpec::new("a.b", "A2", ok)), Err(RegistryError::DuplicateId("a.b")));
        assert_eq!(r.register(CommandSpec::new("bad id", "x", ok)), Err(RegistryError::InvalidId("bad id")));
        assert_eq!(r.register(CommandSpec::new("", "x", ok)), Err(RegistryError::InvalidId("")));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn shortcut_map_and_conflicts() {
        let mut r = CommandRegistry::<i32>::with_history_commands();
        let undo = Shortcut::key("z").with_command();
        assert_eq!(r.lookup_shortcut(&undo).map(|s| s.id), Some("edit.undo"));
        let clash = CommandSpec::new("x.clash", "Clash", ok).with_shortcut("ctrl+z");
        assert!(matches!(r.register(clash), Err(RegistryError::ShortcutConflict { existing: "edit.undo", .. })));
        let bad = CommandSpec::new("x.bad", "Bad", ok).with_shortcut("Cmd+");
        assert!(matches!(r.register(bad), Err(RegistryError::BadShortcut { .. })));
        // A failed registration leaves nothing behind.
        assert!(r.get("x.clash").is_none() && r.get("x.bad").is_none());
        assert_eq!(r.shortcut_of("edit.redo").map(ToString::to_string).as_deref(), Some("Cmd+Shift+Z"));
    }

    #[test]
    fn menu_tree_groups_by_path() {
        let mut r = CommandRegistry::<i32>::with_history_commands();
        r.register(CommandSpec::new("file.save", "Save", ok).in_menu(&["File"]).with_shortcut("Cmd+S")).unwrap();
        r.register(CommandSpec::new("file.png", "PNG", ok).in_menu(&["File", "Export"])).unwrap();
        r.register(CommandSpec::new("edit.cut", "Cut", ok).in_menu(&["Edit"]).separated()).unwrap();
        r.register(CommandSpec::new("hidden", "Hidden", ok)).unwrap();
        let tree = r.menu_tree();
        let names: Vec<_> = tree.iter().filter_map(|n| if let MenuNode::Menu { name, .. } = n { Some(*name) } else { None }).collect();
        assert_eq!(names, ["Edit", "File"]);
        let Some(MenuNode::Menu { children, .. }) = tree.first() else { panic!("no edit menu") };
        let ids: Vec<_> = children.iter().filter_map(|n| if let MenuNode::Item { id, .. } = n { Some(*id) } else { None }).collect();
        assert_eq!(ids, ["edit.undo", "edit.redo", "edit.cut"]);
        let Some(MenuNode::Menu { children, .. }) = tree.get(1) else { panic!("no file menu") };
        assert!(matches!(children.get(1), Some(MenuNode::Menu { name: "Export", .. })));
    }

    #[test]
    fn param_helpers() {
        let p = serde_json::json!({ "n": 3, "s": "x" });
        assert_eq!(param::<i32>(&p, "n"), Ok(3));
        assert!(param::<i32>(&p, "s").is_err());
        assert!(param::<i32>(&p, "missing").is_err());
        assert_eq!(param_opt::<i32>(&p, "missing"), Ok(None));
        assert_eq!(param_opt::<i32>(&p, "n"), Ok(Some(3)));
        assert!(param_opt::<i32>(&p, "s").is_err());
        assert_eq!(parse_params::<P>(&p).unwrap().n, 3);
        assert!(parse_params::<P>(&Value::Null).is_err());
    }

    #[test]
    fn schema_defaults_to_object() {
        let s = CommandSpec::<i32>::new("a", "A", ok);
        assert_eq!(s.schema_value(), serde_json::json!({ "type": "object" }));
        let s = s.with_schema(r#"{"type":"object","required":["n"]}"#);
        assert_eq!(s.schema_value()["required"][0], "n");
    }
}
