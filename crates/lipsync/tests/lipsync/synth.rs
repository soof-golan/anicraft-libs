//! Tests with synthesised signals: formant vowels, noise bursts, decaying tones, silence.
// Test helpers (signal synthesis) use casts, indexing and arithmetic freely; this file never ships.
#![allow(
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
    clippy::missing_const_for_fn
)]

use anim_core::Tick;
use anim_lipsync::{LiveLipSync, Viseme, VisemeKey, analyze, viseme_at};

const RATE: u32 = 16_000;

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn n(seconds: f32, rate: u32) -> usize {
    (seconds * rate as f32).round() as usize
}

fn silence(seconds: f32, rate: u32) -> Vec<f32> {
    vec![0.0; n(seconds, rate)]
}

fn scale_to_rms(mut x: Vec<f32>, rms: f32) -> Vec<f32> {
    let cur = (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt().max(1e-9);
    for v in &mut x {
        *v *= rms / cur;
    }
    x
}

/// Two-pole resonator in place.
fn resonate(x: &mut [f32], rate: u32, hz: f32, bw: f32) {
    let r = (-std::f32::consts::PI * bw / rate as f32).exp();
    let c1 = 2.0 * r * (std::f32::consts::TAU * hz / rate as f32).cos();
    let c2 = -r * r;
    let (mut y1, mut y2) = (0.0, 0.0);
    for v in x.iter_mut() {
        let y = *v + c1 * y1 + c2 * y2;
        y2 = y1;
        y1 = y;
        *v = y;
    }
}

/// A glottal pulse train through a formant filter cascade, with a slight vibrato-free steady pitch.
fn vowel(formants: &[f32], f0: f32, seconds: f32, rate: u32, rms: f32) -> Vec<f32> {
    let len = n(seconds, rate);
    let period = rate as f32 / f0;
    let mut src = vec![0.0; len];
    let mut next = 0.0_f32;
    for (i, v) in src.iter_mut().enumerate() {
        if i as f32 >= next {
            *v = 1.0;
            next += period;
        }
    }
    // Glottal spectral tilt.
    let mut y = 0.0;
    for v in &mut src {
        y = 0.5 * y + *v;
        *v = y;
    }
    for &f in formants {
        resonate(&mut src, rate, f, 70.0 + f * 0.04);
    }
    fade(scale_to_rms(src, rms), rate)
}

fn fade(mut x: Vec<f32>, rate: u32) -> Vec<f32> {
    let k = n(0.01, rate).min(x.len() / 2);
    for i in 0..k {
        let g = i as f32 / k as f32;
        x[i] *= g;
        let last = x.len() - 1 - i;
        x[last] *= g;
    }
    x
}

/// Noise concentrated around `centre` Hz (S: ~6.5 kHz).
fn band_noise(centre: f32, bw: f32, seconds: f32, rate: u32, rms: f32, seed: u32) -> Vec<f32> {
    let mut rng = Rng(seed);
    let mut x: Vec<f32> = (0..n(seconds, rate)).map(|_| rng.next()).collect();
    resonate(&mut x, rate, centre, bw);
    resonate(&mut x, rate, centre, bw);
    fade(scale_to_rms(x, rms), rate)
}

fn white_noise(seconds: f32, rate: u32, rms: f32, seed: u32) -> Vec<f32> {
    let mut rng = Rng(seed);
    fade(scale_to_rms((0..n(seconds, rate)).map(|_| rng.next()).collect(), rms), rate)
}

fn cat(parts: &[Vec<f32>]) -> Vec<f32> {
    parts.concat()
}

fn t(seconds: f32) -> Tick {
    Tick::from_seconds_f64(f64::from(seconds))
}

/// The viseme that is showing for most of `[from, to]` seconds.
fn dominant(track: &[VisemeKey], from: f32, to: f32) -> Viseme {
    let mut counts = std::collections::HashMap::new();
    let steps = 100;
    for i in 0..steps {
        let s = from + (to - from) * (i as f32 + 0.5) / steps as f32;
        *counts.entry(viseme_at(track, t(s))).or_insert(0_u32) += 1;
    }
    counts.into_iter().max_by_key(|(_, c)| *c).unwrap().0
}

fn assert_sorted(track: &[VisemeKey]) {
    assert_eq!(track.first().map(|k| k.start), Some(Tick::ZERO));
    for w in track.windows(2) {
        assert!(w[0].start < w[1].start, "{track:?}");
        assert_ne!(w[0].viseme, w[1].viseme, "{track:?}");
    }
}

fn single_sound(sound: Vec<f32>, rate: u32) -> Vec<VisemeKey> {
    let x = cat(&[silence(0.3, rate), sound, silence(0.4, rate)]);
    analyze(&x, rate)
}

fn check_vowel(formants: &[f32], expected: Viseme, rate: u32) {
    let track = single_sound(vowel(formants, 120.0, 0.5, rate, 0.1), rate);
    assert_sorted(&track);
    assert_eq!(dominant(&track, 0.45, 0.7), expected, "{expected:?} {track:?}");
    assert_eq!(dominant(&track, 0.0, 0.25), Viseme::Neutral);
    assert_eq!(dominant(&track, 1.0, 1.2), Viseme::Neutral);
    assert_eq!(track.last().map(|k| k.viseme), Some(Viseme::Neutral));
}

#[test]
fn silence_is_one_neutral_key() {
    let track = analyze(&silence(2.0, RATE), RATE);
    assert_eq!(track, vec![VisemeKey { start: Tick::ZERO, viseme: Viseme::Neutral }]);
    // Very quiet noise stays Neutral too.
    let hiss = white_noise(2.0, RATE, 0.0005, 7);
    assert_eq!(analyze(&hiss, RATE).len(), 1);
}

#[test]
fn ah() {
    check_vowel(&[800.0, 1300.0, 2600.0], Viseme::Ah, RATE);
}

#[test]
fn ee() {
    check_vowel(&[300.0, 2250.0, 3000.0], Viseme::Ee, RATE);
}

#[test]
fn oh() {
    check_vowel(&[540.0, 880.0, 2500.0], Viseme::Oh, RATE);
}

#[test]
fn uh() {
    check_vowel(&[620.0, 1250.0, 2500.0], Viseme::Uh, RATE);
}

#[test]
fn woo() {
    check_vowel(&[320.0, 830.0, 2500.0], Viseme::WOo, RATE);
}

#[test]
fn vowels_hold_across_sample_rates() {
    for rate in [22_050, 44_100, 48_000] {
        check_vowel(&[800.0, 1300.0, 2600.0], Viseme::Ah, rate);
        check_vowel(&[300.0, 2250.0, 3000.0], Viseme::Ee, rate);
    }
}

#[test]
fn vowels_are_pitch_independent() {
    for f0 in [100.0, 160.0, 220.0] {
        let track = single_sound(vowel(&[800.0, 1300.0, 2600.0], f0, 0.5, RATE, 0.1), RATE);
        assert_eq!(dominant(&track, 0.45, 0.7), Viseme::Ah, "f0 {f0}: {track:?}");
    }
}

#[test]
fn sibilant_noise_is_s() {
    let track = single_sound(band_noise(6500.0, 2500.0, 0.25, RATE, 0.08, 1), RATE);
    assert_sorted(&track);
    assert_eq!(dominant(&track, 0.4, 0.5), Viseme::S, "{track:?}");
    assert_eq!(dominant(&track, 0.9, 1.0), Viseme::Neutral);
}

#[test]
fn flat_noise_is_f() {
    let track = single_sound(white_noise(0.3, RATE, 0.03, 3), RATE);
    assert_eq!(dominant(&track, 0.45, 0.55), Viseme::F, "{track:?}");
}

#[test]
fn short_broadband_burst_is_d() {
    let track = single_sound(white_noise(0.02, RATE, 0.1, 5), RATE);
    assert!(track.iter().any(|k| k.viseme == Viseme::D && k.start >= t(0.25) && k.start < t(0.4)), "{track:?}");
    assert_eq!(track.last().map(|k| k.viseme), Some(Viseme::Neutral));
}

#[test]
fn low_voiced_murmur_is_m() {
    // A decaying 150 Hz tone: a closed-mouth hum that dies away.
    let len = n(0.7, RATE);
    let tone: Vec<f32> = (0..len)
        .map(|i| {
            let s = i as f32 / RATE as f32;
            (std::f32::consts::TAU * 150.0 * s).sin() * 0.1 * (-s / 0.35).exp()
        })
        .collect();
    let track = single_sound(tone, RATE);
    assert_sorted(&track);
    assert_eq!(dominant(&track, 0.4, 0.6), Viseme::M, "{track:?}");
    assert_eq!(track.last().map(|k| k.viseme), Some(Viseme::Neutral));
}

#[test]
fn r_and_l_after_a_loud_vowel() {
    let ah = vowel(&[800.0, 1300.0, 2600.0], 120.0, 0.3, RATE, 0.1);
    let r = vowel(&[420.0, 1250.0, 1700.0], 120.0, 0.3, RATE, 0.045);
    let l = vowel(&[370.0, 1150.0, 2900.0], 120.0, 0.3, RATE, 0.04);
    let x = cat(&[silence(0.3, RATE), ah.clone(), r, ah.clone(), l, ah, silence(0.4, RATE)]);
    let track = analyze(&x, RATE);
    assert_sorted(&track);
    assert_eq!(dominant(&track, 0.7, 0.85), Viseme::R, "{track:?}");
    assert_eq!(dominant(&track, 1.3, 1.45), Viseme::L, "{track:?}");
    assert_eq!(dominant(&track, 1.0, 1.1), Viseme::Ah);
}

#[test]
fn phrase_comes_out_in_order() {
    let x = cat(&[
        silence(0.3, RATE),
        band_noise(6500.0, 2500.0, 0.2, RATE, 0.08, 11),
        vowel(&[300.0, 2250.0, 3000.0], 130.0, 0.3, RATE, 0.1),
        vowel(&[800.0, 1300.0, 2600.0], 130.0, 0.3, RATE, 0.1),
        vowel(&[320.0, 830.0, 2500.0], 130.0, 0.3, RATE, 0.1),
        silence(0.4, RATE),
    ]);
    let track = analyze(&x, RATE);
    let shown: Vec<Viseme> = track.iter().map(|k| k.viseme).collect();
    let pos = |v: Viseme| shown.iter().position(|s| *s == v);
    let (s, ee, ah, woo) = (pos(Viseme::S).unwrap(), pos(Viseme::Ee).unwrap(), pos(Viseme::Ah).unwrap(), pos(Viseme::WOo).unwrap());
    assert!(s < ee && ee < ah && ah < woo, "{shown:?}");
    // Onsets are timed to within ~60 ms.
    let start_of = |v: Viseme| track.iter().find(|k| k.viseme == v).unwrap().start;
    assert!((start_of(Viseme::S).seconds() - 0.3).abs() < 0.06, "{track:?}");
    assert!((start_of(Viseme::Ah).seconds() - 0.8).abs() < 0.08, "{track:?}");
}

#[test]
fn chunking_does_not_change_the_result() {
    let x = cat(&[
        silence(0.2, RATE),
        vowel(&[800.0, 1300.0, 2600.0], 120.0, 0.3, RATE, 0.1),
        band_noise(6500.0, 2500.0, 0.2, RATE, 0.08, 2),
        vowel(&[300.0, 2250.0, 3000.0], 120.0, 0.3, RATE, 0.1),
        silence(0.3, RATE),
    ]);
    let whole = analyze(&x, RATE);
    assert_eq!(whole, analyze(&x, RATE), "deterministic");
    for chunk in [1, 7, 160, 333, 4096] {
        let mut live = LiveLipSync::new(RATE);
        let mut keys = Vec::new();
        for c in x.chunks(chunk) {
            keys.extend(live.push_keys(c));
        }
        keys.extend(live.finish());
        let track: Vec<_> = keys.iter().copied().filter(|k| k.start > Tick::ZERO).collect();
        let reference: Vec<_> = whole.iter().copied().filter(|k| k.start > Tick::ZERO).collect();
        assert_eq!(track, reference, "chunk {chunk}");
    }
}

#[test]
fn live_latency_is_about_fifty_milliseconds() {
    let x = cat(&[silence(0.5, RATE), vowel(&[800.0, 1300.0, 2600.0], 120.0, 1.0, RATE, 0.1)]);
    let mut live = LiveLipSync::new(RATE);
    assert!(live.latency() < t(0.06), "{:?}", live.latency());
    let hop = 160;
    let mut reported_at = None;
    for (i, c) in x.chunks(hop).enumerate() {
        if live.push(c) == Some(Viseme::Ah) {
            reported_at = Some(((i + 1) * hop) as f32 / RATE as f32);
            break;
        }
    }
    let delay = reported_at.expect("Ah reported") - 0.5;
    assert!(delay <= 0.08, "delay {delay}");
    assert_eq!(live.current(), Viseme::Ah);
    live.reset();
    assert_eq!(live.current(), Viseme::Neutral);
}

#[test]
fn hostile_input_is_harmless() {
    let mut x = vec![f32::NAN; 1000];
    x.extend(vec![f32::INFINITY; 1000]);
    x.extend(vec![f32::NEG_INFINITY; 1000]);
    x.extend(vec![1e30; 1000]);
    for rate in [0, 1, 100, 4000, 8000, 384_000, u32::MAX] {
        let track = analyze(&x, rate);
        assert_sorted(&track);
    }
}

mod fuzz {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        #[test]
        fn random_audio_never_panics(
            samples in proptest::collection::vec(-2.0_f32..2.0, 0..6000),
            rate in 1_u32..200_000,
        ) {
            assert_sorted(&analyze(&samples, rate));
        }

        #[test]
        fn chunked_live_equals_offline(
            seed in 1_u32..u32::MAX,
            chunk in 1_usize..2000,
        ) {
            let mut rng = Rng(seed);
            let x: Vec<f32> = (0..8000).map(|i| rng.next() * 0.2 * ((i / 1000) % 2) as f32).collect();
            let whole = analyze(&x, RATE);
            let mut live = LiveLipSync::new(RATE);
            let mut keys: Vec<VisemeKey> = x.chunks(chunk).flat_map(|c| live.push_keys(c)).collect();
            keys.extend(live.finish());
            let tail: Vec<_> = keys.into_iter().filter(|k| k.start > Tick::ZERO).collect();
            let want: Vec<_> = whole.into_iter().filter(|k| k.start > Tick::ZERO).collect();
            prop_assert_eq!(tail, want);
        }
    }
}
