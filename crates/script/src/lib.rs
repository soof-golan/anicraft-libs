//! The `ActionScript` subset `AniCraft` understands.
//!
//! - [`lex`]: a tokenizer that covers the whole source, for syntax highlighting in the Actions panel and as the
//!   parser's input.
//! - [`parse`]: a parser for the subset that keeps going after what it cannot read ([`Script::diagnostics`]).
//! - [`player`]: [`Player`], the Test Movie runtime: it plays the main timeline, runs frame scripts as the playhead
//!   enters their frames and delivers button events to the handlers the scripts registered. The module documentation
//!   lists exactly what is supported.
//! - [`ast`]: the syntax tree, which the SWF exporter compiles to `ActionScript` 3 bytecode or `ActionScript` 2 actions.
//!
//! There is no general virtual machine: loops, classes, arrays, objects and the rest of the Flash API are outside the
//! subset, and are reported rather than guessed at.

pub mod ast;
pub mod lex;
pub mod parse;
pub mod player;
pub mod value;

pub use ast::{BinOp, Diagnostic, Expr, Func, Script, Stmt, UnOp};
pub use lex::{Token, TokenKind, tokenize};
pub use parse::parse;
pub use player::{ClipState, Player, Version, instance_name, scene_frame_count, script_version};
pub use value::Value;
