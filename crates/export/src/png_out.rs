//! PNG export: single frames and zero-padded sequences.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::error::{ExportError, Result};
use crate::progress::Progress;
use crate::raster::{RasterOptions, RgbaFrame, for_each_frame, render_frame};
use crate::source::FrameSource;
use anim_render::straight_rgba;
use serde::{Deserialize, Serialize};

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportReport {
    /// Frames rendered.
    pub frames: u32,
    /// Files created, in order.
    pub files: Vec<PathBuf>,
    /// Total bytes written.
    pub bytes: u64,
}

/// Encodes straight-alpha RGBA8 as a PNG.
///
/// # Errors
/// [`ExportError::Png`] when the buffer does not match the size or encoding fails.
pub fn encode_png(width: u32, height: u32, straight_rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    write_png(&mut out, width, height, straight_rgba)?;
    Ok(out)
}

pub fn write_png<W: Write>(writer: W, width: u32, height: u32, straight_rgba: &[u8]) -> Result<()> {
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(straight_rgba)?;
    writer.finish()?;
    Ok(())
}

/// Renders frame `index` and returns it as PNG bytes.
///
/// # Errors
/// Render, range or encoding errors.
pub fn png_frame_bytes(source: &dyn FrameSource, index: u32, options: &RasterOptions) -> Result<Vec<u8>> {
    let frame = render_frame(source, index, options)?;
    encode_png(frame.width, frame.height, &frame.rgba)
}

/// Renders frame `index` into the PNG file `path`.
///
/// # Errors
/// Render, range, i/o or encoding errors.
pub fn export_png_frame(source: &dyn FrameSource, index: u32, path: &Path, options: &RasterOptions) -> Result<ExportReport> {
    let bytes = png_frame_bytes(source, index, options)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &bytes)?;
    Ok(ExportReport { frames: 1, files: vec![path.to_path_buf()], bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX) })
}

/// File naming and range for a PNG sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PngSequenceOptions {
    pub raster: RasterOptions,
    /// File name prefix (no path separators), e.g. `"walk_"`.
    pub prefix: String,
    /// Number given to the first exported frame (Animate numbers from 1).
    pub start_number: u32,
    /// Minimum digits; names are zero-padded to at least this width and to fit the largest number.
    pub min_digits: u32,
}

impl Default for PngSequenceOptions {
    fn default() -> Self {
        Self { raster: RasterOptions::default(), prefix: "frame_".to_owned(), start_number: 1, min_digits: 4 }
    }
}

impl PngSequenceOptions {
    /// The file name for sequence position `ordinal` out of `total` frames.
    #[must_use]
    pub fn file_name(&self, ordinal: u32, total: u32) -> String {
        let last = self.start_number.saturating_add(total.saturating_sub(1));
        let digits = last.to_string().len().max(anim_core::num::u32_to_usize(self.min_digits));
        let number = self.start_number.saturating_add(ordinal);
        format!("{}{number:0digits$}.png", self.prefix)
    }
}

pub fn check_prefix(prefix: &str) -> Result<()> {
    if prefix.contains(['/', '\\', '\0', ':']) {
        return Err(ExportError::InvalidSetting(format!("file name prefix {prefix:?} must not contain path separators")));
    }
    Ok(())
}

/// Writes the selected frames as `dir/<prefix><NNNN>.png`.
///
/// # Errors
/// Render, range, i/o or encoding errors, or [`ExportError::Cancelled`] (files written so far stay).
pub fn export_png_sequence(
    source: &dyn FrameSource,
    dir: &Path,
    options: &PngSequenceOptions,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<ExportReport> {
    check_prefix(&options.prefix)?;
    let range = options.raster.resolve_range(source.frame_count())?;
    let total = range.end.saturating_sub(range.start);
    fs::create_dir_all(dir)?;
    let mut report = ExportReport::default();
    for_each_frame(source, &options.raster, progress, |_, ordinal, image| {
        let path = dir.join(options.file_name(ordinal, total));
        let file = BufWriter::new(fs::File::create(&path)?);
        write_png(file, image.width, image.height, &straight_rgba(image))?;
        report.bytes = report.bytes.saturating_add(fs::metadata(&path).map_or(0, |m| m.len()));
        report.files.push(path);
        report.frames = report.frames.saturating_add(1);
        Ok(())
    })?;
    Ok(report)
}

impl RgbaFrame {
    /// Encodes this picture as a PNG.
    ///
    /// # Errors
    /// [`ExportError::Png`].
    pub fn to_png(&self) -> Result<Vec<u8>> {
        encode_png(self.width, self.height, &self.rgba)
    }
}
