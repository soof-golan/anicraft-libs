//! Predict, render, inspect: synthetic audio fixtures with written predictions (`tests/fixtures/visual/*.expect.md`).
//! Each test renders a PNG strip (waveform, predicted visemes, observed visemes) under
//! `/mnt/data/anim/scratch/visual/anim-lipsync/` before it asserts, so a failure can be looked at.
// Test code: casts, indexing and arithmetic are fine here.
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
    clippy::missing_const_for_fn,
    clippy::print_stdout,
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::collection_is_never_read,
    clippy::needless_collect
)]

mod common;

use Viseme::{Ah, D, Ee, M, Neutral, Oh, S, Uh, WOo};
use anim_core::Tick;
use anim_lipsync::{Viseme, VisemeKey, analyze};
use common::*;

const AH: [f32; 3] = [800.0, 1300.0, 2600.0];
const EE: [f32; 3] = [300.0, 2250.0, 3000.0];
const OH: [f32; 3] = [540.0, 880.0, 2500.0];
const UH: [f32; 3] = [620.0, 1250.0, 2500.0];
const WOO: [f32; 3] = [320.0, 830.0, 2500.0];

/// Saves the strip: waveform, prediction (segments `(from, to, viseme)`), observation.
fn render(name: &str, samples: &[f32], rate: u32, predicted: &[(f32, f32, Viseme)], track: &[VisemeKey]) {
    let end = samples.len() as f32 / rate as f32;
    let img = strip(samples, rate, &[predicted.to_vec(), segments(track, end)]);
    let path = img.save(&format!("{name}.png"));
    println!("{name}: {:?} -> {}", secs(track), path.display());
}

/// A sibilant's first, partly filled window can read as a stop burst for 20 ms (indistinguishable from a click in
/// one frame); this removes such a D that is followed directly by S.
fn drop_leading_d(track: Vec<VisemeKey>) -> Vec<VisemeKey> {
    let mut out: Vec<VisemeKey> = Vec::new();
    for (i, k) in track.iter().enumerate() {
        let short_d_before_s = k.viseme == D && track.get(i + 1).is_some_and(|n| n.viseme == S && n.start.seconds() - k.start.seconds() < 0.0251);
        if !short_d_before_s {
            out.push(*k);
        }
    }
    out
}

fn word(parts: &[(Viseme, f32)], start: f32) -> Vec<(f32, f32, Viseme)> {
    let mut at = start;
    parts
        .iter()
        .map(|&(v, d)| {
            let seg = (at, at + d, v);
            at += d;
            seg
        })
        .collect()
}

#[test]
fn vowel_chain() {
    for rate in [16_000_u32, 44_100] {
        let parts: Vec<Vec<f32>> = [&AH, &EE, &OH, &WOO, &UH].iter().map(|f| vowel_raw(*f, 120.0, 120.0, 0.355, rate, 0.1)).collect();
        let chain = fade(crossfade_concat(&parts, 0.005, rate), rate);
        let x = cat(&[silence(0.3, rate), chain, silence(0.4, rate)]);
        let track = analyze(&x, rate);
        let predicted = word(&[(Neutral, 0.3), (Ah, 0.35), (Ee, 0.35), (Oh, 0.35), (WOo, 0.35), (Uh, 0.35), (Neutral, 0.4)], 0.0);
        render(&format!("vowel-chain-{rate}"), &x, rate, &predicted, &track);

        assert_sorted(&track);
        assert_eq!(shown(&track), [Neutral, Ah, Ee, Oh, WOo, Uh, Neutral], "{rate}: {:?}", secs(&track));
        assert_near("Ah", key_time(&track, 1), 0.30, -0.025, 0.04);
        for (i, b) in [(2, 0.65), (3, 1.0), (4, 1.35), (5, 1.7)] {
            assert_near(&format!("key {i}"), key_time(&track, i), b, -0.03, 0.06);
        }
        assert_near("final Neutral", key_time(&track, 6), 2.05, -0.03, 0.06);
        for (i, v) in [Ah, Ee, Oh, WOo, Uh].into_iter().enumerate() {
            assert_eq!(dominant(&track, 0.3 + 0.35 * i as f32 + 0.1, 0.3 + 0.35 * i as f32 + 0.25), v);
        }
    }
}

