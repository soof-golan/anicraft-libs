//! Decodes the real sample files when `ANICRAFT_SAMPLES` is set; passes immediately otherwise.

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]

use std::path::PathBuf;

use anim_archive::Container;
use anim_audio::{MixFormat, MixInput, PeakPyramid, decode_bytes, decode_path, mix, resample, waveform_peaks};
use anim_core::{Tick, TimeRange};

fn samples() -> Option<PathBuf> {
    std::env::var_os("ANICRAFT_SAMPLES").map(PathBuf::from)
}

#[test]
fn news_song_wav_decodes_and_feeds_the_timeline_tools() {
    let Some(root) = samples() else { return };
    let path = root.join("chproj_full/news anchors/Ch Media/Gathered Media/news_song_vocals_mixdown_short.wav");
    if !path.exists() {
        return;
    }
    let buf = decode_path(&path).unwrap();
    assert!(buf.sample_rate >= 8000 && buf.channels >= 1);
    let secs = buf.seconds();
    assert!(secs > 10.0 && secs < 3600.0, "{secs} s");
    assert!(buf.peak() > 0.01, "silent decode");
    // The file size predicts the length: 16/24-bit PCM is 2..4 bytes per sample.
    let bytes = std::fs::metadata(&path).unwrap().len();
    let per_second = u64::from(buf.sample_rate) * u64::from(buf.channels);
    assert!(bytes / per_second / 4 <= secs.ceil() as u64 && secs.floor() as u64 <= bytes / per_second / 2 + 1);

    let one_second = buf.slice_ticks(TimeRange::new(Tick::from_units(5, 1), Tick::from_units(1, 1)));
    assert_eq!(one_second.frames(), buf.sample_rate as usize);
    let at_48k = resample(&one_second, 48_000).unwrap();
    assert_eq!(at_48k.frames(), 48_000);
    let peaks = waveform_peaks(&buf, 1000);
    assert_eq!(peaks.len(), 1000);
    assert!(peaks.iter().any(|p| p.max > 0.01));
    let pyramid = PeakPyramid::build(&buf);
    assert_eq!(pyramid.query(&buf, 0..buf.frames(), 1000).len(), 1000);
    let mixed = mix(&[MixInput::at(&one_second, Tick::ZERO)], MixFormat::STEREO_48K, None).unwrap();
    assert_eq!(mixed.frames(), 48_000);
}

#[test]
fn mp3_and_wav_inside_fla_files_decode() {
    let Some(root) = samples() else { return };
    for (fla, name, min_secs) in [("derwood_green_choreo_short.fla", "LIBRARY/DerwoodGreen.mp3", 60.0), ("unicorn_4AE.fla", "LIBRARY/SCROLLING 4-3.wav", 30.0)]
    {
        let path = root.join(fla);
        if !path.exists() {
            continue;
        }
        let container = Container::open(&path).unwrap();
        let bytes = container.read(name).unwrap();
        let buf = decode_bytes(&bytes, Some(name)).unwrap();
        eprintln!("{name}: {} Hz, {} ch, {:.1} s, peak {:.2}", buf.sample_rate, buf.channels, buf.seconds(), buf.peak());
        assert!(matches!(buf.sample_rate, 22_050 | 32_000 | 44_100 | 48_000), "{} Hz", buf.sample_rate);
        assert!(buf.seconds() > min_secs, "{name}: {} s", buf.seconds());
        assert!(buf.peak() > 0.01, "{name} decoded silent");
    }
}
