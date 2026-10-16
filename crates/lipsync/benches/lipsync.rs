//! Lip-sync analysis benchmarks on synthetic audio: a minute of alternating formant-like vowels, noise bursts and
//! silence at 16 kHz and 48 kHz. With `BENCH_CHECKSUM=1` the resulting viseme track is hashed and printed, so that an
//! optimisation can be checked to leave the output unchanged.
#![allow(
    clippy::unwrap_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    reason = "benchmark code over synthetic audio"
)]

use std::hash::{DefaultHasher, Hash, Hasher};
use std::hint::black_box;

use anim_lipsync::analyze;
use criterion::{Criterion, criterion_group, criterion_main};

/// Deterministic noise in -1..1.
fn noise(i: usize) -> f32 {
    let x = (i as u32).wrapping_mul(2_654_435_761).rotate_left(13).wrapping_mul(40_503);
    (x >> 8) as f32 / (1_u32 << 23) as f32 - 1.0
}

/// `seconds` of speech-like audio: 150 Hz pulses shaped by two formants per 0.4 s segment, then a noise burst, then
/// silence.
fn speech(rate: u32, seconds: u32) -> Vec<f32> {
    const VOWELS: [(f32, f32); 5] = [(730.0, 1090.0), (270.0, 2290.0), (570.0, 840.0), (300.0, 870.0), (530.0, 1840.0)];
    let n = (rate * seconds) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let segment = (t / 0.4) as usize;
            let local = t - segment as f32 * 0.4;
            match segment % 3 {
                0 | 1 => {
                    let (f1, f2) = VOWELS[(segment / 3) % VOWELS.len()];
                    let tau = std::f32::consts::TAU;
                    0.3 * (tau * 150.0 * t).sin() + 0.2 * (tau * f1 * t).sin() + 0.1 * (tau * f2 * t).sin()
                }
                _ if local < 0.1 => 0.2 * noise(i),
                _ => 0.0,
            }
        })
        .collect()
}

fn checksum(keys: &[anim_lipsync::VisemeKey]) -> u64 {
    let mut h = DefaultHasher::new();
    for k in keys {
        k.hash(&mut h);
    }
    h.finish()
}

fn bench(c: &mut Criterion) {
    for rate in [16_000_u32, 48_000] {
        let audio = speech(rate, 60);
        if std::env::var_os("BENCH_CHECKSUM").is_some() {
            let keys = analyze(&audio, rate);
            println!("analyze {rate}: {} keys, checksum {:016x}", keys.len(), checksum(&keys));
        }
        c.bench_function(&format!("analyze_60s_{rate}"), |b| b.iter(|| black_box(analyze(black_box(&audio), rate))));
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).warm_up_time(std::time::Duration::from_secs(1));
    targets = bench
}
criterion_main!(benches);
