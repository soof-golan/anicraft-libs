//! Shared egui widgets for `AniCraft` and `CharCraft`.
//!
//! Every widget is a pure function of state that returns events (`ui(&State) -> Vec<Event>`): nothing is stored
//! inside a widget between frames, so the apps keep all state in their models and apply the events as commands.
//!
//! | Module | Entry point | Returns |
//! |---|---|---|
//! | [`stage`] | [`stage_view`] | [`StageEvent`] (pointer in stage coordinates, view changes, guides, drops) |
//! | [`timeline`] | [`timeline::timeline`] | [`TimelineEvent`] (scrub, select, move, context menu, layers, scroll, zoom) |
//! | [`timeline`] | [`layers_list`] | [`LayerEvent`] |
//! | [`property`] | [`property_grid`] | [`PropertyEdit`] |
//! | [`library`] | [`library_panel`] | [`LibraryEvent`] |
//! | [`transport`] | [`transport_bar`] | [`TransportEvent`] |
//! | [`toolbar`] | [`toolbar::toolbar`] | [`ToolbarEvent`] |
//! | [`dock`] | [`dock::show_dock`] | craft-ui dock actions |
//!
//! Geometry (frame hit testing, scroll and zoom maths, ruler ticks, handle positions) is plain functions with unit
//! tests next to the code. [`theme`] holds the dark and light palettes and the right-to-left helpers: egui lays
//! glyphs out left to right, so every label goes through [`theme::visual`], which puts Hebrew and Arabic runs in
//! visual order.

// View models carry several independent flags by design (visible/locked/outline, show_grid/show_rulers, ...).
#![allow(clippy::struct_excessive_bools, reason = "view models are flag sets")]
// Widget names repeat their module on purpose (`stage::StageEvent`) so call sites read well when imported flat.
#![allow(clippy::module_name_repetitions, reason = "flat re-exports need self-describing names")]

pub mod dock;
pub mod icons;
pub mod library;
pub mod property;
pub mod stage;
pub mod theme;
pub mod timeline;
pub mod toolbar;
pub mod transport;
mod util;
pub mod widgets;

pub use dock::{PanelId, default_layout};
pub use icons::Icon;
pub use library::{LibraryAction, LibraryEvent, LibraryItemView, LibraryKind, LibraryModel, library_panel};
pub use property::{PropValue, Property, PropertyEdit, property_grid};
pub use stage::{Guide, Overlay, StageEvent, StageView, StageViewState, stage_view};
pub use theme::{Palette, Theme};
pub use timeline::{LayerEvent, LayerRow, Marker, Span, TimelineEvent, TimelineModel, TimelineViewState, layers_list, timeline};
pub use toolbar::{ToolItem, ToolbarEvent, toolbar};
pub use transport::{TransportEvent, TransportState, transport_bar};