fn rate_phrase(rate: u32, with_s: bool) -> (Vec<f32>, Vec<(f32, f32, Viseme)>, f32) {
    let mut parts = vec![silence(0.3, rate)];
    let mut pred = vec![(Neutral, 0.3)];
    if with_s {
        parts.push(band_noise(6500.0, 2500.0, 0.2, rate, 0.08, 4));
        pred.push((S, 0.2));
    }
    parts.push(vowel(&EE, 120.0, 0.3, rate, 0.1));
    parts.push(vowel(&AH, 120.0, 0.3, rate, 0.1));
    parts.push(vowel(&WOO, 120.0, 0.3, rate, 0.1));
    parts.push(silence(0.4, rate));
    pred.extend([(Ee, 0.3), (Ah, 0.3), (WOo, 0.3), (Neutral, 0.4)]);
    let end = if with_s { 1.4 } else { 1.2 };
    (cat(&parts), word(&pred, 0.0), end)
}

#[test]
fn sample_rate_sweep() {
    let rates = [8_000_u32, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 96_000];
    let mut contact = Vec::new();
    let (xr, pr, _) = rate_phrase(16_000, true);
    let reference = analyze(&xr, 16_000);
    let ref_vowels = analyze(&rate_phrase(16_000, false).0, 16_000);
    for rate in rates {
        let with_s = rate >= 16_000;
        let (x, predicted, end) = rate_phrase(rate, with_s);
        let track = drop_leading_d(analyze(&x, rate));
        render(&format!("sample-rate-{rate}"), &x, rate, &predicted, &track);
        let want: Vec<Viseme> = if with_s { vec![Neutral, S, Ee, Ah, WOo, Neutral] } else { vec![Neutral, Ee, Ah, WOo, Neutral] };
        assert_eq!(shown(&track), want, "{rate} Hz: {:?}", secs(&track));
        let ref_track = if with_s { &reference } else { &ref_vowels };
        for (a, b) in track.iter().zip(ref_track) {
            assert!((a.start.seconds() - b.start.seconds()).abs() < 0.025, "{rate} Hz: {:?} vs {:?}", secs(&track), secs(ref_track));
        }
        let last = track.last().unwrap();
        assert_near("final Neutral", last.start.seconds(), f64::from(end), -0.03, 0.06);
        assert!(track.get(1).is_some_and(|k| k.start.seconds() > 0.27), "{rate} Hz invents sound in the silence");
        contact.push(strip(&x, rate, &[predicted, segments(&track, x.len() as f32 / rate as f32)]));
    }
    let _ = pr;
    // One contact image of the 8 strips stacked.
    let w = contact.iter().map(|i| i.w).max().unwrap();
    let h: usize = contact.iter().map(|i| i.h).sum();
    let mut all = Image::new(w, h, [255, 255, 255]);
    let mut y = 0;
    for img in &contact {
        for yy in 0..img.h {
            for xx in 0..img.w {
                all.put(xx, y + yy, img.get(xx, yy));
            }
        }
        y += img.h;
    }
    all.save("sample-rate-sweep.png");
}

