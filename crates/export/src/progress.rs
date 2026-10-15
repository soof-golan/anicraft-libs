//! Progress reporting and cancellation.

use crate::error::{ExportError, Result};

/// What an export is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Producing and rasterising a frame (or serialising it, for SVG).
    Rendering,
    /// Packing frames into a sheet.
    Packing,
    /// Encoding and writing the final output.
    Writing,
}

/// One progress report. `done` counts finished units out of `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub stage: Stage,
    pub done: u32,
    pub total: u32,
}

impl Progress {
    /// Completed fraction, 0.0 ..= 1.0 (1.0 when `total` is zero).
    #[must_use]
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        let (done, total) = (f64::from(self.done.min(self.total)), f64::from(self.total));
        anim_core::num::f64_to_f32(done / total)
    }
}

/// The progress callback: return `false` to cancel. It is called before each unit of work with the number of units
/// already finished, and once at the end with `done == total` (whose return value is ignored).
pub type ProgressFn<'a> = &'a mut dyn FnMut(Progress) -> bool;

/// Reports progress; a `false` answer becomes [`ExportError::Cancelled`].
pub fn report(progress: &mut dyn FnMut(Progress) -> bool, stage: Stage, done: u32, total: u32) -> Result<()> {
    if progress(Progress { stage, done, total }) { Ok(()) } else { Err(ExportError::Cancelled) }
}

/// Final report; cancellation no longer matters.
pub fn report_done(progress: &mut dyn FnMut(Progress) -> bool, stage: Stage, total: u32) {
    let _ = progress(Progress { stage, done: total, total });
}

/// A progress callback that never cancels and ignores reports.
#[must_use]
pub const fn no_progress(_: Progress) -> bool {
    true
}
