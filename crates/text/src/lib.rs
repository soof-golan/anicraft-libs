//! Rich text layout with full right-to-left and bidi support, for the `AniCraft` family.
//!
//! ```
//! use anim_text::{Alignment, Direction, Paragraph, RichText, Run, TextEngine};
//!
//! let mut engine = TextEngine::new(); // owns the fonts: create once and reuse
//! let text = RichText::new(vec![Paragraph {
//!     alignment: Alignment::Start,
//!     direction: Direction::Auto,
//!     runs: vec![Run { size: 32.0, ..Run::new("Hello, שלום, مرحبا") }],
//!     ..Paragraph::default()
//! }]);
//! let layout = engine.layout(&text, Some(300.0));
//! let nodes = engine.to_display(&layout); // filled glyph outlines for a DisplayList
//! let caret = layout.caret_at(5);
//! let _rect = layout.caret_rect(caret, 1.0); // where to draw the editing caret
//! # let _ = nodes;
//! ```
//!
//! # What lives where
//!
//! - [`model`]: [`RichText`] / [`Paragraph`] / [`Run`], plain serde data plus editing helpers (insert, delete, restyle).
//!   All byte offsets in the crate index into [`RichText::plain_text`] (paragraphs joined with `'\n'`).
//! - [`TextEngine`]: owns the font database and parley's layout context; [`TextEngine::layout`] shapes, bidi-reorders,
//!   breaks and aligns, giving a [`TextLayout`].
//! - [`TextLayout`]: lines, glyph runs, bounds, and the editor queries: [`hit_test`](TextLayout::hit_test),
//!   [`caret_rect`](TextLayout::caret_rect), [`selection_rects`](TextLayout::selection_rects),
//!   [`move_caret`](TextLayout::move_caret) (visual movement, correct in right-to-left and mixed text).
//! - [`TextEngine::to_display`]: glyph outlines (read with skrifa) as `anim_display` nodes, so renderers and exporters
//!   never shape text.
//! - Fonts: system fonts through fontique, registered font data, and per-script fallback chains ([`Script`]) found by
//!   probing what is installed. No fonts are bundled.
//!
//! Layout space is y down with the origin at the top-left of the text box; sizes and spacings are stage pixels.

pub mod model;
pub mod script;

mod engine;
mod extract;
mod fonts;
mod guard;
mod layout;
mod motion;
mod outline;
mod sanitize;

pub use engine::{MAX_PARAGRAPHS, MAX_TEXT_BYTES, TextEngine};
pub use layout::{FontFace, Glyph, GlyphRun, Line, ParagraphInfo, TextLayout};
pub use model::{Alignment, Direction, Paragraph, RichText, Run};
pub use motion::{Affinity, Caret, Motion, Selection};
pub use script::Script;
