//! Sprite sheets: frames packed into one PNG plus JSON metadata in the common "JSON hash" layout (the format
//! `TexturePacker` calls "JSON (Hash)", read by Phaser, `PixiJS`, Cocos and most 2D engines).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anim_core::num::{ceil_f64_to_i32, u32_to_usize};
use anim_display::Image;
use anim_render::MAX_PIXELS;
use serde::{Deserialize, Serialize};

use crate::error::{ExportError, Result};
use crate::png_out::{ExportReport, PngSequenceOptions, check_prefix, encode_png};
use crate::progress::{Progress, Stage, report, report_done};
use crate::raster::{RasterOptions, RgbaFrame, for_each_frame};
use crate::source::FrameSource;

/// How frames are arranged on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SheetLayout {
    /// Uniform cells in rows. `columns: None` picks the column count that gives the smallest sheet.
    Grid { columns: Option<u32> },
    /// Tight packing of (trimmed) frames with the best-short-side-fit max-rects heuristic.
    #[default]
    MaxRects,
}

/// Sprite sheet settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetOptions {
    pub raster: RasterOptions,
    pub layout: SheetLayout,
    /// Empty pixels between frames.
    pub padding: u32,
    /// Crop transparent borders from each frame (the metadata records the offset and original size).
    pub trim: bool,
    /// Largest sheet side in pixels.
    pub max_size: u32,
    /// Round the sheet sides up to powers of two.
    pub power_of_two: bool,
    /// Frame name prefix (no path separators); names are `<prefix><number>.png`.
    pub prefix: String,
    pub start_number: u32,
    pub min_digits: u32,
    /// Name of the PNG recorded in the metadata (`export_sprite_sheet` replaces it with the real file name).
    pub image_name: String,
    /// Name of the animation listing every frame in order (empty = no `animations` entry).
    pub animation_name: String,
}

impl Default for SheetOptions {
    fn default() -> Self {
        Self {
            raster: RasterOptions::default(),
            layout: SheetLayout::default(),
            padding: 1,
            trim: false,
            max_size: 4096,
            power_of_two: false,
            prefix: "frame_".to_owned(),
            start_number: 1,
            min_digits: 4,
            image_name: "spritesheet.png".to_owned(),
            animation_name: "all".to_owned(),
        }
    }
}

/// A rectangle in sheet or source pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PxSize {
    pub w: u32,
    pub h: u32,
}

/// One frame's entry in the JSON hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetFrame {
    /// Where the (trimmed) pixels sit on the sheet.
    pub frame: PxRect,
    /// Always false: frames are never rotated.
    pub rotated: bool,
    pub trimmed: bool,
    /// Where the trimmed pixels sit inside the original frame.
    #[serde(rename = "spriteSourceSize")]
    pub sprite_source_size: PxRect,
    /// The original frame size.
    #[serde(rename = "sourceSize")]
    pub source_size: PxSize,
}

/// The `meta` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetMeta {
    pub app: String,
    pub version: String,
    pub image: String,
    pub format: String,
    pub size: PxSize,
    pub scale: String,
    /// Frames per second of the source (extra field; engines ignore it).
    pub fps: f64,
}

/// The JSON hash document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetMetadata {
    /// Frame name to placement. Sorted by name, which is frame order thanks to the zero padding.
    pub frames: BTreeMap<String, SheetFrame>,
    /// Animation name to ordered frame names.
    #[serde(default)]
    pub animations: BTreeMap<String, Vec<String>>,
    pub meta: SheetMeta,
}

/// A packed sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteSheet {
    pub image: RgbaFrame,
    pub metadata: SheetMetadata,
}

impl SpriteSheet {
    /// The metadata as pretty JSON.
    ///
    /// # Errors
    /// [`ExportError::Json`].
    pub fn json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.metadata)?)
    }
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Slot {
    const fn right(self) -> u32 {
        self.x.saturating_add(self.w)
    }

    const fn bottom(self) -> u32 {
        self.y.saturating_add(self.h)
    }

    const fn intersects(self, other: Self) -> bool {
        self.x < other.right() && other.x < self.right() && self.y < other.bottom() && other.y < self.bottom()
    }

    const fn contains(self, other: Self) -> bool {
        self.x <= other.x && self.y <= other.y && self.right() >= other.right() && self.bottom() >= other.bottom()
    }
}

/// Best-short-side-fit max-rects bin.
struct MaxRects {
    free: Vec<Slot>,
}

impl MaxRects {
    fn new(w: u32, h: u32) -> Self {
        Self { free: vec![Slot { x: 0, y: 0, w, h }] }
    }

