//! Video encoding benchmarks on synthetic stress inputs: moving gradient-and-noise frames (hard for the codecs),
//! alpha frames for `ProRes`, and a long stereo soundtrack for AAC and PCM.
//!
//! The crate has no document parsers, so there are no sample-file benchmarks here. With `BENCH_FIXED=<runs>
//! BENCH_ONLY=<case>` only that case runs, a fixed number of times (for `perf record`). With `BENCH_CHECKSUM=1` each
//! case prints a checksum of the file it wrote, so that an optimisation can be checked to leave the bytes unchanged.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::suboptimal_flops,
    clippy::missing_const_for_fn,
    reason = "benchmark code over synthetic frames"
)]

use std::hint::black_box;

use anim_core::FrameRate;
use anim_display::Image;
use anim_video::{AudioTrack, Codec, H264Rate, VideoSettings, encode_to_file};
use criterion::{Criterion, criterion_group, criterion_main};

/// Deterministic noise in 0..1.
fn noise(i: u32) -> f64 {
    let x = i.wrapping_mul(2_654_435_761).rotate_left(13).wrapping_mul(40_503);
    f64::from(x >> 8) / f64::from(1_u32 << 24)
}

/// A premultiplied frame: a moving gradient with a little noise and a soft-edged disc, alpha 255 outside the disc
/// when `alpha` is false and a vignette when it is true.
fn frame(width: u32, height: u32, step: u32, alpha: bool) -> Image {
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        for col in 0..width {
            let dx = f64::from(col) - f64::from(width) / 2.0 - f64::from(step) * 3.0;
            let dy = f64::from(row) - f64::from(height) / 2.0;
            let coverage = if alpha { 0.8f64.mul_add(-dx.hypot(dy), 255.0).clamp(0.0, 255.0) } else { 255.0 };
            let grain = noise(row * width + col + step * 7919) * 24.0;
            let channel = |v: f64| ((v + grain) * coverage / 255.0).clamp(0.0, coverage) as u8;
            bytes.extend_from_slice(&[
                channel(f64::from(col * 255 / width)),
                channel(f64::from(row * 255 / height)),
                channel(f64::from((col + row + step * 4) % 256)),
                coverage as u8,
            ]);
        }
    }
    Image::new(width, height, bytes).unwrap()
}

fn tone(seconds: u32, rate: u32) -> AudioTrack {
    let samples = (0..seconds * rate).flat_map(|i| {
        let t = f64::from(i) / f64::from(rate);
        [(0.4 * (t * 440.0 * std::f64::consts::TAU).sin()) as f32, (0.3 * (t * 660.0 * std::f64::consts::TAU).sin()) as f32]
    });
    AudioTrack { samples: samples.collect(), sample_rate: rate, channels: 2 }
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3))
}

/// Encodes `frames` to a temporary file and returns its bytes.
fn encode(settings: &VideoSettings, frames: &[Image]) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("out.{}", settings.codec.extension()));
    encode_to_file(&path, settings.clone(), frames.iter().cloned()).unwrap();
    std::fs::read(path).unwrap()
}

type Runner = Box<dyn Fn() -> Vec<u8>>;

struct Case {
    name: &'static str,
    build: fn() -> Runner,
}

fn clip(width: u32, height: u32, count: u32, alpha: bool, settings: VideoSettings) -> Runner {
    let frames: Vec<Image> = (0..count).map(|t| frame(width, height, t, alpha)).collect();
    Box::new(move || encode(&settings, &frames))
}

const H264_30: FrameRate = FrameRate::FPS_30;

const CASES: [Case; 5] = [
    Case {
        name: "h264_crf_640x360_30_frames",
        build: || clip(640, 360, 30, false, VideoSettings::new(640, 360, H264_30, Codec::H264 { rate: H264Rate::Quality { crf: 20.0 } })),
    },
    Case {
        name: "h264_bitrate_1280x720_10_frames",
        build: || clip(1280, 720, 10, false, VideoSettings::new(1280, 720, H264_30, Codec::H264 { rate: H264Rate::Bitrate { kbps: 4000 } })),
    },
    Case {
        name: "h264_aac_640x360_30_frames_10s_stereo",
        build: || {
            let settings = VideoSettings::new(640, 360, H264_30, Codec::H264 { rate: H264Rate::Quality { crf: 20.0 } }).with_audio(tone(10, 48_000));
            clip(640, 360, 30, false, settings)
        },
    },
    Case {
        name: "prores4444_alpha_640x360_20_frames",
        build: || clip(640, 360, 20, true, VideoSettings::new(640, 360, H264_30, Codec::ProRes4444 { alpha: true })),
    },
    Case {
        name: "prores4444_pcm_640x360_30_frames_10s_stereo",
        build: || {
            let settings = VideoSettings::new(640, 360, H264_30, Codec::ProRes4444 { alpha: false }).with_audio(tone(10, 48_000));
            clip(640, 360, 30, false, settings)
        },
    },
];

fn video_benches(c: &mut Criterion) {
    let fixed = std::env::var("BENCH_FIXED").ok().and_then(|v| v.parse::<u32>().ok());
    let only = std::env::var("BENCH_ONLY").ok();
    let mut group = c.benchmark_group("video");
    group.sample_size(10);
    for case in &CASES {
        if fixed.is_some() && !only.as_deref().is_some_and(|only| case.name.contains(only)) {
            continue;
        }
        let run = (case.build)();
        if std::env::var_os("BENCH_CHECKSUM").is_some() {
            eprintln!("checksum {} {:016x}", case.name, hash(&run()));
        }
        if let Some(count) = fixed {
            for _ in 0..count {
                black_box(run());
            }
            continue;
        }
        group.bench_function(case.name, |b| b.iter(|| black_box(run())));
    }
    group.finish();
}

criterion_group!(benches, video_benches);
criterion_main!(benches);
