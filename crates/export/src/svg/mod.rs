//! SVG export: a static SVG of one display list, and an animated SVG of a frame sequence.
//!
//! Mapping from the display list:
//! - fills and strokes become `<path>` (solid colours, `linearGradient`/`radialGradient` with `gradientTransform`,
//!   bitmap fills as `<pattern>`); text is already outlines;
//! - bitmaps become `<image>` definitions (base64 PNG, once per image) used with `<use>`;
//! - groups become `<g>` with a `transform`, `opacity`, `mix-blend-mode`, a `<mask mask-type="alpha">` for the group
//!   mask and a `<filter>` for blur, drop shadow, glow, colour matrix and non-trivial colour transforms;
//! - meshes become one clipped, affinely mapped copy of the content per triangle.
//!
//! Approximations (SVG has no equivalent): `StrokeScale::Horizontal/Vertical` render as normal scaling; bitmap fills
//! always repeat; the Flash blend modes Subtract, Invert, Alpha and Erase draw as Normal; filters and masks are
//! computed in the group's local space, so a skewed group blurs along its own axes.

mod emit;
mod filter;
mod fmt;

use std::fmt::Write as _;
use std::fs;
use std::ops::Range;
use std::path::Path;

use anim_core::num::round_f64_to_i64;
use anim_core::{Affine, Size};
use anim_display::{DisplayList, ImageStore};
use serde::{Deserialize, Serialize};

use self::emit::Emitter;
use self::fmt::num;
use crate::error::{ExportError, Result};
use crate::progress::{Progress, Stage, report, report_done};
use crate::source::FrameSource;

/// Static SVG settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SvgOptions {
    /// Multiplies the `width`/`height` attributes (the `viewBox` stays in stage pixels).
    pub scale: f64,
    /// Decimals kept in coordinates.
    pub precision: u8,
    /// Draw the stage background as a rectangle.
    pub background: bool,
    /// Add `data-key` to groups that carry a [`anim_display::Group::key`].
    pub emit_keys: bool,
    /// Refuse to produce more than this many bytes.
    pub max_bytes: usize,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self { scale: 1.0, precision: 3, background: true, emit_keys: false, max_bytes: 256 * 1024 * 1024 }
    }
}

/// How frames are switched in an animated SVG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SvgAnimation {
    /// SMIL `<animate attributeName="visibility">` inside each frame group.
    #[default]
    Smil,
    /// A `<style>` element with one `@keyframes` rule per frame group.
    Css,
}

/// Animated SVG settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimatedSvgOptions {
    pub svg: SvgOptions,
    pub animation: SvgAnimation,
    /// Loop forever (otherwise play once and hold the last frame).
    pub repeat: bool,
    /// Frames to export (`None` = all).
    pub frames: Option<Range<u32>>,
    /// Write a frame that equals the one before it once and keep it visible instead of duplicating it.
    pub merge_holds: bool,
}

impl Default for AnimatedSvgOptions {
    fn default() -> Self {
        Self { svg: SvgOptions::default(), animation: SvgAnimation::Smil, repeat: true, frames: None, merge_holds: true }
    }
}

fn check_options(opts: &SvgOptions, stage: Size) -> Result<()> {
    if !opts.scale.is_finite() || opts.scale <= 0.0 {
        return Err(ExportError::InvalidSetting(format!("scale {} must be finite and positive", opts.scale)));
    }
    if !(stage.width.is_finite() && stage.height.is_finite()) || stage.width <= 0.0 || stage.height <= 0.0 {
        return Err(ExportError::InvalidSetting(format!("stage size {}x{} is not usable", stage.width, stage.height)));
    }
    Ok(())
}

fn header(stage: Size, opts: &SvgOptions) -> String {
    let p = opts.precision;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        num(stage.width * opts.scale, p),
        num(stage.height * opts.scale, p),
        num(stage.width, p),
        num(stage.height, p)
    )
}

fn assemble(head: &str, defs: &str, body: &str) -> String {
    let mut svg = String::with_capacity(head.len().saturating_add(defs.len()).saturating_add(body.len()).saturating_add(32));
    svg.push_str(head);
    if !defs.is_empty() {
        svg.push_str("<defs>\n");
        svg.push_str(defs);
        svg.push_str("</defs>\n");
    }
    svg.push_str(body);
    svg.push_str("</svg>\n");
    svg
}

