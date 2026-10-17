//! Test support shared by the `AniCraft` family. Only ever a dev-dependency.
//!
//! Every visual test tree used to carry its own PNG writer, nearest-neighbour zoom, golden comparison and output
//! folder lookup. They live here once. All pixel buffers are straight-alpha RGBA8, row-major, 4 bytes per pixel;
//! convert from premultiplied renders with `anim_render::straight_rgba` first.

use std::fs::File;
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};

/// A decoded or cropped straight-alpha RGBA8 image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Png {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of straight-alpha RGBA.
    pub rgba: Vec<u8>,
}

impl Png {
    /// The pixel at `(x, y)`, or transparent black outside the image.
    #[must_use]
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        pixel(&self.rgba, self.width, x, y)
    }

    /// Writes the image to `path`, creating parent folders.
    ///
    /// # Errors
    /// I/O or PNG encoding failures.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_png(path, self.width, self.height, &self.rgba)
    }
}

/// The pixel at `(x, y)` of a `width`-wide RGBA buffer, or transparent black outside the buffer.
#[must_use]
pub fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    if x >= width {
        return [0; 4];
    }
    let at = usize::try_from(y)
        .ok()
        .and_then(|row| row.checked_mul(usize::try_from(width).ok()?))
        .and_then(|row| row.checked_add(usize::try_from(x).ok()?))
        .and_then(|index| index.checked_mul(4));
    at.and_then(|at| rgba.get(at..at.checked_add(4)?)).and_then(|bytes| <[u8; 4]>::try_from(bytes).ok()).unwrap_or([0; 4])
}

/// The folder `$ANICRAFT_SAMPLES` names, or `None` when the variable is unset or the folder is missing.
///
/// The Adobe-authored samples are never committed, so every test or benchmark that reads them returns early on `None`.
#[must_use]
pub fn samples_dir() -> Option<PathBuf> {
    std::env::var_os("ANICRAFT_SAMPLES").map(PathBuf::from).filter(|dir| dir.is_dir())
}

/// `<samples folder>/<relative>`, or `None` when there is no samples folder or the file is not in it.
#[must_use]
pub fn sample(relative: impl AsRef<Path>) -> Option<PathBuf> {
    let path = samples_dir()?.join(relative);
    path.exists().then_some(path)
}

/// `$ANICRAFT_VISUAL_OUT/<component>` (created), or `None` when the variable is unset (CI): then nothing is written.
#[must_use]
pub fn out_dir(component: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("ANICRAFT_VISUAL_OUT")?).join(component);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Writes straight-alpha RGBA8 as a PNG, creating parent folders.
///
/// # Errors
/// I/O or PNG encoding failures (including a buffer that does not match the size).
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    encode_png_to(BufWriter::new(File::create(path)?), width, height, rgba)
}

/// Encodes straight-alpha RGBA8 as PNG bytes.
///
/// # Errors
/// PNG encoding failures (including a buffer that does not match the size).
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    encode_png_to(&mut out, width, height, rgba)?;
    Ok(out)
}

fn encode_png_to<W: io::Write>(sink: W, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    let mut encoder = png::Encoder::new(sink, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    writer.write_image_data(rgba).map_err(io::Error::other)?;
    writer.finish().map_err(io::Error::other)
}

/// Decodes a PNG of any colour type into straight-alpha RGBA8 (palettes and 16-bit expanded, grey and RGB given an
/// opaque alpha).
///
/// # Errors
/// I/O or PNG decoding failures.
pub fn read_png(path: &Path) -> io::Result<Png> {
    decode_png_from(BufReader::new(File::open(path)?))
}

/// [`read_png`] for PNG bytes in memory.
///
/// # Errors
/// PNG decoding failures.
pub fn decode_png(bytes: &[u8]) -> io::Result<Png> {
    decode_png_from(io::Cursor::new(bytes))
}

fn decode_png_from<R: io::BufRead + io::Seek>(source: R) -> io::Result<Png> {
    let mut decoder = png::Decoder::new(source);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(io::Error::other)?;
    let size = reader.output_buffer_size().ok_or_else(|| io::Error::other("png too large"))?;
    let mut buffer = vec![0; size];
    let info = reader.next_frame(&mut buffer).map_err(io::Error::other)?;
    buffer.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buffer.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        png::ColorType::Indexed => return Err(io::Error::other("indexed png was not expanded")),
    };
    Ok(Png { width: info.width, height: info.height, rgba })
}

/// Nearest-neighbour crop of the `w` x `h` region at `(x, y)` (clamped to the image), each source pixel `scale` pixels
/// wide. `scale` 1 is a plain crop.
#[must_use]
pub fn crop_zoom(rgba: &[u8], width: u32, x: u32, y: u32, w: u32, h: u32, scale: u32) -> Png {
    let scale = scale.max(1);
    let pixels = rgba.len().checked_div(4).unwrap_or(0);
    let rows = u32::try_from(pixels.checked_div(usize::try_from(width).unwrap_or(0)).unwrap_or(0)).unwrap_or(0);
    let (w, h) = (w.min(width.saturating_sub(x)), h.min(rows.saturating_sub(y)));
    let (out_w, out_h) = (w.saturating_mul(scale), h.saturating_mul(scale));
    let mut out = Vec::with_capacity(usize::try_from(out_w).unwrap_or(0).saturating_mul(usize::try_from(out_h).unwrap_or(0)).saturating_mul(4));
    for oy in 0..out_h {
        for ox in 0..out_w {
            let source = (x.saturating_add(ox.checked_div(scale).unwrap_or(0)), y.saturating_add(oy.checked_div(scale).unwrap_or(0)));
            out.extend_from_slice(&pixel(rgba, width, source.0, source.1));
        }
    }
    Png { width: out_w, height: out_h, rgba: out }
}

