#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use std::path::Path;

use anim_core::FrameRate;
use anim_display::Image;
use filmcraft_isobmff::{CodecConfig, Mp4File, TrackKind, open};
use proptest::prelude::*;

use super::*;

const W: u32 = 64;
const H: u32 = 48;
const FRAMES: u64 = 12;

/// Left half red moving right by `t` columns, right half transparent green-ish; premultiplied.
fn frame(t: u32, opaque: bool) -> Image {
    let mut px = Vec::new();
    for _y in 0..H {
        for x in 0..W {
            let p = if x < 32 + t {
                [200, 20, 20, 255]
            } else if opaque {
                [20, 160, 40, 255]
            } else {
                [0, 0, 0, 0]
            };
            px.extend_from_slice(&p);
        }
    }
    Image::new(W, H, px).unwrap()
}

fn sine(rate: u32, channels: u16, seconds_num: u32, seconds_den: u32) -> AudioTrack {
    let frames = (u64::from(rate) * u64::from(seconds_num) / u64::from(seconds_den)) as usize;
    let mut samples = Vec::new();
    for i in 0..frames {
        let s = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
        for _ in 0..channels {
            samples.push(s);
        }
    }
    AudioTrack { samples, sample_rate: rate, channels }
}

fn settings(codec: Codec) -> VideoSettings {
    VideoSettings::new(W, H, FrameRate::FPS_24, codec)
}

fn encode(path: &Path, s: VideoSettings, opaque: bool) -> Summary {
    let frames = (0..FRAMES).map(|t| frame(u32::try_from(t).unwrap(), opaque));
    encode_to_file(path, s, frames).unwrap()
}

fn demux(path: &Path) -> (Mp4File, Vec<u8>) {
    let bytes = std::fs::read(path).unwrap();
    (open(bytes.clone()).unwrap(), bytes)
}

fn mean(plane: &[u8]) -> f64 {
    plane.iter().map(|&v| f64::from(v)).sum::<f64>() / plane.len() as f64
}

#[test]
fn mp4_has_twelve_h264_frames_and_decodes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.mp4");
    let summary = encode(&path, settings(Codec::H264 { rate: H264Rate::Quality { crf: 18.0 } }), true);
    assert_eq!((summary.frames, summary.width, summary.height, summary.has_audio), (FRAMES, W, H, false));
    assert_eq!(summary.bytes, std::fs::metadata(&path).unwrap().len());
    assert_eq!(summary.duration, Tick::from_units(12, 24));

    let (mp4, bytes) = demux(&path);
    assert!(!mp4.is_quicktime);
    assert_eq!(mp4.tracks.len(), 1);
    let t = &mp4.tracks[0];
    assert_eq!(t.kind, TrackKind::Video);
    assert_eq!((t.width, t.height), (W, H));
    assert_eq!(t.samples.len() as u64, FRAMES);
    assert_eq!(t.timescale, 24);
    assert_eq!(t.duration, FRAMES);
    assert!(t.samples[0].is_sync);
    assert_eq!(t.edits.len(), 0);
    assert!(t.samples.iter().all(|s| s.pts == s.dts), "no B-frames");
    // moov precedes mdat (fast start): the first 64 KiB contain it.
    assert!(mp4.moov_offset < 4096);

    let Some(CodecConfig::Avc(avc)) = t.codec() else { panic!("not AVC") };
    let mut dec = filmcraft_h264::Decoder::from_avcc(&avc.to_bytes()).unwrap();
    let mut pictures = Vec::new();
    for i in 0..t.samples.len() {
        let au = mp4.read_sample(&bytes, 0, i).unwrap();
        pictures.extend(dec.decode(&au, t.samples[i].pts).unwrap());
    }
    pictures.extend(dec.flush());
    assert_eq!(pictures.len() as u64, FRAMES);
    let p = &pictures[0];
    assert_eq!((p.width, p.height), (W, H));
    // Left of the red/green boundary is red (luma about 16 + 219 * 0.31), right is green (about 0.50): compare the halves.
    let row = &p.y[..p.y_stride * H as usize];
    let left: Vec<u8> = row.chunks(p.y_stride).flat_map(|r| r[2..28].to_vec()).collect();
    let right: Vec<u8> = row.chunks(p.y_stride).flat_map(|r| r[38..60].to_vec()).collect();
    assert!(mean(&right) > mean(&left) + 20.0, "left {} right {}", mean(&left), mean(&right));
}

