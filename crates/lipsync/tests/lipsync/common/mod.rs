//! Shared helpers of the visual tests: signal synthesis, viseme-track queries, PNG strips and crops.
// Test helpers use casts, indexing and arithmetic freely; this file never ships.
#![allow(
    dead_code,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::suboptimal_flops,
    clippy::needless_range_loop,
    clippy::missing_const_for_fn,
    clippy::print_stdout,
    clippy::too_many_arguments
)]

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use anim_core::Tick;
use anim_lipsync::{Viseme, VisemeKey, viseme_at};

pub use anim_testkit::synth::*;

pub fn t(seconds: f32) -> Tick {
    Tick::from_seconds_f64(f64::from(seconds))
}

pub fn secs(track: &[VisemeKey]) -> Vec<(f64, Viseme)> {
    track.iter().map(|k| (k.start.seconds(), k.viseme)).collect()
}

pub fn shown(track: &[VisemeKey]) -> Vec<Viseme> {
    track.iter().map(|k| k.viseme).collect()
}

/// The viseme showing for most of `[from, to]` seconds.
pub fn dominant(track: &[VisemeKey], from: f32, to: f32) -> Viseme {
    let mut counts = std::collections::BTreeMap::new();
    let steps = 100;
    for i in 0..steps {
        let s = from + (to - from) * (i as f32 + 0.5) / steps as f32;
        *counts.entry(viseme_at(track, t(s))).or_insert(0_u32) += 1;
    }
    counts.into_iter().max_by_key(|(_, c)| *c).unwrap().0
}

pub fn assert_sorted(track: &[VisemeKey]) {
    assert_eq!(track.first().map(|k| k.start), Some(Tick::ZERO));
    for w in track.windows(2) {
        assert!(w[0].start < w[1].start, "{track:?}");
        assert_ne!(w[0].viseme, w[1].viseme, "{track:?}");
    }
}

/// Start time (seconds) of the `index`-th key.
pub fn key_time(track: &[VisemeKey], index: usize) -> f64 {
    track[index].start.seconds()
}

pub fn assert_near(what: &str, got: f64, want: f64, lo: f64, hi: f64) {
    assert!(got >= want + lo && got <= want + hi, "{what}: {got:.4} s, expected {want:.4} + [{lo}, {hi}]");
}

// ---------------------------------------------------------------------------------------------------------------
// Images

pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

impl Image {
    pub fn new(w: usize, h: usize, fill: [u8; 3]) -> Self {
        Self { w, h, rgb: fill.iter().copied().cycle().take(w * h * 3).collect() }
    }

    pub fn put(&mut self, x: usize, y: usize, c: [u8; 3]) {
        if x < self.w && y < self.h {
            self.rgb[(y * self.w + x) * 3..(y * self.w + x) * 3 + 3].copy_from_slice(&c);
        }
    }

    pub fn get(&self, x: usize, y: usize) -> [u8; 3] {
        let at = (y * self.w + x) * 3;
        [self.rgb[at], self.rgb[at + 1], self.rgb[at + 2]]
    }

    pub fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, c: [u8; 3]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.put(xx, yy, c);
            }
        }
    }

    /// Crop `(x, y, w, h)` and scale it by `k` with nearest neighbour.
    pub fn crop_zoom(&self, x: usize, y: usize, w: usize, h: usize, k: usize) -> Self {
        let mut out = Self::new(w * k, h * k, [0, 0, 0]);
        for yy in 0..h * k {
            for xx in 0..w * k {
                let (sx, sy) = (x + xx / k, y + yy / k);
                if sx < self.w && sy < self.h {
                    out.put(xx, yy, self.get(sx, sy));
                }
            }
        }
        out
    }

    /// Writes the strip to `$ANICRAFT_VISUAL_OUT/anim-lipsync/<name>`; does nothing (returns `None`) when unset.
    pub fn save(&self, name: &str) -> Option<PathBuf> {
        let dir = PathBuf::from(std::env::var_os("ANICRAFT_VISUAL_OUT")?).join("anim-lipsync");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut enc = png::Encoder::new(BufWriter::new(File::create(&path).unwrap()), self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&self.rgb).unwrap();
        Some(path)
    }
}

