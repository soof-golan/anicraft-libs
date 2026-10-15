//! Where exports get their frames from.

use anim_core::Size;
use anim_display::{DisplayList, ImageStore};

/// A sequence of resolved frames. Implemented by the animation editors (timeline evaluation, character
/// performance playback); [`VecFrameSource`] covers tests and pre-baked sequences.
///
/// `display_list` is called once per exported frame, in order, so implementations may evaluate lazily.
pub trait FrameSource {
    /// Number of frames.
    fn frame_count(&self) -> u32;
    /// Stage size in stage pixels (every exporter uses this, not the per-list size).
    fn size(&self) -> Size;
    /// Frames per second (must be finite and positive for GIF and animated SVG).
    fn fps(&self) -> f64;
    /// The resolved frame `index` (0-based, `index < frame_count()`).
    fn display_list(&self, index: u32) -> DisplayList;
    /// Pixels that the display lists refer to.
    fn images(&self) -> &ImageStore;
}

/// A [`FrameSource`] over display lists held in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct VecFrameSource {
    pub size: Size,
    pub fps: f64,
    pub frames: Vec<DisplayList>,
    pub images: ImageStore,
}

impl VecFrameSource {
    #[must_use]
    pub const fn new(size: Size, fps: f64, frames: Vec<DisplayList>, images: ImageStore) -> Self {
        Self { size, fps, frames, images }
    }

    /// A one-frame source (static SVG, single PNG).
    #[must_use]
    pub fn single(list: DisplayList, images: ImageStore) -> Self {
        Self { size: list.size, fps: 24.0, frames: vec![list], images }
    }
}

impl FrameSource for VecFrameSource {
    fn frame_count(&self) -> u32 {
        u32::try_from(self.frames.len()).unwrap_or(u32::MAX)
    }

    fn size(&self) -> Size {
        self.size
    }

    fn fps(&self) -> f64 {
        self.fps
    }

    fn display_list(&self, index: u32) -> DisplayList {
        usize::try_from(index).ok().and_then(|i| self.frames.get(i)).cloned().unwrap_or_else(|| DisplayList::new(self.size, None))
    }

    fn images(&self) -> &ImageStore {
        &self.images
    }
}