    fn insert(&mut self, w: u32, h: u32) -> Option<Slot> {
        let mut best: Option<(u32, u32, Slot)> = None;
        for f in &self.free {
            if f.w >= w && f.h >= h {
                let (lw, lh) = (f.w.saturating_sub(w), f.h.saturating_sub(h));
                let key = (lw.min(lh), lw.max(lh));
                if best.is_none_or(|(a, b, _)| key < (a, b)) {
                    best = Some((key.0, key.1, Slot { x: f.x, y: f.y, w, h }));
                }
            }
        }
        let placed = best?.2;
        let mut next = Vec::with_capacity(self.free.len().saturating_add(4));
        for f in &self.free {
            if !f.intersects(placed) {
                next.push(*f);
                continue;
            }
            if placed.x > f.x {
                next.push(Slot { w: placed.x.saturating_sub(f.x), ..*f });
            }
            if placed.right() < f.right() {
                next.push(Slot { x: placed.right(), w: f.right().saturating_sub(placed.right()), ..*f });
            }
            if placed.y > f.y {
                next.push(Slot { h: placed.y.saturating_sub(f.y), ..*f });
            }
            if placed.bottom() < f.bottom() {
                next.push(Slot { y: placed.bottom(), h: f.bottom().saturating_sub(placed.bottom()), ..*f });
            }
        }
        // Drop free rectangles that lie inside another one.
        let keep: Vec<bool> = next
            .iter()
            .enumerate()
            .map(|(i, a)| !next.iter().enumerate().any(|(j, b)| i != j && b.contains(*a) && (!a.contains(*b) || j < i)))
            .collect();
        self.free = next.into_iter().zip(keep).filter_map(|(r, k)| k.then_some(r)).collect();
        Some(placed)
    }
}

/// Packs `sizes` (padding included) into a square bin of `side`; the result is in input order.
fn pack_in_bin(sizes: &[(u32, u32)], side: u32) -> Option<Vec<Slot>> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| {
        let (w, h) = sizes.get(i).copied().unwrap_or((0, 0));
        std::cmp::Reverse((w.max(h), u64::from(w).saturating_mul(u64::from(h))))
    });
    let mut bin = MaxRects::new(side, side);
    let mut slots = vec![Slot { x: 0, y: 0, w: 0, h: 0 }; sizes.len()];
    for i in order {
        let (w, h) = sizes.get(i).copied()?;
        *slots.get_mut(i)? = bin.insert(w, h)?;
    }
    Some(slots)
}

fn pack_max_rects(sizes: &[(u32, u32)], padding: u32, max_size: u32) -> Option<Vec<Slot>> {
    let padded: Vec<(u32, u32)> = sizes.iter().map(|&(w, h)| (w.saturating_add(padding), h.saturating_add(padding))).collect();
    let area: u64 = padded.iter().map(|&(w, h)| u64::from(w).saturating_mul(u64::from(h))).sum();
    let widest = padded.iter().map(|&(w, h)| w.max(h)).max().unwrap_or(1);
    let bin_limit = max_size.saturating_add(padding);
    // Smallest square that could hold the area, grown by an eighth until everything fits.
    let mut side = widest.max(u32::try_from(isqrt_ceil(area)).unwrap_or(u32::MAX));
    loop {
        let attempt = side.min(bin_limit);
        if let Some(slots) = pack_in_bin(&padded, attempt) {
            // Remove the padding from the slot sizes again.
            return Some(slots.into_iter().zip(sizes).map(|(s, &(w, h))| Slot { x: s.x, y: s.y, w, h }).collect());
        }
        if attempt >= bin_limit {
            return None;
        }
        side = side.saturating_add((side / 8).max(1));
    }
}

/// `ceil(sqrt(n))`, saturating (areas beyond `u32::MAX` are far past any permitted sheet).
fn isqrt_ceil(n: u64) -> u64 {
    let n = u32::try_from(n).unwrap_or(u32::MAX);
    u64::from(u32::try_from(ceil_f64_to_i32(f64::from(n).sqrt())).unwrap_or(u32::MAX))
}