#[test]
fn mp4_with_audio_has_an_aac_track_of_matching_duration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("av.mp4");
    // One second of 48 kHz stereo against half a second of video: truncated to the video.
    let s = settings(Codec::H264 { rate: H264Rate::Bitrate { kbps: 400 } }).with_audio(sine(48_000, 2, 1, 1));
    let summary = encode(&path, s, true);
    assert!(summary.has_audio);
    let (mp4, _) = demux(&path);
    assert_eq!(mp4.tracks.len(), 2);
    let a = &mp4.tracks[mp4.track_of_kind(TrackKind::Audio).unwrap()];
    assert!(matches!(a.codec(), Some(CodecConfig::Aac(_))));
    let params = a.audio().unwrap();
    assert_eq!(params.channels, 2);
    assert!((params.sample_rate - 48_000.0).abs() < 1.0);
    assert_eq!(a.timescale, 48_000);
    // 0.5 s of audio is 24 000 sample frames = 23.4 AAC frames, plus priming and a flush tail.
    let aac_frames = a.samples.len();
    assert!((22..=27).contains(&aac_frames), "{aac_frames} AAC frames");
    let seconds = a.samples.iter().map(|s| u64::from(s.duration)).sum::<u64>() as f64 / 48_000.0;
    assert!((seconds - 0.5).abs() < 0.08, "audio is {seconds} s");
    // Video is unchanged by the audio.
    let v = &mp4.tracks[mp4.track_of_kind(TrackKind::Video).unwrap()];
    assert_eq!(v.samples.len() as u64, FRAMES);
}

#[test]
fn mono_audio_and_ntsc_rate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ntsc.mp4");
    let mut s = VideoSettings::new(W, H, FrameRate::FPS_29_97, Codec::H264 { rate: H264Rate::default() });
    s.audio = Some(sine(44_100, 1, 1, 2));
    let summary = encode_to_file(&path, s, (0..FRAMES).map(|t| frame(u32::try_from(t).unwrap(), true))).unwrap();
    assert_eq!(summary.fps, FrameRate::FPS_29_97);
    let (mp4, _) = demux(&path);
    let v = &mp4.tracks[mp4.track_of_kind(TrackKind::Video).unwrap()];
    assert_eq!(v.timescale, 30_000);
    assert!(v.samples.iter().all(|s| s.duration == 1001));
    assert_eq!(v.duration, 12 * 1001);
    assert!(mp4.track_of_kind(TrackKind::Audio).is_some());
}

#[test]
fn prores_mov_keeps_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.mov");
    let summary = encode(&path, settings(Codec::ProRes4444 { alpha: true }), false);
    assert_eq!(summary.frames, FRAMES);
    let (mov, bytes) = demux(&path);
    assert!(mov.is_quicktime);
    let t = &mov.tracks[0];
    assert_eq!((t.width, t.height, t.samples.len() as u64, t.duration), (W, H, FRAMES, FRAMES));
    let Some(CodecConfig::ProRes { fourcc }) = t.codec() else { panic!("not ProRes") };
    assert_eq!(&fourcc.0, b"ap4h");
    let first = filmcraft_prores::decode_frame(&mov.read_sample(&bytes, 0, 0).unwrap()).unwrap();
    assert_eq!((first.width, first.height), (W, H));
    let alpha = first.alpha.as_ref().expect("alpha plane");
    let at = |x: usize, y: usize| alpha[y * W as usize + x];
    assert!(at(10, 10) > 1000, "opaque red area: {}", at(10, 10));
    assert!(at(60, 10) < 24, "transparent area: {}", at(60, 10));
    // Straight colour: red stays saturated, so Cr is high and Y is the red luma, not a darkened one.
    // The decoder may widen 4444 to 12 bits; compare at 10.
    let shift = first.bit_depth - 10;
    let (y, cr) = (first.y[10 * W as usize + 10] >> shift, first.cr[10 * W as usize + 10] >> shift);
    assert!((230..300).contains(&y), "y {y}");
    assert!(cr > 650, "cr {cr}");
}

