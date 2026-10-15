//! Errors from the editing helpers and the media store.

use crate::document::TimelineId;

/// Why an edit or a store operation was refused. The document is left unchanged when an error is returned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SceneError {
    /// The timeline does not exist.
    #[error("no such timeline {0:?}")]
    NoSuchTimeline(TimelineId),
    /// The layer index is out of range.
    #[error("no layer {layer} in the timeline (it has {count} layers)")]
    NoSuchLayer { layer: usize, count: usize },
    /// No keyframe covers the frame.
    #[error("no frame at {frame} on layer {layer}")]
    NoFrame { layer: usize, frame: u32 },
    /// The library has no item with that name.
    #[error("no library item named {0:?}")]
    NoSuchItem(String),
    /// A library item with that name exists.
    #[error("a library item named {0:?} already exists")]
    NameTaken(String),
    /// A name that cannot be used.
    #[error("invalid name {0:?}")]
    InvalidName(String),
    /// The frame is already a keyframe.
    #[error("frame {frame} on layer {layer} is already a keyframe")]
    AlreadyKeyframe { layer: usize, frame: u32 },
    /// A count or range that makes no sense (zero frames, beyond limits).
    #[error("invalid range: {0}")]
    InvalidRange(&'static str),
    /// A tween needs a keyframe after the span.
    #[error("the keyframe at {frame} on layer {layer} has no keyframe after it to tween to")]
    NoEndKeyframe { layer: usize, frame: u32 },
    /// A tween of this kind cannot be made from this content.
    #[error("cannot tween here: {0}")]
    NotTweenable(&'static str),
    /// Element index out of range.
    #[error("no element {element} in the keyframe (it has {count})")]
    NoSuchElement { element: usize, count: usize },
    /// Nothing was selected.
    #[error("empty selection")]
    EmptySelection,
    /// Layer relationship that Animate cannot represent.
    #[error("invalid layer parent: {0}")]
    InvalidParent(&'static str),
    /// A limit (frames, layers, items, bytes) was exceeded.
    #[error("limit exceeded: {0}")]
    LimitExceeded(&'static str),
    /// A symbol cannot contain itself.
    #[error("symbol {0:?} would contain itself")]
    Cycle(String),
}