#[test]
fn gain_polarity_fade() {
    let rate = 16_000;
    let ah = vowel(&AH, 120.0, 0.5, rate, 1.0);
    let at = |level: f32, sign: f32| -> Vec<f32> {
        let body: Vec<f32> = ah.iter().map(|v| v * level * sign).collect();
        cat(&[silence(0.3, rate), body, silence(0.4, rate)])
    };
    let pred_ah = word(&[(Neutral, 0.3), (Ah, 0.5), (Neutral, 0.4)], 0.0);
    let ah_rms = rms(&ah);
    // The fixture's vowel has rms `ah_rms`; scale to the requested rms.
    let lv = |target: f32| target / ah_rms;
    let mut rows = Vec::new();
    for (name, level, sign) in [("m40", 0.01, 1.0), ("m20", 0.1, 1.0), ("m02", 0.8, 1.0), ("inv", 0.1, -1.0)] {
        let x = at(lv(level), sign);
        let track = analyze(&x, rate);
        render(&format!("gain-{name}"), &x, rate, &pred_ah, &track);
        assert_eq!(shown(&track), [Neutral, Ah, Neutral], "{name}: {:?}", secs(&track));
        assert_near(name, key_time(&track, 1), 0.3, -0.025, 0.04);
        rows.push(track);
    }
    let x = at(lv(0.002), 1.0);
    let track = analyze(&x, rate);
    render("gain-m54", &x, rate, &[(0.0, 1.2, Neutral)], &track);
    assert_eq!(shown(&track), [Neutral], "below the silence floor: {:?}", secs(&track));

    // Fade-in: rms 0.0005 * 400^(t/2) for 2 s.
    let len = n(2.0, rate);
    let base = vowel_raw(&AH, 120.0, 120.0, 2.0, rate, 1.0);
    let ramp: Vec<f32> = (0..len).map(|i| base[i] * 0.0005 * 400.0_f32.powf(i as f32 / rate as f32 / 2.0)).collect();
    let x = cat(&[ramp, silence(0.4, rate)]);
    let track = analyze(&x, rate);
    let pred = word(&[(Neutral, 0.537), (Ah, 1.463), (Neutral, 0.4)], 0.0);
    render("gain-fade-in", &x, rate, &pred, &track);
    let first = track.iter().find(|k| k.viseme != Neutral).expect("some viseme");
    assert_eq!(first.viseme, Ah, "{:?}", secs(&track));
    assert_near("fade-in Ah", first.start.seconds(), 0.5, -0.05, 0.3);
    assert!(track.iter().all(|k| matches!(k.viseme, Neutral | Ah)), "{:?}", secs(&track));
    assert_eq!(track.iter().filter(|k| k.viseme == Ah).count(), 1, "{:?}", secs(&track));
}

fn room_tone(seconds: f32, rate: u32, seed: u32) -> Vec<f32> {
    let hum_wave: Vec<f32> = (0..n(seconds, rate))
        .map(|i| {
            let s = i as f32 / rate as f32;
            (1..=3).map(|h| (std::f32::consts::TAU * 50.0 * h as f32 * s).sin() / h as f32).sum::<f32>()
        })
        .collect();
    let hum_wave = scale_to_rms(hum_wave, 0.01);
    add(&hum_wave, &white_noise_raw(seconds, rate, 0.004, seed))
}

#[test]
fn ambient_start() {
    let rate = 16_000;
    let tone = room_tone(6.0, rate, 21);
    let track = analyze(&tone, rate);
    render("ambient-tone-only", &tone, rate, &[(0.0, 6.0, Neutral)], &track);

    let speech = cat(&[silence(3.0, rate), vowel(&AH, 120.0, 0.6, rate, 0.1), silence(1.0, rate)]);
    let x = add(&speech, &room_tone(4.6, rate, 22));
    let track2 = analyze(&x, rate);
    render("ambient-speech", &x, rate, &word(&[(Neutral, 3.0), (Ah, 0.6), (Neutral, 1.0)], 0.0), &track2);

    assert_eq!(shown(&track), [Neutral], "room tone alone must be silence: {:?}", secs(&track));
    // Known limitation (see the .expect.md): the noise-floor estimate starts at -60 dBFS and rises 2 dB/s, so a
    // room tone at -40 dBFS can flash one viseme during the first second. After that it is silence.
    let before: Vec<_> = track2.iter().filter(|k| k.viseme != Neutral && k.start > t(1.0) && k.start < t(2.97)).collect();
    assert!(before.is_empty(), "room tone read as speech: {:?}", secs(&track2));
    assert!(track2.iter().filter(|k| k.viseme != Neutral && k.start < t(1.0)).count() <= 1, "{:?}", secs(&track2));
    let ah = track2.iter().find(|k| k.viseme == Ah && k.start > t(2.0)).expect("the vowel is found");
    assert_near("Ah", ah.start.seconds(), 3.0, -0.03, 0.1);
    assert_eq!(dominant(&track2, 3.2, 3.5), Ah);
    assert_eq!(dominant(&track2, 3.8, 4.6), Neutral);
}