/// Converts one display list into a static SVG document.
///
/// # Errors
/// [`ExportError::InvalidSetting`] for a bad scale or stage, [`ExportError::TooLarge`] past the node or byte limit.
pub fn display_list_to_svg(list: &DisplayList, images: &ImageStore, options: &SvgOptions) -> Result<String> {
    check_options(options, list.size)?;
    let mut emitter = Emitter::new(images, options, list.size);
    let mut body = String::new();
    if options.background
        && let Some(bg) = Emitter::background(list, list.size, options.precision)
    {
        body.push_str(&bg);
    }
    emitter.nodes(&list.nodes, &mut body, Affine::IDENTITY, 0)?;
    Ok(assemble(&header(list.size, options), &emitter.defs, &body))
}

/// Converts frame `index` of a source into a static SVG document.
///
/// # Errors
/// As [`display_list_to_svg`], or [`ExportError::FrameOutOfRange`].
pub fn frame_to_svg(source: &dyn FrameSource, index: u32, options: &SvgOptions) -> Result<String> {
    let count = source.frame_count();
    if index >= count {
        return Err(ExportError::FrameOutOfRange { index, count });
    }
    let mut list = source.display_list(index);
    list.size = source.size();
    display_list_to_svg(&list, source.images(), options)
}

/// Writes a static SVG of frame `index` to `path`.
///
/// # Errors
/// As [`frame_to_svg`], plus i/o errors.
pub fn export_svg_file(source: &dyn FrameSource, index: u32, path: &Path, options: &SvgOptions) -> Result<()> {
    let svg = frame_to_svg(source, index, options)?;
    write_text(path, &svg)
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text.as_bytes())?;
    Ok(())
}

/// One distinct frame and the runs of frame positions during which it shows.
struct FrameGroup {
    body: String,
    /// Half-open runs of sequence positions.
    runs: Vec<Range<u32>>,
}

/// The `values`/`keyTimes` of the visibility animation of a group: `(time 0..=1, visible)` pairs, or `None` when
/// the group is visible for the whole animation.
fn visibility_track(runs: &[Range<u32>], total: u32) -> Option<Vec<(f64, bool)>> {
    if runs.len() == 1 && runs.first().is_some_and(|r| r.start == 0 && r.end >= total) {
        return None;
    }
    let at = |frame: u32| f64::from(frame) / f64::from(total.max(1));
    let first_visible = runs.first().is_some_and(|r| r.start == 0);
    let mut track = vec![(0.0, first_visible)];
    for run in runs {
        if run.start > 0 {
            track.push((at(run.start), true));
        }
        if run.end < total {
            track.push((at(run.end), false));
        }
    }
    let last_visible = track.last().is_some_and(|t| t.1);
    track.push((1.0, last_visible));
    Some(track)
}

