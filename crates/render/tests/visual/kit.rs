//! Helpers for the visual fixtures: output folders, nearest-neighbour zoom crops, golden comparison and probes.

use std::path::{Path, PathBuf};

use anim_display::Image;
use anim_render::straight_rgba;

use crate::common::pixel;

/// Folder with the `.expect.md` predictions and the PNG goldens.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/visual")
}

/// Where people look at the renders: `$ANICRAFT_VISUAL_OUT/shape-render`, or nowhere when the variable is unset (CI).
pub fn out_dir() -> Option<PathBuf> {
    anim_testkit::out_dir("shape-render")
}

fn write_png(path: &Path, width: u32, height: u32, straight: &[u8]) {
    let _ = anim_testkit::write_png(path, width, height, straight);
}

/// Nearest-neighbour crop of `image` (x, y, w, h), each source pixel `scale` pixels wide. Straight RGBA bytes.
pub fn zoom(image: &Image, x: u32, y: u32, w: u32, h: u32, scale: u32) -> (u32, u32, Vec<u8>) {
    let crop = anim_testkit::crop_zoom(&straight_rgba(image), image.width, x, y, w, h, scale);
    (crop.width, crop.height, crop.rgba)
}

/// Writes a zoomed crop as `<name>.png` in the scratch folder, for the Read tool.
pub fn save_crop(name: &str, image: &Image, x: u32, y: u32, w: u32, h: u32, scale: u32) {
    let Some(dir) = out_dir() else { return };
    let (cw, ch, bytes) = zoom(image, x, y, w, h, scale);
    write_png(&dir.join(format!("{name}.png")), cw, ch, &bytes);
}

/// Saves the render (and an enlarged copy when it is small) to the scratch folder, then compares it with the golden
/// `tests/fixtures/visual/<name>.png` (per-channel tolerance 6, since SIMD paths differ in the last bit between CPUs).
/// A missing golden is created (and the test fails when `CI` is set), so that the image gets looked at before it is
/// committed.
pub fn finish(name: &str, image: &Image) {
    let straight = straight_rgba(image);
    if let Some(dir) = out_dir() {
        anim_testkit::save_for_looking(&dir, name, image.width, image.height, &straight);
    }
    let golden = fixture_dir().join(format!("{name}.png"));
    match anim_testkit::check_golden(&golden, image.width, image.height, &straight, 6) {
        Ok(anim_testkit::Golden::Matched) => {}
        Ok(anim_testkit::Golden::Written) => eprintln!("golden {} was created: look at it, then commit it", golden.display()),
        Err(message) => panic!("{name}: {message}"),
    }
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
