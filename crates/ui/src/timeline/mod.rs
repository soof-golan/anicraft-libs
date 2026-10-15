//! The timeline: layer column, frame grid, ruler and playhead as a pure function of a view model.
//!
//! [`timeline`] takes a [`TimelineModel`] (what the document looks like) and a [`TimelineViewState`] (scroll, zoom,
//! selection) and returns [`TimelineEvent`]s. The geometry (hit testing, scroll and zoom maths) lives in
//! [`geometry`] as plain functions with unit tests.

pub mod geometry;
mod layers;
pub mod model;
pub mod state;
mod widget;

pub use geometry::{FrameSelection, Hit, MAX_FRAME_WIDTH, MIN_FRAME_WIDTH, TimelineGeometry, layers_between};
pub use layers::{INDENT, LayerDrag, TOGGLE_WIDTH, layers_list};
pub use model::{LayerId, LayerKind, LayerRow, OnionSkin, Span, TimelineModel, TweenKind, Waveform};
pub use state::{DropPlacement, FrameAction, LayerEvent, TimelineEvent, TimelineViewState};
pub use widget::{BAR_HEIGHT, RULER_HEIGHT, drag_delta, frame_shortcuts, range_selection, ruler_label, ruler_label_step, timeline};
