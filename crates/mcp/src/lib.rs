//! The MCP (Model Context Protocol) layer shared by `AniCraft` and `CharCraft`.
//!
//! # Architecture
//!
//! ```text
//!  MCP client (Claude, ...)         anim-mcp process / app                          the editor
//!  ───────────────────────          ──────────────────────────────────────────      ─────────────────────────
//!  stdio / in-memory JSON-RPC ──►   AnimMcp (rmcp ServerHandler, front.rs)
//!                                     holds only a Backend = Sender<Incoming>
//!                                            │ Request + Ticket (call.rs)
//!                          ┌─────────────────┴──────────────────┐
//!                   Headless backend                    Bridge backend (bridge.rs)
//!                   actor thread owns a                 TCP, JSON lines, token  ──►  ControlServer (server.rs)
//!                   SessionHost (host.rs)                                              embedded in the app; its
//!                                                                                      ControlInbox is pumped by the UI
//!                                                                                      thread into a SessionHost
//! ```
//!
//! * [`protocol`]: the control vocabulary ([`Request`], [`Reply`], [`ControlError`]) and its JSON-lines wire form.
//! * [`call`]: the actor boundary. A [`Backend`] sends [`Incoming`] requests; the owner answers through a [`Ticket`]
//!   (now or later, with progress and cancellation).
//! * [`host`]: [`SessionHost`] applies requests to an `anim-session` `Session` and the application's [`AppHooks`]:
//!   commands, atomic batches (one undo step), properties, queries, render, screenshot, export, open/save, with path
//!   [`Confinement`] and a [`Journal`] of every call.
//! * [`headless`], [`bridge`], [`server`]: the two backends and the control-server half the apps embed.
//! * [`front`], [`tools`], [`serve`]: the rmcp server (tools generated from the command registry plus built-ins,
//!   resources, progress, cancellation) and transport helpers.
//!
//! The crate is generic: applications plug in only through [`AppHooks`] and their `CommandRegistry`. No `Arc`, no
//! `Mutex`: documents live on the thread that owns the `SessionHost`, and everything else talks to it over channels.

#![cfg_attr(
    test,
    allow(clippy::arithmetic_side_effects, clippy::assert_is_empty, clippy::unnecessary_wraps, clippy::branches_sharing_code, reason = "test code")
)]

pub mod bridge;
pub mod call;
pub mod confine;
pub mod front;
pub mod headless;
pub mod hooks;
pub mod host;
#[cfg(feature = "http")]
pub mod http;
pub mod journal;
pub mod props;
pub mod protocol;
pub mod serve;
pub mod server;
pub mod tools;
pub mod wire;

pub use bridge::{connect_bridge, connect_bridge_from_file};
pub use call::{Backend, Incoming, PendingCall, Ticket};
pub use confine::{Access, Confinement};
pub use front::{AnimMcp, PromptArg, PromptDef};
pub use headless::{HeadlessSession, spawn_headless};
pub use hooks::{AppHooks, ExportFormat, ExportRequest, HookOutcome, HookResult, QuerySpec, RenderRequest, ScreenshotRequest, run_job, save_image};
pub use host::{ControlHost, HostInfo, SessionHost};
pub use journal::{Journal, JournalEntry};
pub use protocol::{BatchStep, CommandInfo, ControlError, ErrorKind, Event, Image, Progress, Reply, Request, encode_png_rgba};
pub use serve::{ServeError, run_stdio, serve_io, serve_stdio};
pub use server::{ControlConfig, ControlInbox, ControlServer, generate_token, read_token_file, write_token_file};