fn pack_grid(sizes: &[(u32, u32)], columns: Option<u32>, padding: u32, max_size: u32) -> Option<Vec<Slot>> {
    let n = u32::try_from(sizes.len()).ok()?;
    let cell_w = sizes.iter().map(|s| s.0).max()?;
    let cell_h = sizes.iter().map(|s| s.1).max()?;
    let dims = |cols: u32| {
        let rows = n.div_ceil(cols);
        let w = cols.saturating_mul(cell_w).saturating_add(cols.saturating_sub(1).saturating_mul(padding));
        let h = rows.saturating_mul(cell_h).saturating_add(rows.saturating_sub(1).saturating_mul(padding));
        (w, h)
    };
    let cols = match columns {
        Some(c) => c.clamp(1, n),
        None => (1..=n)
            .filter(|&c| {
                let (w, h) = dims(c);
                w <= max_size && h <= max_size
            })
            .min_by_key(|&c| {
                let (w, h) = dims(c);
                (u64::from(w).saturating_mul(u64::from(h)), w.abs_diff(h))
            })?,
    };
    let (w, h) = dims(cols);
    if w > max_size || h > max_size {
        return None;
    }
    Some(
        sizes
            .iter()
            .enumerate()
            .map(|(i, &(sw, sh))| {
                let i = u32::try_from(i).unwrap_or(u32::MAX);
                let (col, row) = (i.checked_rem(cols).unwrap_or(0), i.checked_div(cols).unwrap_or(0));
                Slot {
                    x: col.saturating_mul(cell_w.saturating_add(padding)),
                    y: row.saturating_mul(cell_h.saturating_add(padding)),
                    w: sw,
                    h: sh,
                }
            })
            .collect(),
    )
}

struct Cell {
    name: String,
    pixels: RgbaFrame,
    /// Offset of the cropped pixels inside the original frame.
    offset: (u32, u32),
    source: (u32, u32),
    trimmed: bool,
}

