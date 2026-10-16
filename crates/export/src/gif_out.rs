//! Animated GIF export.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use anim_core::num::{i64_to_f64, round_f64_to_i64, u32_to_usize};
use anim_render::straight_rgba;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::{ExportError, Result};
use crate::png_out::ExportReport;
use crate::progress::Progress;
use crate::raster::{RasterOptions, for_each_frame};
use crate::source::FrameSource;

/// How often the animation plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GifLoop {
    /// Loops forever (the NETSCAPE extension with count 0).
    #[default]
    Forever,
    /// Plays once; no loop extension is written.
    Once,
    /// Repeats this many times after the first play (the NETSCAPE loop count; 0 means [`Self::Once`]).
    Repeat(u16),
}

/// Animated GIF settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GifOptions {
    pub raster: RasterOptions,
    pub loop_mode: GifLoop,
    /// Colour quantiser effort, 1 (best, slowest) ..= 30 (fastest). Default 10.
    pub speed: u8,
    /// Pixels with alpha below this are fully transparent, the rest fully opaque (GIF has 1-bit alpha).
    pub alpha_threshold: u8,
    /// Smallest frame delay in hundredths of a second. Browsers treat delays under 2 as 10, so 2 is the default.
    pub min_delay_cs: u16,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self { raster: RasterOptions::default(), loop_mode: GifLoop::Forever, speed: 10, alpha_threshold: 128, min_delay_cs: 2 }
    }
}

/// Frame delays in hundredths of a second for `count` frames at `fps`, keeping the cumulative time within half a
/// hundredth of the ideal even though each delay is a whole number.
///
/// # Errors
/// [`ExportError::InvalidSetting`] when `fps` is not finite and positive.
pub fn frame_delays_cs(count: u32, fps: f64, min_delay_cs: u16) -> Result<Vec<u16>> {
    if !fps.is_finite() || fps <= 0.0 {
        return Err(ExportError::InvalidSetting(format!("frame rate {fps} must be finite and positive")));
    }
    let mut delays = Vec::new();
    let mut elapsed: i64 = 0;
    for i in 0..count {
        let ideal_end = round_f64_to_i64(i64_to_f64(i64::from(i).saturating_add(1)) * 100.0 / fps);
        let wanted = ideal_end.saturating_sub(elapsed).max(i64::from(min_delay_cs)).max(1);
        let delay = u16::try_from(wanted).unwrap_or(u16::MAX);
        elapsed = elapsed.saturating_add(i64::from(delay));
        delays.push(delay);
    }
    Ok(delays)
}

/// Writes the selected frames as an animated GIF to `writer`.
///
/// # Errors
/// Render, range or encoding errors, or [`ExportError::Cancelled`].
pub fn export_gif<W: Write>(source: &dyn FrameSource, writer: W, options: &GifOptions, progress: &mut dyn FnMut(Progress) -> bool) -> Result<ExportReport> {
    write_gif(source, writer, options, progress).map(|(_, report)| report)
}

fn write_gif<W: Write>(source: &dyn FrameSource, writer: W, options: &GifOptions, progress: &mut dyn FnMut(Progress) -> bool) -> Result<(W, ExportReport)> {
    let range = options.raster.resolve_range(source.frame_count())?;
    let total = range.end.saturating_sub(range.start);
    let (width, height) = options.raster.output_size(source.size())?;
    let (Ok(w16), Ok(h16)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(ExportError::TooLarge(format!("{width}x{height} does not fit a GIF (65535 pixels per side)")));
    };
    let delays = frame_delays_cs(total, source.fps(), options.min_delay_cs)?;
    let speed = i32::from(options.speed.clamp(1, 30));

    let mut encoder = gif::Encoder::new(writer, w16, h16, &[])?;
    match options.loop_mode {
        GifLoop::Forever => encoder.set_repeat(gif::Repeat::Infinite)?,
        GifLoop::Repeat(n) if n > 0 => encoder.set_repeat(gif::Repeat::Finite(n))?,
        GifLoop::Once | GifLoop::Repeat(_) => {}
    }
    // Quantising a frame (NeuQuant) dominates the export and frames are independent, so rendered frames wait in a batch
    // that is quantised on all cores and then written in order. The batch is capped by memory as well as by core count.
    let frame_bytes = u32_to_usize(width).saturating_mul(u32_to_usize(height)).saturating_mul(4).max(1);
    let batch_len = rayon::current_num_threads().min(BATCH_BYTES.checked_div(frame_bytes).unwrap_or(1)).max(1);
    let mut delay_iter = delays.into_iter();
    let mut batch: Vec<(Vec<u8>, bool, u16)> = Vec::with_capacity(batch_len);
    for_each_frame(source, &options.raster, progress, |_, _, image| {
        let mut pixels = straight_rgba(image);
        let mut any_transparent = false;
        let (chunks, _) = pixels.as_chunks_mut::<4>();
        for [_, _, _, alpha] in chunks {
            if *alpha < options.alpha_threshold {
                *alpha = 0;
                any_transparent = true;
            } else {
                *alpha = 255;
            }
        }
        batch.push((pixels, any_transparent, delay_iter.next().unwrap_or_else(|| options.min_delay_cs.max(1))));
        if batch.len() >= batch_len {
            write_batch(&mut encoder, &mut batch, (w16, h16), speed)?;
        }
        Ok(())
    })?;
    write_batch(&mut encoder, &mut batch, (w16, h16), speed)?;
    let writer = encoder.into_inner()?;
    Ok((writer, ExportReport { frames: total, files: Vec::new(), bytes: 0 }))
}

/// Pixel bytes that may wait for quantising at once.
const BATCH_BYTES: usize = 256 * 1024 * 1024;

/// Quantises the batched frames in parallel and writes them in order. Empties `batch`.
fn write_batch<W: Write>(encoder: &mut gif::Encoder<W>, batch: &mut Vec<(Vec<u8>, bool, u16)>, (w, h): (u16, u16), speed: i32) -> Result<()> {
    let frames: Vec<gif::Frame<'static>> = std::mem::take(batch)
        .into_par_iter()
        .map(|(mut pixels, any_transparent, delay)| {
            let mut frame = gif::Frame::from_rgba_speed(w, h, &mut pixels, speed);
            frame.delay = delay;
            frame.dispose = if any_transparent { gif::DisposalMethod::Background } else { gif::DisposalMethod::Keep };
            frame
        })
        .collect();
    for frame in &frames {
        encoder.write_frame(frame)?;
    }
    Ok(())
}

/// Writes an animated GIF to the file `path`. A cancelled or failed export removes the partial file.
///
/// # Errors
/// As [`export_gif`], plus i/o errors creating the file.
pub fn export_gif_file(source: &dyn FrameSource, path: &Path, options: &GifOptions, progress: &mut dyn FnMut(Progress) -> bool) -> Result<ExportReport> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let file = BufWriter::new(fs::File::create(path)?);
    let written = write_gif(source, file, options, progress).and_then(|(mut file, report)| {
        file.flush()?;
        Ok(report)
    });
    match written {
        Ok(mut report) => {
            report.files.push(path.to_path_buf());
            report.bytes = fs::metadata(path).map_or(0, |m| m.len());
            Ok(report)
        }
        Err(err) => {
            let _ = fs::remove_file(path);
            Err(err)
        }
    }
}
