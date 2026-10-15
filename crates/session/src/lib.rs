//! The application framework shared by `AniCraft` and `CharCraft`.
//!
//! * [`command`]: every action is a [`CommandSpec`] in a [`CommandRegistry`] (id, label, menu path, shortcut,
//!   enabled predicate, `run(&mut D, &Value)`); menus and shortcut maps are derived from it.
//! * [`history`]: snapshot undo/redo with labels, merge keys, drag gestures, step and byte budgets, and a clean marker
//!   that follows undo.
//! * [`session`]: [`Session`] ties document, history, file path and registry together; `execute(id, params)` is the one
//!   way to change the document, and saving goes through `craft-atomic`.
//! * [`job`]: background work as actor threads with progress and cancellation over channels; panics become errors.
//! * [`prefs`]: settings pieces for `craft-settings` (`config.toml` in the platform config folder), recent files.
//! * [`mcp`]: a Model Context Protocol server derived from the registry, with stdio, TCP and in-process transports.

#![cfg_attr(
    test,
    allow(clippy::arithmetic_side_effects, clippy::assert_is_empty, clippy::unnecessary_wraps, clippy::branches_sharing_code, reason = "test code")
)]

pub mod command;
pub mod history;
pub mod job;
pub mod mcp;
pub mod prefs;
pub mod session;
pub mod shortcut;

pub use command::{Action, CommandError, CommandRegistry, CommandSpec, HistoryMode, MenuNode, RegistryError, param, param_opt, parse_params, tool_name};
pub use history::{Budget, History, SizeHint};
pub use job::{JobContext, JobError, JobEvent, JobHandle, JobId, JobSet, JobStatus};
pub use mcp::{McpInbox, McpRequest, McpServer};
pub use prefs::{CommonSettings, HistorySettings, RecentFiles};
pub use session::{SaveError, Session};
pub use shortcut::{Shortcut, ShortcutError};

pub use craft_atomic;
pub use craft_settings;
