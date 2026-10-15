//! Errors from rendering.

/// Why a render could not start. Content problems (a missing image, a non-finite path, an over-deep tree) never
/// fail a render: the offending node is skipped and counted in [`crate::RenderStats`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// Width or height is zero or above [`crate::MAX_DIMENSION`].
    #[error("output size {width}x{height} is invalid: each side must be 1..={max}")]
    InvalidSize { width: u32, height: u32, max: u32 },
    /// The pixel count is above [`crate::MAX_PIXELS`].
    #[error("output of {width}x{height} pixels is above the budget of {max} pixels")]
    TooManyPixels { width: u32, height: u32, max: usize },
    /// The stage-to-pixels transform contains NaN or infinity.
    #[error("the view transform is not finite")]
    InvalidView,
    /// The pixmap could not be created (size overflow).
    #[error("could not allocate a {width}x{height} pixmap")]
    Allocation { width: u32, height: u32 },
    /// The finished pixels did not make a valid [`anim_display::Image`].
    #[error(transparent)]
    Image(#[from] anim_display::ImageError),
}
