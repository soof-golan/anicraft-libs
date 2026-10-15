//! The stage view: pan/zoom canvas with rulers, grid, guides, a document texture and tool overlays.
//!
//! [`stage_view`] is a pure function of a [`StageViewState`]: it draws, and returns [`StageEvent`]s (pointer events
//! already in stage coordinates, view changes, guide edits). Nothing is stored between frames except egui's own
//! transient drag bookkeeping.

mod overlay;
mod view;
mod widget;

pub use overlay::{HANDLE_SIZE, MarkerShape, Overlay, OverlayStyle, PathNode, ROTATE_OFFSET_PX, at, paint_overlay, selection_box};
pub use view::{HandleId, MAX_TICKS, MAX_ZOOM, MIN_ZOOM, RulerTick, StageView, hit_transform_handle, nice_step, ruler_ticks, snap_to_grid, transform_handles};
pub use widget::{Axis, Guide, PointerPhase, RULER_SIZE, StageEvent, StagePointer, StageViewState, WheelMode, stage_view};

/// Payload carried by a library item being dragged; [`stage_view`] turns its release over the stage into
/// [`StageEvent::DropLibraryItem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryDrag {
    pub id: u64,
}