#[test]
fn sas_mama() {
    let rate = 16_000;
    let ah = || vowel(&AH, 120.0, 0.3, rate, 0.1);
    let sas = [band_noise(6500.0, 2500.0, 0.22, rate, 0.08, 7), ah(), band_noise(6500.0, 2500.0, 0.22, rate, 0.08, 8)];
    let mama = [hum(140.0, 0.25, rate, 0.05), ah(), hum(140.0, 0.25, rate, 0.05)];
    // Cross-fade the joins by 3 ms so no click separates the sounds.
    let w1 = crossfade_concat(&sas, 0.003, rate);
    let w2 = crossfade_concat(&mama, 0.003, rate);
    let x = cat(&[silence(0.3, rate), w1, silence(0.5, rate), w2, silence(0.4, rate)]);
    let track = analyze(&x, rate);
    let pred = word(&[(Neutral, 0.3), (S, 0.22), (Ah, 0.3), (S, 0.22), (Neutral, 0.5), (M, 0.25), (Ah, 0.3), (M, 0.25), (Neutral, 0.4)], 0.0);
    render("sas-mama", &x, rate, &pred, &track);
    assert_sorted(&track);
    assert_eq!(shown(&track), [Neutral, S, Ah, S, Neutral, M, Ah, M, Neutral], "{:?}", secs(&track));
    let bounds = [0.3, 0.52, 0.82, 1.04, 1.54, 1.79, 2.09, 2.34];
    for (i, b) in bounds.iter().enumerate() {
        assert_near(&format!("key {}", i + 1), key_time(&track, i + 1), *b, -0.03, 0.06);
    }
}

#[test]
fn click_train() {
    let rate = 16_000;
    let mut x = silence(1.1, rate);
    for (i, at) in [0.30_f32, 0.45, 0.60].iter().enumerate() {
        let burst = white_noise_raw(0.015, rate, 0.2, 30 + i as u32);
        let s = n(*at, rate);
        x[s..s + burst.len()].copy_from_slice(&burst);
    }
    let track = analyze(&x, rate);
    let mut pred = vec![(0.0, 0.3, Neutral)];
    for at in [0.30_f32, 0.45, 0.60] {
        pred.push((at, at + 0.015, D));
        pred.push((at + 0.015, at + 0.15, Neutral));
    }
    render("click-train", &x, rate, &pred, &track);
    // The 25 ms window smears a 15 ms burst over about 40 ms, so a soft-noise F may follow the D (see .expect.md).
    assert!(track.iter().all(|k| matches!(k.viseme, Neutral | D | Viseme::F)), "{:?}", secs(&track));
    let ds: Vec<&VisemeKey> = track.iter().filter(|k| k.viseme == D).collect();
    assert_eq!(ds.len(), 3, "{:?}", secs(&track));
    for (d, at) in ds.iter().zip([0.30, 0.45, 0.60]) {
        assert_near("D", d.start.seconds(), at, -0.02, 0.05);
        let next = track.iter().find(|k| k.viseme == Neutral && k.start > d.start).expect("back to Neutral");
        let len = next.start.seconds() - d.start.seconds();
        assert!(len <= 0.1, "burst shows for {len} s");
    }
    assert_eq!(dominant(&track, 0.39, 0.41), Neutral);
    assert_eq!(dominant(&track, 0.54, 0.56), Neutral);
}