#[test]
fn prores_without_alpha_has_no_alpha_plane_and_pcm_audio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opaque.mov");
    let s = settings(Codec::ProRes4444 { alpha: false }).with_audio(sine(48_000, 2, 1, 2));
    encode(&path, s, true);
    let (mov, bytes) = demux(&path);
    let v = &mov.tracks[mov.track_of_kind(TrackKind::Video).unwrap()];
    let frame = filmcraft_prores::decode_frame(&mov.read_sample(&bytes, mov.track_of_kind(TrackKind::Video).unwrap(), 3).unwrap()).unwrap();
    assert!(frame.alpha.is_none());
    assert_eq!(v.samples.len() as u64, FRAMES);
    let a = &mov.tracks[mov.track_of_kind(TrackKind::Audio).unwrap()];
    let Some(CodecConfig::Pcm(pcm)) = a.codec() else { panic!("not PCM") };
    assert_eq!((pcm.bits, pcm.channels), (16, 2));
    // PCM samples are merged per chunk; the total duration is exactly the video's 0.5 s.
    let total: u64 = a.samples.iter().map(|s| u64::from(s.duration)).sum();
    assert_eq!(total, 24_000);
}

#[test]
fn odd_sizes_work_for_prores_but_not_h264() {
    let dir = tempfile::tempdir().unwrap();
    let odd = |codec| VideoSettings::new(33, 17, FrameRate::FPS_24, codec);
    assert!(matches!(odd(Codec::H264 { rate: H264Rate::default() }).validate(), Err(VideoError::InvalidSettings(_))));
    let path = dir.path().join("odd.mov");
    let frames = (0..3).map(|_| Image::new(33, 17, vec![255; 33 * 17 * 4]).unwrap());
    let summary = encode_to_file(&path, odd(Codec::ProRes4444 { alpha: true }), frames).unwrap();
    assert_eq!((summary.width, summary.height, summary.frames), (33, 17, 3));
}

#[test]
fn progress_events_arrive_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.mp4");
    let mut job = start(&path, settings(Codec::H264 { rate: H264Rate::default() })).unwrap();
    let progress = job.progress().clone();
    // More frames than the queue holds: send_frame must block rather than fail.
    for t in 0..20 {
        job.send_frame(frame(t % 16, true)).unwrap();
    }
    assert_eq!(job.frames_sent(), 20);
    let summary = job.finish().unwrap();
    let events: Vec<Progress> = progress.try_iter().collect();
    let counts: Vec<u64> = events.iter().filter_map(|e| if let Progress::Encoded { frames } = e { Some(*frames) } else { None }).collect();
    assert_eq!(counts, (1..=20).collect::<Vec<u64>>());
    assert_eq!(events.last(), Some(&Progress::Done(summary)));
}

#[test]
fn cancel_removes_the_partial_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.mp4");
    let mut job = start(&path, settings(Codec::H264 { rate: H264Rate::default() })).unwrap();
    let progress = job.progress().clone();
    job.send_frame(frame(0, true)).unwrap();
    job.send_frame(frame(1, true)).unwrap();
    job.cancel();
    assert!(!path.exists());
    assert_eq!(progress.try_iter().last(), Some(Progress::Cancelled));
}

#[test]
fn dropping_the_handle_cancels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("d.mp4");
    let mut job = start(&path, settings(Codec::H264 { rate: H264Rate::default() })).unwrap();
    let progress = job.progress().clone();
    job.send_frame(frame(0, true)).unwrap();
    drop(job);
    // The thread ends when the channel closes; the progress channel closes with it.
    let last = progress.iter().last();
    assert_eq!(last, Some(Progress::Cancelled));
    assert!(!path.exists());
}

