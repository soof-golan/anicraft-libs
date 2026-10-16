//! Helpers for the predict / render / inspect tests: output folder, PNG decode, crop and zoom, pixel probes, and the
//! external checkers (headless Chrome for SVG, ffmpeg for GIF). External tools are used for verification only; every
//! test that needs one skips itself (and says so) when the tool is missing.

#![allow(
    clippy::pedantic,
    clippy::nursery,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::chunks_exact_to_as_chunks
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use anim_export::RgbaFrame;

/// Where renders go: `ANICRAFT_VISUAL_OUT`, else the shared scratch folder when it exists, else the system temp dir.
pub fn out_dir() -> PathBuf {
    let dir = std::env::var_os("ANICRAFT_VISUAL_OUT").map(PathBuf::from).unwrap_or_else(|| {
        let scratch = PathBuf::from("/mnt/data/anim/scratch/visual");
        if scratch.is_dir() { scratch } else { std::env::temp_dir() }
    });
    let dir = dir.join("export-video");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn save(name: &str, frame: &RgbaFrame) -> PathBuf {
    let path = out_dir().join(format!("{name}.png"));
    std::fs::write(&path, frame.to_png().unwrap()).unwrap();
    path
}

pub fn decode_png(bytes: &[u8]) -> RgbaFrame {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        other => panic!("unexpected PNG colour type {other:?}"),
    };
    RgbaFrame { width: info.width, height: info.height, rgba }
}

pub fn px(frame: &RgbaFrame, x: u32, y: u32) -> [u8; 4] {
    assert!(x < frame.width && y < frame.height, "probe ({x},{y}) outside {}x{}", frame.width, frame.height);
    let i = ((y * frame.width + x) * 4) as usize;
    [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2], frame.rgba[i + 3]]
}

/// Crops `(x, y, w, h)` and scales it up `k` times with nearest neighbour.
pub fn crop_zoom(frame: &RgbaFrame, x: u32, y: u32, w: u32, h: u32, k: u32) -> RgbaFrame {
    let (w, h) = (w.min(frame.width.saturating_sub(x)), h.min(frame.height.saturating_sub(y)));
    let mut rgba = Vec::with_capacity((w * h * k * k * 4) as usize);
    for oy in 0..h * k {
        for ox in 0..w * k {
            rgba.extend_from_slice(&px(frame, x + ox / k, y + oy / k));
        }
    }
    RgbaFrame { width: w * k, height: h * k, rgba }
}

/// Composites onto an opaque colour so two pictures can be compared whatever their alpha.
pub fn flatten(frame: &RgbaFrame, bg: [u8; 3]) -> RgbaFrame {
    let mut out = frame.clone();
    for p in out.rgba.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        for c in 0..3 {
            p[c] = ((u32::from(p[c]) * a + u32::from(bg[c]) * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
    out
}

/// `(mean absolute RGB error, fraction of pixels with any channel differing by more than `threshold`)`.
pub fn compare(a: &RgbaFrame, b: &RgbaFrame, threshold: u8) -> (f64, f64) {
    assert_eq!((a.width, a.height), (b.width, b.height), "size mismatch");
    let (mut sum, mut over) = (0u64, 0u64);
    for (p, q) in a.rgba.chunks_exact(4).zip(b.rgba.chunks_exact(4)) {
        let mut worst = 0u8;
        for c in 0..3 {
            let d = p[c].abs_diff(q[c]);
            sum += u64::from(d);
            worst = worst.max(d);
        }
        over += u64::from(worst > threshold);
    }
    let n = f64::from(a.width * a.height);
    (sum as f64 / (n * 3.0), over as f64 / n)
}

/// A difference picture (absolute error x4, opaque) for the eye.
pub fn diff_image(a: &RgbaFrame, b: &RgbaFrame) -> RgbaFrame {
    let rgba = a
        .rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .flat_map(|(p, q)| [0, 1, 2].map(|c| p[c].abs_diff(q[c]).saturating_mul(4)).into_iter().chain([255]))
        .collect();
    RgbaFrame { width: a.width, height: a.height, rgba }
}

/// `Some(message)` when the pixel is not within `tol` of `want`.
pub fn miss(frame: &RgbaFrame, x: u32, y: u32, want: [u8; 3], tol: u8, what: &str) -> Option<String> {
    let got = px(frame, x, y);
    (0..3).any(|c| got[c].abs_diff(want[c]) > tol).then(|| format!("{what}: pixel ({x},{y}) is {got:?}, want {want:?} +-{tol}"))
}

#[track_caller]
pub fn near(frame: &RgbaFrame, x: u32, y: u32, want: [u8; 3], tol: u8, what: &str) {
    let got = px(frame, x, y);
    let bad = (0..3).any(|c| got[c].abs_diff(want[c]) > tol);
    assert!(!bad, "{what}: pixel ({x},{y}) is {got:?}, want {want:?} +-{tol}");
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file()))
}

pub fn chrome() -> Option<PathBuf> {
    std::env::var_os("CHROME")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| ["/opt/google/chrome/chrome", "/usr/bin/google-chrome", "/usr/bin/chromium"].iter().map(PathBuf::from).find(|p| p.is_file()))
        .or_else(|| which("google-chrome").or_else(|| which("chromium")))
}

