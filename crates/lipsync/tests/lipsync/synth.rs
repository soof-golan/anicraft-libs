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

use crate::common::{Rng, assert_sorted, band_noise, cat, dominant, n, silence, t, vowel, white_noise};

use anim_core::Tick;
use anim_lipsync::{LiveLipSync, Viseme, VisemeKey, analyze};

const RATE: u32 = 16_000;

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
            let x: Vec<f32> = (0..8000).map(|i| rng.next_f32() * 0.2 * ((i / 1000) % 2) as f32).collect();
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