#[test]
fn errors_are_reported_without_leaving_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.mp4");
    let codec = Codec::H264 { rate: H264Rate::default() };

    // No frames.
    let job = start(&path, settings(codec)).unwrap();
    assert!(matches!(job.finish(), Err(VideoError::NoFrames)));
    assert!(!path.exists());

    // Wrong size and inconsistent buffer are refused at send_frame; the job still finishes.
    let mut job = start(&path, settings(codec)).unwrap();
    let small = Image::new(2, 2, vec![0; 16]).unwrap();
    assert!(matches!(job.send_frame(small), Err(VideoError::WrongFrameSize { .. })));
    let broken = Image { width: W, height: H, premultiplied_rgba: vec![0; 10] };
    assert!(matches!(job.send_frame(broken), Err(VideoError::InvalidFrame(_))));
    job.send_frame(frame(0, true)).unwrap();
    assert_eq!(job.finish().unwrap().frames, 1);

    // Unwritable destination: the failure surfaces from finish().
    let bad = dir.path().join("missing-dir").join("x.mp4");
    let mut job = start(&bad, settings(codec)).unwrap();
    let _ = job.send_frame(frame(0, true));
    assert!(matches!(job.finish(), Err(VideoError::Io(_))));
}

#[test]
fn settings_validation_covers_every_limit() {
    let h264 = Codec::H264 { rate: H264Rate::default() };
    let ok = settings(h264);
    assert!(ok.validate().is_ok());
    let bad = |s: VideoSettings| assert!(matches!(s.validate(), Err(VideoError::InvalidSettings(_))), "{s:?}");
    bad(VideoSettings { width: 0, ..ok.clone() });
    bad(VideoSettings { width: MAX_DIMENSION + 2, ..ok.clone() });
    bad(VideoSettings { fps: FrameRate { num: 0, den: 1 }, ..ok.clone() });
    bad(VideoSettings { fps: FrameRate { num: 24, den: -1 }, ..ok.clone() });
    bad(VideoSettings { codec: Codec::H264 { rate: H264Rate::Quality { crf: 80.0 } }, ..ok.clone() });
    bad(VideoSettings { codec: Codec::H264 { rate: H264Rate::Quality { crf: f32::NAN } }, ..ok.clone() });
    bad(VideoSettings { codec: Codec::H264 { rate: H264Rate::Bitrate { kbps: 0 } }, ..ok.clone() });
    bad(ok.clone().with_audio(AudioTrack { samples: vec![0.0; 3], sample_rate: 48_000, channels: 2 }));
    bad(ok.clone().with_audio(AudioTrack { samples: vec![], sample_rate: 48_000, channels: 3 }));
    bad(ok.with_audio(AudioTrack { samples: vec![], sample_rate: 100, channels: 1 }));
    assert!(settings(Codec::ProRes4444 { alpha: true }).with_audio(AudioTrack { samples: vec![0.0; 12], sample_rate: 48_000, channels: 6 }).validate().is_ok());
    assert_eq!(h264.extension(), "mp4");
    assert_eq!(Codec::ProRes4444 { alpha: true }.extension(), "mov");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Arbitrary pixels, including invalid premultiplied data (colour above alpha), never panic and keep plane sizes.
    #[test]
    fn conversion_is_total(w in 1_u32..9, h in 1_u32..9, seed in any::<u64>()) {
        let n = (w * h) as usize;
        let mut state = seed | 1;
        let px: Vec<[u8; 4]> = (0..n).map(|_| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 32).to_le_bytes()[..4].try_into().unwrap()
        }).collect();
        let mut frame = filmcraft_prores::Frame::new(w, h, filmcraft_prores::ChromaFormat::Yuv422, 10, true);
        crate::convert::rgba_to_prores(&px, w as usize, true, false, &mut frame);
        prop_assert!(frame.y.iter().all(|&v| (4..=1019).contains(&v)));
        let (we, he) = (w as usize & !1, h as usize & !1);
        if we > 0 && he > 0 {
            let even: Vec<[u8; 4]> = (0..he).flat_map(|r| px[r * w as usize..r * w as usize + we].to_vec()).collect();
            let (mut y, mut u, mut v) = (vec![], vec![], vec![]);
            crate::convert::rgba_to_yuv420(&even, we, &mut y, &mut u, &mut v);
            prop_assert_eq!((y.len(), u.len(), v.len()), (we * he, we * he / 4, we * he / 4));
        }
    }
}
