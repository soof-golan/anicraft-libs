//! Helpers for the visual fixtures: output folders, nearest-neighbour zoom crops, golden comparison and probes.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anim_display::Image;
use anim_render::straight_rgba;

use crate::common::pixel;

/// Folder with the `.expect.md` predictions and the PNG goldens.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/visual")
}

/// Where people look at the renders: the shared scratch area when it exists, else the temp folder.
pub fn out_dir() -> PathBuf {
    let shared = Path::new("/mnt/data/anim/scratch");
    let dir = if shared.is_dir() { shared.join("visual/shape-render") } else { std::env::temp_dir().join("anim-visual-shape-render") };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn write_png(path: &Path, width: u32, height: u32, straight: &[u8]) {
    let Ok(file) = File::create(path) else { return };
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let Ok(mut writer) = encoder.write_header() else { return };
    let _ = writer.write_image_data(straight);
}

/// Nearest-neighbour crop of `image` (x, y, w, h), each source pixel `scale` pixels wide. Straight RGBA bytes.
pub fn zoom(image: &Image, x: u32, y: u32, w: u32, h: u32, scale: u32) -> (u32, u32, Vec<u8>) {
    let straight = straight_rgba(image);
    let (w, h) = (w.min(image.width.saturating_sub(x)), h.min(image.height.saturating_sub(y)));
    let mut out = Vec::with_capacity((w * h * scale * scale * 4) as usize);
    for oy in 0..h * scale {
        for ox in 0..w * scale {
            let at = (((y + oy / scale) * image.width + x + ox / scale) * 4) as usize;
            out.extend_from_slice(&straight[at..at + 4]);
        }
    }
    (w * scale, h * scale, out)
}

/// Writes a zoomed crop as `<name>.png` in the scratch folder, for the Read tool.
pub fn save_crop(name: &str, image: &Image, x: u32, y: u32, w: u32, h: u32, scale: u32) {
    let (cw, ch, bytes) = zoom(image, x, y, w, h, scale);
    write_png(&out_dir().join(format!("{name}.png")), cw, ch, &bytes);
}

/// Saves the render (and an enlarged copy when it is small) to the scratch folder, then compares it with the golden
/// `tests/fixtures/visual/<name>.png` (per-channel tolerance 6, since SIMD paths differ in the last bit between CPUs).
/// A missing golden is created (and the test fails when `CI` is set), so that the image gets looked at before it is
/// committed.
pub fn finish(name: &str, image: &Image) {
    let straight = straight_rgba(image);
    write_png(&out_dir().join(format!("{name}.png")), image.width, image.height, &straight);
    let big = (900 / image.width.max(1)).clamp(1, 6);
    if big > 1 {
        save_crop(&format!("{name}@{big}x"), image, 0, 0, image.width, image.height, big);
    }
    let golden = fixture_dir().join(format!("{name}.png"));
    let Ok(file) = File::open(&golden) else {
        assert!(std::env::var_os("CI").is_none(), "golden {} is missing", golden.display());
        write_png(&golden, image.width, image.height, &straight);
        eprintln!("golden {} did not exist and was created: look at it, then commit it", golden.display());
        return;
    };
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    assert_eq!((info.width, info.height), (image.width, image.height), "{name}: golden size");
    let worst = buffer.iter().zip(&straight).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(worst <= 6, "{name}: differs from its golden by {worst} (tolerance 6)");
}

/// Ink of a black-on-white pixel: 1 for black, 0 for white (from the red channel).
pub fn ink(image: &Image, x: u32, y: u32) -> f64 {
    1.0 - f64::from(pixel(image, x, y)[0]) / 255.0
}

/// Sum of the ink of a rectangle of pixels.
pub fn ink_sum(image: &Image, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> f64 {
    ys.flat_map(|y| xs.clone().map(move |x| (x, y))).map(|(x, y)| ink(image, x, y)).sum()
}

/// Asserts `a` is within `tolerance` of `b`.
#[track_caller]
pub fn assert_close(what: &str, a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{what}: {a} is not within {tolerance} of {b}");
}

/// Asserts a pixel is within `tolerance` per channel, naming the check.
#[track_caller]
pub fn assert_rgb(what: &str, image: &Image, x: u32, y: u32, expected: [u8; 3], tolerance: u8) {
    let got = pixel(image, x, y);
    let close = got.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= tolerance);
    assert!(close && got[3] == 255, "{what}: pixel ({x}, {y}) is {got:?}, expected {expected:?} +-{tolerance} opaque");
}

/// White opaque pixel check.
#[track_caller]
pub fn assert_white(what: &str, image: &Image, x: u32, y: u32) {
    assert_rgb(what, image, x, y, [255, 255, 255], 2);
}

/// Opaque pure-colour check with a small tolerance.
#[track_caller]
pub fn assert_color(what: &str, image: &Image, x: u32, y: u32, expected: [u8; 3]) {
    assert_rgb(what, image, x, y, expected, 2);
}