/// One colour per viseme, in [`Viseme::ALL`] order; legible side by side.
pub const PALETTE: [[u8; 3]; 12] = [
    [235, 235, 235], // Neutral
    [220, 50, 47],   // Ah
    [38, 139, 210],  // D
    [133, 153, 0],   // Ee
    [181, 137, 0],   // F
    [108, 113, 196], // L
    [42, 161, 152],  // M
    [203, 75, 22],   // Oh
    [211, 54, 130],  // R
    [255, 220, 40],  // S
    [120, 70, 20],   // Uh
    [0, 43, 54],     // WOo
];

pub fn colour(v: Viseme) -> [u8; 3] {
    Viseme::ALL.iter().position(|a| *a == v).map_or([0, 0, 0], |i| PALETTE[i])
}

pub const PX_PER_SEC: usize = 600;

/// A strip: waveform envelope, then rows of viseme tracks (the first usually the prediction, then the observation),
/// with a grid every 0.1 s (darker each second) and a legend of the 12 colours at the bottom.
pub fn strip(samples: &[f32], rate: u32, rows: &[Vec<(f32, f32, Viseme)>]) -> Image {
    let secs = samples.len() as f32 / rate as f32;
    let width = (secs * PX_PER_SEC as f32).ceil() as usize + 2;
    let wave_h = 90;
    let row_h = 26;
    let height = wave_h + 6 + rows.len() * (row_h + 4) + 30;
    let mut img = Image::new(width, height, [255, 255, 255]);
    // Waveform envelope.
    let per_px = (rate as usize / PX_PER_SEC).max(1);
    for x in 0..width {
        let a = x * per_px;
        let chunk = samples.get(a..(a + per_px).min(samples.len())).unwrap_or(&[]);
        let (lo, hi) = chunk.iter().fold((0.0_f32, 0.0_f32), |(l, h), v| (l.min(*v), h.max(*v)));
        let mid = wave_h as f32 / 2.0;
        let y0 = (mid - hi.clamp(-1.0, 1.0) * mid * 3.0).clamp(0.0, wave_h as f32 - 1.0) as usize;
        let y1 = (mid - lo.clamp(-1.0, 1.0) * mid * 3.0).clamp(0.0, wave_h as f32 - 1.0) as usize;
        for y in y0.min(y1)..=y0.max(y1) {
            img.put(x, y, [60, 60, 60]);
        }
    }
    // Rows.
    for (r, row) in rows.iter().enumerate() {
        let y = wave_h + 6 + r * (row_h + 4);
        img.rect(0, y, width, row_h, [250, 250, 250]);
        for &(from, to, v) in row {
            let x0 = (from * PX_PER_SEC as f32).round().max(0.0) as usize;
            let x1 = ((to * PX_PER_SEC as f32).round() as usize).min(width);
            if x1 > x0 {
                img.rect(x0, y, x1 - x0, row_h, colour(v));
            }
        }
    }
    // Grid.
    let bottom = wave_h + 6 + rows.len() * (row_h + 4);
    for i in 0..=((secs * 10.0) as usize) {
        let x = i * PX_PER_SEC / 10;
        let c = if i % 10 == 0 { [0, 0, 0] } else { [190, 190, 190] };
        for y in 0..bottom {
            if i % 10 == 0 || y % 3 == 0 {
                img.put(x, y, c);
            }
        }
    }
    // Legend.
    for (i, c) in PALETTE.iter().enumerate() {
        img.rect(4 + i * 28, bottom + 4, 24, 18, *c);
    }
    img
}

/// A track as `(from, to, viseme)` segments up to `end` seconds.
pub fn segments(track: &[VisemeKey], end: f32) -> Vec<(f32, f32, Viseme)> {
    track
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let to = track.get(i + 1).map_or(end, |next| next.start.seconds() as f32);
            (k.start.seconds() as f32, to, k.viseme)
        })
        .collect()
}
