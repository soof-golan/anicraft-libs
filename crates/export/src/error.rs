//! Export errors.

use anim_display::ImageError;
use anim_render::RenderError;

/// Everything that can stop an export.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The progress callback returned `false`. Files written so far by sequence exports are kept; single-file
    /// exports remove their partial output.
    #[error("export cancelled")]
    Cancelled,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("render error: {0}")]
    Render(#[from] RenderError),
    #[error("png encoding failed: {0}")]
    Png(#[from] png::EncodingError),
    #[error("gif encoding failed: {0}")]
    Gif(#[from] gif::EncodingError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("image error: {0}")]
    Image(#[from] ImageError),
    /// The source has no frames, or the requested range is empty.
    #[error("there are no frames to export")]
    NoFrames,
    #[error("frame {index} is out of range (the source has {count} frames)")]
    FrameOutOfRange { index: u32, count: u32 },
    /// A scale, size or frame rate that is zero, negative, not finite or beyond the supported range.
    #[error("invalid export setting: {0}")]
    InvalidSetting(String),
    /// The result would exceed a safety limit (pixels, bytes, frames, nodes).
    #[error("export exceeds a limit: {0}")]
    TooLarge(String),
    /// The frames do not fit in a sprite sheet of the permitted size.
    #[error("{frames} frames do not fit in a sprite sheet of at most {max}x{max} pixels")]
    SheetTooSmall { frames: usize, max: u32 },
}

/// `Result` with [`ExportError`].
pub type Result<T> = std::result::Result<T, ExportError>;