pub fn ffmpeg() -> Option<PathBuf> {
    which("ffmpeg")
}

pub fn ffprobe() -> Option<PathBuf> {
    which("ffprobe")
}

/// Rasterises `svg` in headless Chrome at `w` x `h` (virtual time `virtual_ms` advances animations first). `None` when Chrome is absent.
pub fn chrome_raster(name: &str, svg: &str, w: u32, h: u32, virtual_ms: Option<u32>) -> Option<RgbaFrame> {
    let Some(chrome) = chrome() else {
        eprintln!("SKIP {name}: no Chrome found (set CHROME)");
        return None;
    };
    let dir = out_dir();
    let svg_path = dir.join(format!("{name}.svg"));
    std::fs::write(&svg_path, svg).unwrap();
    let shot = dir.join(format!("{name}.chrome.png"));
    let _ = std::fs::remove_file(&shot);
    let profile = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(chrome);
    cmd.arg("--headless=new")
        .arg("--no-sandbox")
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg("--force-device-scale-factor=1")
        .arg("--default-background-color=00000000")
        .arg(format!("--user-data-dir={}", profile.path().display()))
        // Headless Chrome paints only the part of a small window that is not covered by its (invisible) browser chrome, so the
        // window is made taller than the picture and the picture is cropped out of the top-left corner.
        .arg(format!("--window-size={w},{}", h + 200))
        .arg(format!("--screenshot={}", shot.display()));
    // Without a budget the screenshot can be taken before every tile is rasterised (a half-painted picture).
    cmd.arg("--run-all-compositor-stages-before-draw").arg(format!("--virtual-time-budget={}", virtual_ms.unwrap_or(500)));
    let out = cmd.arg(format!("file://{}", svg_path.display())).output().unwrap();
    assert!(shot.is_file(), "Chrome wrote no screenshot: {}", String::from_utf8_lossy(&out.stderr));
    let frame = decode_png(&std::fs::read(&shot).unwrap());
    assert!(frame.width >= w && frame.height >= h, "Chrome screenshot is {}x{}", frame.width, frame.height);
    Some(crop_zoom(&frame, 0, 0, w, h, 1))
}

/// Every frame of `path` as ffmpeg composites it (disposal and transparency applied), as straight RGBA. `None` without ffmpeg.
pub fn ffmpeg_frames(name: &str, path: &Path) -> Option<Vec<RgbaFrame>> {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("SKIP {name}: no ffmpeg found");
        return None;
    };
    let dir = tempfile::tempdir().unwrap();
    let pattern = dir.path().join("f%03d.png");
    let out = Command::new(ffmpeg).args(["-v", "error", "-y", "-i"]).arg(path).args(["-vsync", "0", "-pix_fmt", "rgba"]).arg(&pattern).output().unwrap();
    assert!(out.status.success(), "ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr));
    let mut frames = Vec::new();
    for i in 1.. {
        let file = dir.path().join(format!("f{i:03}.png"));
        if !file.is_file() {
            break;
        }
        frames.push(decode_png(&std::fs::read(file).unwrap()));
    }
    Some(frames)
}

pub fn path_str(p: &Path) -> String {
    p.display().to_string()
}