fn seconds(frames: u32, fps: f64) -> String {
    // Round to microseconds so the attribute stays short and deterministic.
    let micros = round_f64_to_i64(f64::from(frames) / fps * 1e6);
    let mut s = format!("{}.{:06}", micros.div_euclid(1_000_000), micros.rem_euclid(1_000_000));
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// Renders a frame sequence as one animated SVG: one `<g>` per distinct frame, shown in turn at the source frame
/// rate. A cancelled export returns [`ExportError::Cancelled`].
///
/// # Errors
/// [`ExportError::NoFrames`], settings and limit errors, or [`ExportError::Cancelled`].
pub fn export_animated_svg(source: &dyn FrameSource, options: &AnimatedSvgOptions, progress: &mut dyn FnMut(Progress) -> bool) -> Result<String> {
    let stage = source.size();
    check_options(&options.svg, stage)?;
    let fps = source.fps();
    if !fps.is_finite() || fps <= 0.0 {
        return Err(ExportError::InvalidSetting(format!("frame rate {fps} must be finite and positive")));
    }
    let count = source.frame_count();
    let range = options.frames.clone().unwrap_or(0..count);
    let end = range.end.min(count);
    if range.start >= end {
        return Err(ExportError::NoFrames);
    }
    let total = end.saturating_sub(range.start);

    let mut emitter = Emitter::new(source.images(), &options.svg, stage);
    let mut groups: Vec<FrameGroup> = Vec::new();
    let mut previous: Option<DisplayList> = None;
    let mut backgrounds: Vec<Option<String>> = Vec::new();
    for (ordinal, index) in (range.start..end).enumerate() {
        let ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
        report(progress, Stage::Rendering, ordinal, total)?;
        let mut list = source.display_list(index);
        list.size = stage;
        if options.merge_holds
            && previous.as_ref() == Some(&list)
            && let Some(run) = groups.last_mut().and_then(|g| g.runs.last_mut())
        {
            run.end = ordinal.saturating_add(1);
            continue;
        }
        let mut body = String::new();
        emitter.nodes(&list.nodes, &mut body, Affine::IDENTITY, 0)?;
        backgrounds.push(Emitter::background(&list, stage, options.svg.precision).filter(|_| options.svg.background));
        let run = Range { start: ordinal, end: ordinal.saturating_add(1) };
        groups.push(FrameGroup { body, runs: vec![run] });
        previous = Some(list);
    }

    // A background shared by every frame is drawn once, outside the frame groups.
    let shared_bg = backgrounds.first().cloned().flatten().filter(|first| backgrounds.iter().all(|b| b.as_ref() == Some(first)));
    let duration = seconds(total, fps);
    let mut body = shared_bg.clone().unwrap_or_default();
    let mut css = String::new();
    for (i, group) in groups.iter().enumerate() {
        let id = format!("frame{i}");
        let track = visibility_track(&group.runs, total);
        let initially = if track.as_ref().is_none_or(|t| t.first().is_some_and(|f| f.1)) { "visible" } else { "hidden" };
        let _ = writeln!(body, "<g id=\"{id}\" visibility=\"{initially}\">");
        if shared_bg.is_none()
            && let Some(Some(bg)) = backgrounds.get(i)
        {
            body.push_str(bg);
        }
        body.push_str(&group.body);
        if let Some(track) = &track {
            match options.animation {
                SvgAnimation::Smil => {
                    let values: Vec<&str> = track.iter().map(|t| visibility_name(t.1)).collect();
                    let times: Vec<String> = track.iter().map(|t| num(t.0, 6)).collect();
                    let repeat = if options.repeat { "repeatCount=\"indefinite\"" } else { "repeatCount=\"1\" fill=\"freeze\"" };
                    let _ = writeln!(
                        body,
                        "<animate attributeName=\"visibility\" calcMode=\"discrete\" values=\"{}\" keyTimes=\"{}\" dur=\"{duration}s\" {repeat}/>",
                        values.join(";"),
                        times.join(";")
                    );
                }
                SvgAnimation::Css => {
                    let _ = write!(css, "@keyframes k{i}{{");
                    for (t, visible) in track {
                        let _ = write!(css, "{}%{{visibility:{}}}", num(t * 100.0, 4), visibility_name(*visible));
                    }
                    css.push('}');
                    let repeat = if options.repeat { "infinite" } else { "1 forwards" };
                    let _ = write!(css, "#{id}{{animation:k{i} {duration}s step-end {repeat}}}");
                }
            }
        }
        body.push_str("</g>\n");
    }
    if !css.is_empty() {
        body.insert_str(0, &format!("<style>{css}</style>\n"));
    }
    report_done(progress, Stage::Rendering, total);
    let svg = assemble(&header(stage, &options.svg), &emitter.defs, &body);
    if svg.len() > options.svg.max_bytes {
        return Err(ExportError::TooLarge(format!("SVG output exceeds {} bytes", options.svg.max_bytes)));
    }
    Ok(svg)
}

const fn visibility_name(visible: bool) -> &'static str {
    if visible { "visible" } else { "hidden" }
}

/// Writes the animated SVG to `path`.
///
/// # Errors
/// As [`export_animated_svg`], plus i/o errors.
pub fn export_animated_svg_file(source: &dyn FrameSource, path: &Path, options: &AnimatedSvgOptions, progress: &mut dyn FnMut(Progress) -> bool) -> Result<()> {
    let svg = export_animated_svg(source, options, progress)?;
    write_text(path, &svg)
}
