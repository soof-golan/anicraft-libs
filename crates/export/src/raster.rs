//! Shared rasterising: options, frame iteration with progress and cancel.

use std::ops::Range;

use anim_core::num::{ceil_f64_to_i32, u32_to_usize};
use anim_core::{Rgba8, Size};
use anim_display::Image;
use anim_render::{Background, MAX_DIMENSION, RenderOptions, Renderer, straight_rgba};
use serde::{Deserialize, Serialize};

use crate::error::{ExportError, Result};
use crate::progress::{Stage, report, report_done};
use crate::source::FrameSource;

/// Where the backdrop comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExportBackground {
    /// The stage background of each display list (transparent when it has none).
    #[default]
    Stage,
    /// Transparent, whatever the stage says.
    Transparent,
    /// This colour, whatever the stage says.
    Solid(Rgba8),
}

/// How frames are rasterised, shared by every bitmap export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RasterOptions {
    /// Output pixels per stage pixel (1.0 = stage size).
    pub scale: f64,
    pub background: ExportBackground,
    pub antialias: bool,
    /// Frames to export (`None` = all). The end is clamped to the frame count.
    pub frames: Option<Range<u32>>,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self { scale: 1.0, background: ExportBackground::Stage, antialias: true, frames: None }
    }
}

impl RasterOptions {
    /// Options for exporting every frame at stage size.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn with_scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    #[must_use]
    pub const fn with_background(mut self, background: ExportBackground) -> Self {
        self.background = background;
        self
    }

    #[must_use]
    pub const fn with_frames(mut self, frames: Range<u32>) -> Self {
        self.frames = Some(frames);
        self
    }

    /// Validates the settings and returns the output size in pixels for a stage of `stage`.
    ///
    /// # Errors
    /// [`ExportError::InvalidSetting`] for a bad scale or stage; [`ExportError::TooLarge`] beyond
    /// [`MAX_DIMENSION`].
    pub fn output_size(&self, stage: Size) -> Result<(u32, u32)> {
        if !self.scale.is_finite() || self.scale <= 0.0 {
            return Err(ExportError::InvalidSetting(format!("scale {} must be finite and positive", self.scale)));
        }
        if !(stage.width.is_finite() && stage.height.is_finite()) || stage.width <= 0.0 || stage.height <= 0.0 {
            return Err(ExportError::InvalidSetting(format!("stage size {}x{} is not usable", stage.width, stage.height)));
        }
        let pixels = |extent: f64| u32::try_from(ceil_f64_to_i32((extent * self.scale).max(1.0))).unwrap_or(1).max(1);
        let (w, h) = (pixels(stage.width), pixels(stage.height));
        if w > MAX_DIMENSION || h > MAX_DIMENSION {
            return Err(ExportError::TooLarge(format!("{w}x{h} pixels exceeds {MAX_DIMENSION} on one side")));
        }
        Ok((w, h))
    }

    /// The renderer options for a stage of `stage`.
    pub(crate) fn render_options(&self, stage: Size) -> Result<RenderOptions> {
        self.output_size(stage)?;
        let background = match self.background {
            ExportBackground::Stage => Background::FromDisplayList,
            ExportBackground::Transparent => Background::Transparent,
            ExportBackground::Solid(c) => Background::Solid(c),
        };
        Ok(RenderOptions::scaled_stage(stage, self.scale).with_background(background).with_antialias(self.antialias))
    }

    /// The frame range to export for a source of `count` frames.
    ///
    /// # Errors
    /// [`ExportError::NoFrames`] when the range is empty, [`ExportError::FrameOutOfRange`] when it starts past the
    /// end.
    pub fn resolve_range(&self, count: u32) -> Result<Range<u32>> {
        let range = self.frames.clone().unwrap_or(0..count);
        if range.start >= count && range.start < range.end {
            return Err(ExportError::FrameOutOfRange { index: range.start, count });
        }
        let end = range.end.min(count);
        if range.start >= end {
            return Err(ExportError::NoFrames);
        }
        Ok(range.start..end)
    }
}

/// A straight-alpha RGBA8 picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, straight alpha, rows top to bottom.
    pub rgba: Vec<u8>,
}

impl RgbaFrame {
    pub(crate) fn from_image(image: &Image) -> Self {
        Self { width: image.width, height: image.height, rgba: straight_rgba(image) }
    }

    /// One row of pixels.
    #[must_use]
    pub fn row(&self, y: u32) -> Option<&[u8]> {
        let stride = u32_to_usize(self.width).checked_mul(4)?;
        let start = u32_to_usize(y).checked_mul(stride)?;
        self.rgba.get(start..start.checked_add(stride)?)
    }
}

/// Renders one frame.
///
/// # Errors
/// [`ExportError::FrameOutOfRange`] or a render/setting error.
pub fn render_frame(source: &dyn FrameSource, index: u32, options: &RasterOptions) -> Result<RgbaFrame> {
    let count = source.frame_count();
    if index >= count {
        return Err(ExportError::FrameOutOfRange { index, count });
    }
    let render = options.render_options(source.size())?;
    let list = source.display_list(index);
    let image = Renderer::new().render(&list, source.images(), &render)?;
    Ok(RgbaFrame::from_image(&image))
}

/// Renders the selected frames in order, handing each to `f` as `(frame index, ordinal, premultiplied image)`.
/// Reports progress before each frame and at the end.
pub fn for_each_frame(
    source: &dyn FrameSource,
    options: &RasterOptions,
    progress: &mut dyn FnMut(crate::Progress) -> bool,
    mut f: impl FnMut(u32, u32, &Image) -> Result<()>,
) -> Result<Range<u32>> {
    let range = options.resolve_range(source.frame_count())?;
    let render = options.render_options(source.size())?;
    let total = range.end.saturating_sub(range.start);
    let mut renderer = Renderer::new();
    for (ordinal, index) in range.clone().enumerate() {
        let ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
        report(progress, Stage::Rendering, ordinal, total)?;
        let list = source.display_list(index);
        let image = renderer.render(&list, source.images(), &render)?;
        f(index, ordinal, &image)?;
        renderer.recycle(image);
    }
    report_done(progress, Stage::Rendering, total);
    Ok(range)
}
