//! The `AniCraft` document model: Animate/XFL semantics as plain serde data.
//!
//! `anim-scene` owns what a document *is*; it reads and writes no files (that is `anicraft-xfl`) and draws nothing
//! (that is `anim-timeline`). The shape mirrors XFL closely enough that loading and saving lose nothing:
//!
//! - [`Document`]: stage size, frame rate, background, [`Document::scenes`] (a [`Timeline`] each) and the [`Library`].
//! - [`Library`] of [`LibraryItem`]s: [`Symbol`] (graphic / movie clip / button, with its own [`Timeline`]),
//!   [`BitmapItem`], [`SoundItem`], [`FontItem`], [`VideoItem`], [`FolderItem`], addressed by path-like names.
//! - [`Timeline`] of [`Layer`]s of [`Frame`]s (keyframes with their spans, tweens, eases, labels, sounds, scripts) of
//!   [`Element`]s: [`ShapeElement`] (an `anim_shape::EdgeShape`), [`SymbolInstance`], [`BitmapInstance`],
//!   [`TextElement`] (runs with font, size, colour, alignment and direction, so Hebrew and Arabic text is carried as
//!   Unicode with an explicit or automatic base direction), [`Group`].
//! - [`Extra`]: unknown attributes and child elements kept verbatim on every XFL-shaped struct, so a writer can
//!   emit what it did not understand.
//! - [`MediaStore`]: content-addressed bytes for bitmaps, sounds and video, held *outside* the cloneable document so
//!   undo snapshots stay cheap.
//! - Queries ([`Timeline::keyframe_at`], [`Timeline::frame_count`], [`Timeline::layer_tree`],
//!   [`Document::references`], bounds), [`Document::validate`] returning [`Warning`]s, and pure editing functions in
//!   [`edit`] (insert frames and keyframes, remove frames, tweens, convert to symbol, layers, scenes, renames).
//!
//! Units: pixels, y down, matrices as `kurbo::Affine` in Flash's `a b c d tx ty` order. Frame numbers are 0-based
//! `u32`. Times are frame indices; convert with [`Document::frame_rate_exact`].

#![cfg_attr(
    test,
    allow(clippy::assert_is_empty, clippy::field_reassign_with_default, reason = "assertions on emptiness and default-then-tweak setup read better in tests")
)]

pub mod bounds;
pub mod document;
pub mod edit;
pub mod element;
pub mod error;
pub mod extra;
pub mod library;
pub mod media;
pub mod query;
pub mod timeline;
pub mod validate;

pub use bounds::{MAX_SYMBOL_DEPTH, element_bounds, elements_bounds, timeline_bounds};
pub use document::{Document, LAYER_COLORS, TimelineId};
pub use element::{
    BitmapInstance, ColorEffect, Element, Group, InstanceFilter, LoopMode, ShapeElement, SymbolInstance, SymbolKind, TextAlign, TextDirection, TextElement,
    TextKind, TextOrientation, TextRun,
};
pub use error::SceneError;
pub use extra::{Extra, RawXml};
pub use library::{BitmapCompression, BitmapItem, FolderItem, FontItem, Library, LibraryItem, Linkage, SoundItem, Symbol, VideoItem};
pub use media::{MediaId, MediaStore};
pub use query::{DocumentStats, LayerNode, RefKind, Reference, shape_bitmap_paths};
pub use timeline::{
    EaseCurve, EaseTarget, EnvelopePoint, Frame, FrameSound, KEY_MODE_CLASSIC_TWEEN, KEY_MODE_NORMAL, KEY_MODE_SHAPE_TWEEN, LabelType, Layer, LayerKind,
    ShapeHint, SoundLoop, SoundSync, Timeline, TweenEase, TweenRotate, TweenType,
};
pub use validate::{Location, Warning, WarningKind};

/// Deepest group nesting that walks and bounds follow; anything deeper is ignored rather than recursed into.
pub const MAX_GROUP_DEPTH: usize = 64;