/// Writes `<dir>/<name>.png` 1:1 and, when the image is small, `<dir>/<name>@<k>x.png` enlarged.
///
/// The enlargement is about 900 pixels wide, at most 6x. Failures are ignored: these files are for people.
pub fn save_for_looking(dir: &Path, name: &str, width: u32, height: u32, rgba: &[u8]) {
    let _ = write_png(&dir.join(format!("{name}.png")), width, height, rgba);
    let big = 900_u32.checked_div(width).unwrap_or(1).clamp(1, 6);
    if big > 1 {
        let zoomed = crop_zoom(rgba, width, 0, 0, width, height, big);
        let _ = zoomed.save(&dir.join(format!("{name}@{big}x.png")));
    }
}

/// The text entries of a zipped FLA (or an XFL folder) whose names `keep` accepts, as `(name, lossy UTF-8 text)`.
///
/// Reads through `anim-archive`, which copes with the 54 bytes short central directory Animate writes.
///
/// # Errors
/// The archive error when the container cannot be opened or an accepted entry cannot be read.
pub fn archive_texts(path: &Path, keep: impl Fn(&str) -> bool) -> Result<Vec<(String, String)>, anim_archive::ArchiveError> {
    let container = anim_archive::Container::open(path)?;
    let mut out = Vec::new();
    for entry in container.files().filter(|entry| keep(&entry.name)) {
        out.push((entry.name.clone(), String::from_utf8_lossy(&container.read_entry(entry)?).into_owned()));
    }
    Ok(out)
}

/// The largest per-channel difference between two equally sized buffers (255 when the sizes differ).
#[must_use]
pub fn max_channel_diff(a: &[u8], b: &[u8]) -> u8 {
    if a.len() == b.len() { a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0) } else { u8::MAX }
}

/// What [`check_golden`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Golden {
    /// The render matched the committed file.
    Matched,
    /// The file was missing (or `UPDATE_GOLDENS` is set) and was written: look at it, then commit it.
    Written,
}

/// Compares straight RGBA8 with the PNG golden at `path`, per channel within `tolerance`.
///
/// SIMD paths differ in the last bit between CPUs, hence the tolerance. A missing golden is written, unless `CI` is
/// set; `UPDATE_GOLDENS` rewrites every golden.
///
/// # Errors
/// A message naming the golden when the size or pixels differ, or when it is missing on CI or unreadable.
pub fn check_golden(path: &Path, width: u32, height: u32, rgba: &[u8], tolerance: u8) -> Result<Golden, String> {
    let name = path.display();
    let update = std::env::var_os("UPDATE_GOLDENS").is_some();
    if update || !path.exists() {
        if !update && std::env::var_os("CI").is_some() {
            return Err(format!("golden {name} is missing"));
        }
        write_png(path, width, height, rgba).map_err(|e| format!("golden {name} could not be written: {e}"))?;
        return Ok(Golden::Written);
    }
    let golden = read_png(path).map_err(|e| format!("golden {name} is unreadable: {e}"))?;
    if (golden.width, golden.height) != (width, height) {
        return Err(format!("golden {name}: size is {}x{}, the render is {width}x{height}", golden.width, golden.height));
    }
    let worst = max_channel_diff(&golden.rgba, rgba);
    if worst > tolerance {
        return Err(format!("golden {name}: worst channel difference {worst} exceeds {tolerance}"));
    }
    Ok(Golden::Matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> Vec<u8> {
        vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 9, 9, 9, 0]
    }

    #[test]
    fn crop_zoom_repeats_pixels_and_clamps() {
        let zoomed = crop_zoom(&checker(), 2, 1, 0, 5, 2, 2);
        assert_eq!((zoomed.width, zoomed.height), (2, 4));
        assert_eq!(zoomed.at(0, 0), [0, 255, 0, 128]);
        assert_eq!(zoomed.at(1, 1), [0, 255, 0, 128]);
        assert_eq!(zoomed.at(1, 3), [9, 9, 9, 0]);
        assert_eq!(zoomed.at(2, 0), [0; 4]);
    }

    #[test]
    fn png_round_trip_and_golden_checks() {
        let dir = std::env::temp_dir().join(format!("anim-testkit-{}", std::process::id()));
        let path = dir.join("sub/round.png");
        write_png(&path, 2, 2, &checker()).unwrap();
        let back = read_png(&path).unwrap();
        assert_eq!(back, Png { width: 2, height: 2, rgba: checker() });
        assert_eq!(decode_png(&encode_png(2, 2, &checker()).unwrap()).unwrap(), back);
        assert_eq!(check_golden(&path, 2, 2, &checker(), 0), Ok(Golden::Matched));
        let mut near = checker();
        near[0] = 250;
        assert_eq!(check_golden(&path, 2, 2, &near, 5), Ok(Golden::Matched));
        assert!(check_golden(&path, 2, 2, &near, 4).is_err());
        assert!(check_golden(&path, 1, 4, &checker(), 0).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sample_lookup_needs_the_folder_and_the_file() {
        // The variable is process-wide and tests run in parallel threads, so only the unset-safe parts are asserted.
        assert_eq!(sample("no-such-sample-file.fla"), None);
        if let Some(dir) = samples_dir() {
            assert!(dir.is_dir());
        }
    }

    #[test]
    fn pixel_outside_is_transparent() {
        assert_eq!(pixel(&checker(), 2, 2, 0), [0; 4]);
        assert_eq!(pixel(&checker(), 2, 0, 5), [0; 4]);
    }
}