#[test]
fn alternating_vowels() {
    let rate = 16_000;
    let alt = |seg: f32, count: usize| -> Vec<f32> {
        let parts: Vec<Vec<f32>> = (0..count).map(|i| vowel_raw(if i % 2 == 0 { &AH } else { &EE }, 120.0, 120.0, seg + 0.005, rate, 0.1)).collect();
        fade(crossfade_concat(&parts, 0.005, rate), rate)
    };
    // 100 ms segments.
    let x = cat(&[silence(0.3, rate), alt(0.1, 8), silence(0.4, rate)]);
    let track = analyze(&x, rate);
    let pred: Vec<_> = (0..8).map(|i| (0.3 + 0.1 * i as f32, 0.4 + 0.1 * i as f32, if i % 2 == 0 { Ah } else { Ee })).collect();
    render("alternating-100", &x, rate, &pred, &track);
    assert_sorted(&track);
    let right = (0..8)
        .filter(|i| {
            let want = if i % 2 == 0 { Ah } else { Ee };
            anim_lipsync::viseme_at(&track, t(0.3 + 0.1 * *i as f32 + 0.08)) == want
        })
        .count();
    assert!(right >= 7, "{right}/8 segments right: {:?}", secs(&track));
    assert!(track.iter().all(|k| matches!(k.viseme, Ah | Ee | Neutral)), "{:?}", secs(&track));
    check_min_gap(&track);

    // 60 ms segments.
    let x = cat(&[silence(0.3, rate), alt(0.06, 8), silence(0.4, rate)]);
    let track = analyze(&x, rate);
    let pred: Vec<_> = (0..8).map(|i| (0.3 + 0.06 * i as f32, 0.36 + 0.06 * i as f32, if i % 2 == 0 { Ah } else { Ee })).collect();
    render("alternating-60", &x, rate, &pred, &track);
    assert_sorted(&track);
    check_min_gap(&track);

    // Three isolated 80 ms syllables.
    let syl = vowel(&AH, 120.0, 0.08, rate, 0.1);
    let x = cat(&[silence(0.2, rate), syl.clone(), silence(0.15, rate), syl.clone(), silence(0.15, rate), syl, silence(0.4, rate)]);
    let track = analyze(&x, rate);
    let pred = word(&[(Neutral, 0.2), (Ah, 0.08), (Neutral, 0.15), (Ah, 0.08), (Neutral, 0.15), (Ah, 0.08), (Neutral, 0.4)], 0.0);
    render("short-syllables", &x, rate, &pred, &track);
    assert_eq!(shown(&track), [Neutral, Ah, Neutral, Ah, Neutral, Ah, Neutral], "{:?}", secs(&track));
    for (i, at) in [0.2, 0.43, 0.66].iter().enumerate() {
        assert_near("syllable", key_time(&track, 1 + 2 * i), *at, -0.025, 0.04);
    }
}

fn check_min_gap(track: &[VisemeKey]) {
    for w in track.windows(2) {
        if w[0].viseme != D {
            let gap = w[1].start.seconds() - w[0].start.seconds();
            assert!(gap >= 0.0399, "{gap}: {:?}", secs(track));
        }
    }
}

#[test]
fn high_pitch_voice() {
    let rate = 16_000;
    let mut report = Vec::new();
    for f0 in [100.0_f32, 150.0, 200.0, 250.0, 300.0, 350.0] {
        let x = cat(&[silence(0.3, rate), vowel(&AH, f0, 0.5, rate, 0.1), silence(0.1, rate), vowel(&EE, f0, 0.5, rate, 0.1), silence(0.3, rate)]);
        let track = analyze(&x, rate);
        let pred = word(&[(Neutral, 0.3), (Ah, 0.5), (Neutral, 0.1), (Ee, 0.5), (Neutral, 0.3)], 0.0);
        render(&format!("pitch-{f0}"), &x, rate, &pred, &track);
        let (a, e) = (dominant(&track, 0.45, 0.7), dominant(&track, 1.0, 1.3));
        report.push((f0, a, e));
        if f0 <= 250.0 {
            assert_eq!((a, e), (Ah, Ee), "f0 {f0}: {:?}", secs(&track));
        }
    }
    println!("pitch report (f0, Ah->, Ee->): {report:?}");
    let x = cat(&[silence(0.3, rate), vowel_raw(&AH, 90.0, 320.0, 0.8, rate, 0.1), silence(0.4, rate)]);
    let x = fade(x, rate);
    let track = analyze(&x, rate);
    render("pitch-glide", &x, rate, &word(&[(Neutral, 0.3), (Ah, 0.8), (Neutral, 0.4)], 0.0), &track);
    assert_eq!(shown(&track), [Neutral, Ah, Neutral], "{:?}", secs(&track));
}

#[test]
fn track_ticks_are_exact_sample_times() {
    // Keys are `Tick::from_units(samples, rate)` of whole analysis hops: at 48 kHz every key is a multiple of 1/48000 s.
    let rate = 48_000;
    let x = cat(&[silence(0.3, rate), vowel(&AH, 120.0, 0.5, rate, 0.1), silence(0.4, rate)]);
    for k in analyze(&x, rate) {
        let samples = k.start.0 * 48_000 % anim_core::TICKS_PER_SECOND;
        assert_eq!(samples, 0, "{:?} is not a whole sample at 48 kHz", k.start);
    }
    assert_eq!(Tick::ZERO, analyze(&x, rate)[0].start);
}