/// Crops transparent borders. A fully transparent frame keeps a single transparent pixel.
fn trim_frame(frame: &RgbaFrame) -> (RgbaFrame, (u32, u32)) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
    let mut any = false;
    for y in 0..frame.height {
        let Some(row) = frame.row(y) else { continue };
        let (pixels, _) = row.as_chunks::<4>();
        for (x, [_, _, _, alpha]) in (0..frame.width).zip(pixels) {
            if *alpha != 0 {
                any = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if !any {
        return (RgbaFrame { width: 1, height: 1, rgba: vec![0; 4] }, (0, 0));
    }
    let (w, h) = (max_x.saturating_sub(min_x).saturating_add(1), max_y.saturating_sub(min_y).saturating_add(1));
    let mut rgba = Vec::with_capacity(Image::byte_len(w, h).unwrap_or(0));
    for y in min_y..=max_y {
        let start = u32_to_usize(min_x).saturating_mul(4);
        let len = u32_to_usize(w).saturating_mul(4);
        if let Some(part) = frame.row(y).and_then(|row| row.get(start..start.saturating_add(len))) {
            rgba.extend_from_slice(part);
        }
    }
    (RgbaFrame { width: w, height: h, rgba }, (min_x, min_y))
}

fn blit(dst: &mut RgbaFrame, src: &RgbaFrame, x: u32, y: u32) {
    let stride = u32_to_usize(dst.width).saturating_mul(4);
    let row_len = u32_to_usize(src.width).saturating_mul(4);
    for row in 0..src.height {
        let Some(from) = src.row(row) else { continue };
        let start = u32_to_usize(y.saturating_add(row)).saturating_mul(stride).saturating_add(u32_to_usize(x).saturating_mul(4));
        if let Some(to) = dst.rgba.get_mut(start..start.saturating_add(row_len)) {
            to.copy_from_slice(from);
        }
    }
}

/// Renders the selected frames and packs them.
///
/// # Errors
/// Render and range errors, [`ExportError::SheetTooSmall`] when the frames do not fit `max_size`, or
/// [`ExportError::Cancelled`].
pub fn build_sprite_sheet(
    source: &dyn FrameSource,
    options: &SheetOptions,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<SpriteSheet> {
    check_prefix(&options.prefix)?;
    if options.max_size == 0 || options.max_size > anim_render::MAX_DIMENSION {
        return Err(ExportError::InvalidSetting(format!("max_size {} must be between 1 and {}", options.max_size, anim_render::MAX_DIMENSION)));
    }
    let range = options.raster.resolve_range(source.frame_count())?;
    let total = range.end.saturating_sub(range.start);
    let naming = PngSequenceOptions {
        raster: RasterOptions::default(),
        prefix: options.prefix.clone(),
        start_number: options.start_number,
        min_digits: options.min_digits,
    };
    let mut cells: Vec<Cell> = Vec::new();
    let mut budget: u64 = 0;
    for_each_frame(source, &options.raster, progress, |_, ordinal, image| {
        let full = RgbaFrame::from_image(image);
        let source_size = (full.width, full.height);
        let (pixels, offset) = if options.trim { trim_frame(&full) } else { (full, (0, 0)) };
        budget = budget.saturating_add(u64::from(pixels.width).saturating_mul(u64::from(pixels.height)));
        if usize::try_from(budget).map_or(true, |b| b > MAX_PIXELS) {
            return Err(ExportError::TooLarge(format!("the frames hold more than {MAX_PIXELS} pixels")));
        }
        let trimmed = (pixels.width, pixels.height) != source_size;
        cells.push(Cell { name: naming.file_name(ordinal, total), pixels, offset, source: source_size, trimmed });
        Ok(())
    })?;

    let sizes: Vec<(u32, u32)> = cells.iter().map(|c| (c.pixels.width, c.pixels.height)).collect();
    report(progress, Stage::Packing, 0, 1)?;
    let packed = match options.layout {
        SheetLayout::Grid { columns } => pack_grid(&sizes, columns, options.padding, options.max_size),
        SheetLayout::MaxRects => pack_max_rects(&sizes, options.padding, options.max_size),
    };
    let slots = packed.ok_or(ExportError::SheetTooSmall { frames: cells.len(), max: options.max_size })?;
    let (mut width, mut height) = slots.iter().fold((1, 1), |(w, h), s| (w.max(s.right()), h.max(s.bottom())));
    if options.power_of_two {
        width = width.checked_next_power_of_two().unwrap_or(u32::MAX);
        height = height.checked_next_power_of_two().unwrap_or(u32::MAX);
    }
    if width > options.max_size || height > options.max_size {
        return Err(ExportError::SheetTooSmall { frames: cells.len(), max: options.max_size });
    }
    let bytes = Image::byte_len(width, height).ok_or_else(|| ExportError::TooLarge(format!("{width}x{height} sheet")))?;
    let mut sheet = RgbaFrame { width, height, rgba: vec![0; bytes] };
    let mut frames = BTreeMap::new();
    let mut order = Vec::with_capacity(cells.len());
    for (cell, slot) in cells.iter().zip(&slots) {
        blit(&mut sheet, &cell.pixels, slot.x, slot.y);
        frames.insert(
            cell.name.clone(),
            SheetFrame {
                frame: PxRect { x: slot.x, y: slot.y, w: slot.w, h: slot.h },
                rotated: false,
                trimmed: cell.trimmed,
                sprite_source_size: PxRect { x: cell.offset.0, y: cell.offset.1, w: cell.pixels.width, h: cell.pixels.height },
                source_size: PxSize { w: cell.source.0, h: cell.source.1 },
            },
        );
        order.push(cell.name.clone());
    }
    let mut animations = BTreeMap::new();
    if !options.animation_name.is_empty() {
        animations.insert(options.animation_name.clone(), order);
    }
    let metadata = SheetMetadata {
        frames,
        animations,
        meta: SheetMeta {
            app: "anim-export".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            image: options.image_name.clone(),
            format: "RGBA8888".to_owned(),
            size: PxSize { w: width, h: height },
            scale: "1".to_owned(),
            fps: source.fps(),
        },
    };
    report_done(progress, Stage::Packing, 1);
    Ok(SpriteSheet { image: sheet, metadata })
}

/// Builds a sheet and writes `png_path` and `json_path`.
///
/// # Errors
/// As [`build_sprite_sheet`], plus i/o and encoding errors.
pub fn export_sprite_sheet(
    source: &dyn FrameSource,
    png_path: &Path,
    json_path: &Path,
    options: &SheetOptions,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<ExportReport> {
    let mut options = options.clone();
    if let Some(name) = png_path.file_name().and_then(|n| n.to_str()) {
        name.clone_into(&mut options.image_name);
    }
    let sheet = build_sprite_sheet(source, &options, progress)?;
    let png = encode_png(sheet.image.width, sheet.image.height, &sheet.image.rgba)?;
    let json = sheet.json()?;
    for path in [png_path, json_path] {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(png_path, &png)?;
    fs::write(json_path, json.as_bytes())?;
    let bytes = u64::try_from(png.len()).unwrap_or(u64::MAX).saturating_add(u64::try_from(json.len()).unwrap_or(u64::MAX));
    Ok(ExportReport { frames: u32::try_from(sheet.metadata.frames.len()).unwrap_or(u32::MAX), files: vec![png_path.to_path_buf(), json_path.to_path_buf()], bytes })
}
