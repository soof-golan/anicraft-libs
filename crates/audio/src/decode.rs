//! Decoding to [`PcmBuffer`]: MP3, WAV/PCM, AAC, FLAC and Ogg Vorbis through symphonia, plus the raw payloads that
//! FLA/XFL sound items store.
//!
//! All entry points treat the input as hostile: the input size and the decoded sample count are capped
//! ([`DecodeLimits`]), damaged packets are skipped, and a stream that fails after some audio was decoded returns the
//! audio decoded so far.

use std::fs::File;
use std::io::Cursor;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::buffer::{MAX_CHANNELS, MAX_SAMPLE_RATE, PcmBuffer};
use crate::error::{AudioError, Result};

/// Caps applied while decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Largest input accepted, in bytes.
    pub max_input_bytes: u64,
    /// Largest decoded size accepted, in samples over all channels.
    pub max_samples: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        // 512 MiB of input; 400 M samples is about 70 minutes of 48 kHz stereo (1.6 GB of f32).
        Self { max_input_bytes: 512 * 1024 * 1024, max_samples: 400_000_000 }
    }
}

/// How the bytes of a raw (headerless) sound are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawCodec {
    /// A complete MP3 stream (rate and channels come from the stream; those in [`RawFormat`] are ignored).
    Mp3,
    /// Unsigned 8-bit PCM, as Flash stores 8-bit sounds.
    Pcm8,
    /// Signed 16-bit little-endian PCM.
    Pcm16Le,
    /// Signed 16-bit big-endian PCM.
    Pcm16Be,
    /// 32-bit IEEE float little-endian PCM.
    Float32Le,
    /// Any container symphonia can probe (WAV, FLAC, Ogg, MP4/AAC ...).
    Probe,
}

/// Description of a raw sound payload, as found next to it in an XFL `DOMSoundItem` (`format="44kHz Stereo 16 bit"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawFormat {
    /// The encoding.
    pub codec: RawCodec,
    /// Frames per second (PCM codecs).
    pub sample_rate: u32,
    /// Channels (PCM codecs).
    pub channels: u16,
}

impl RawFormat {
    /// 16-bit little-endian PCM at the given rate and channel count.
    #[must_use]
    pub const fn pcm16(sample_rate: u32, channels: u16) -> Self {
        Self { codec: RawCodec::Pcm16Le, sample_rate, channels }
    }
}

/// The parsed `format` attribute of an XFL sound item, for example `"44kHz Stereo 16 bit"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashSoundFormat {
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels.
    pub channels: u16,
    /// Bits per sample (8 or 16).
    pub bits: u8,
}

impl FlashSoundFormat {
    /// Parses text like `"44kHz Stereo 16 bit"`, `"22kHz Mono 8 bit"` or `"48kHz Stereo 16 bit"`.
    /// Flash's nominal `5`, `11`, `22` and `44` kHz mean 5512, 11025, 22050 and 44100 Hz.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut rate = None;
        let mut channels = None;
        let mut bits = None;
        let words: Vec<&str> = text.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            let lower = w.to_ascii_lowercase();
            if let Some(khz) = lower.strip_suffix("khz") {
                let nominal: f64 = khz.parse().ok()?;
                rate = Some(match anim_core::num::round_f64_to_u32(nominal) {
                    5 => 5512,
                    11 => 11_025,
                    22 => 22_050,
                    44 => 44_100,
                    other => other.checked_mul(1000)?,
                });
            } else if lower == "mono" {
                channels = Some(1);
            } else if lower == "stereo" {
                channels = Some(2);
            } else if lower == "bit" || lower == "bits" {
                bits = i.checked_sub(1).and_then(|p| words.get(p)).and_then(|b| b.parse::<u8>().ok());
            }
        }
        let rate = rate?;
        let channels = channels?;
        let bits = bits.unwrap_or(16);
        (rate > 0 && rate <= MAX_SAMPLE_RATE && (bits == 8 || bits == 16)).then_some(Self { sample_rate: rate, channels, bits })
    }

    /// The matching raw PCM description.
    #[must_use]
    pub const fn raw_format(self) -> RawFormat {
        RawFormat { codec: if self.bits == 8 { RawCodec::Pcm8 } else { RawCodec::Pcm16Le }, sample_rate: self.sample_rate, channels: self.channels }
    }
}

/// Decodes an encoded file held in memory. `name_hint` (a file name or extension such as `"mp3"`) only speeds up
/// format detection.
///
/// # Errors
/// [`AudioError::InputTooLarge`], [`AudioError::Unsupported`] when no audio stream is found and
/// [`AudioError::Corrupt`] when nothing could be decoded.
pub fn decode_bytes(bytes: &[u8], name_hint: Option<&str>) -> Result<PcmBuffer> {
    decode_bytes_with(bytes, name_hint, &DecodeLimits::default())
}

/// [`decode_bytes`] with explicit limits.
///
/// # Errors
/// As [`decode_bytes`].
pub fn decode_bytes_with(bytes: &[u8], name_hint: Option<&str>, limits: &DecodeLimits) -> Result<PcmBuffer> {
    check_input(u64::try_from(bytes.len()).unwrap_or(u64::MAX), limits)?;
    decode_source(Box::new(Cursor::new(bytes.to_vec())), name_hint, limits)
}

/// Decodes a file from disk, streaming it rather than reading it all into memory.
///
/// # Errors
/// [`AudioError::Io`] when the file cannot be opened, otherwise as [`decode_bytes`].
pub fn decode_path(path: &Path) -> Result<PcmBuffer> {
    decode_path_with(path, &DecodeLimits::default())
}

/// [`decode_path`] with explicit limits.
///
/// # Errors
/// As [`decode_path`].
pub fn decode_path_with(path: &Path, limits: &DecodeLimits) -> Result<PcmBuffer> {
    let io = |source| AudioError::Io { path: path.to_path_buf(), source };
    let file = File::open(path).map_err(io)?;
    let size = file.metadata().map_err(io)?.len();
    check_input(size, limits)?;
    let hint = path.extension().and_then(|e| e.to_str());
    decode_source(Box::new(file), hint, limits)
}

/// Decodes a raw payload described by `format` (the contents of an FLA `bin/*.dat` sound, for example).
///
/// # Errors
/// [`AudioError::InvalidFormat`] for a zero or absurd rate or channel count, and the errors of [`decode_bytes`] for
/// the container-based codecs.
pub fn decode_raw(bytes: &[u8], format: RawFormat) -> Result<PcmBuffer> {
    decode_raw_with(bytes, format, &DecodeLimits::default())
}

/// [`decode_raw`] with explicit limits.
///
/// # Errors
/// As [`decode_raw`].
pub fn decode_raw_with(bytes: &[u8], format: RawFormat, limits: &DecodeLimits) -> Result<PcmBuffer> {
    match format.codec {
        RawCodec::Mp3 => return decode_bytes_with(bytes, Some("mp3"), limits),
        RawCodec::Probe => return decode_bytes_with(bytes, None, limits),
        _ => {}
    }
    check_input(u64::try_from(bytes.len()).unwrap_or(u64::MAX), limits)?;
    if format.sample_rate == 0 || format.sample_rate > MAX_SAMPLE_RATE || format.channels == 0 || format.channels > MAX_CHANNELS {
        return Err(AudioError::InvalidFormat(format!("{} Hz, {} channels", format.sample_rate, format.channels)));
    }
    let width: usize = match format.codec {
        RawCodec::Pcm8 => 1,
        RawCodec::Pcm16Le | RawCodec::Pcm16Be => 2,
        _ => 4,
    };
    let frame_bytes = width.saturating_mul(usize::from(format.channels)).max(1);
    let whole = bytes.len().checked_div(frame_bytes).unwrap_or(0).saturating_mul(frame_bytes);
    let used = bytes.get(..whole).unwrap_or_default();
    let count = used.len().checked_div(width).unwrap_or(0);
    if count > limits.max_samples {
        return Err(AudioError::OutputTooLarge { limit: limits.max_samples });
    }
    let samples: Vec<f32> = match format.codec {
        RawCodec::Pcm8 => used.iter().map(|&b| (f32::from(b) - 128.0) / 128.0).collect(),
        RawCodec::Pcm16Le => used.as_chunks::<2>().0.iter().map(|c| i16_to_f32(i16::from_le_bytes(*c))).collect(),
        RawCodec::Pcm16Be => used.as_chunks::<2>().0.iter().map(|c| i16_to_f32(i16::from_be_bytes(*c))).collect(),
        _ => used.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect(),
    };
    let mut buf = PcmBuffer::new(format.sample_rate, format.channels, samples);
    buf.sanitize();
    Ok(buf)
}

/// Decodes the payload of a Flash sound item: a complete file (MP3, WAV, FLAC ...) when symphonia recognises it,
/// otherwise raw PCM as described by the item's `format` text (`"44kHz Stereo 16 bit"`).
///
/// # Errors
/// [`AudioError::Unsupported`] when the data is not a recognised file and `format` is missing or unparsable.
pub fn decode_flash_sound(bytes: &[u8], name_hint: Option<&str>, format: Option<&str>) -> Result<PcmBuffer> {
    match decode_bytes(bytes, name_hint) {
        Ok(b) => Ok(b),
        Err(AudioError::Unsupported(why)) => {
            format.and_then(FlashSoundFormat::parse).map_or(Err(AudioError::Unsupported(why)), |f| decode_raw(bytes, f.raw_format()))
        }
        Err(e) => Err(e),
    }
}

fn i16_to_f32(v: i16) -> f32 {
    f32::from(v) / 32768.0
}

const fn check_input(size: u64, limits: &DecodeLimits) -> Result<()> {
    if size > limits.max_input_bytes {
        return Err(AudioError::InputTooLarge { size, limit: limits.max_input_bytes });
    }
    Ok(())
}

/// How many undecodable packets in a row end decoding.
const MAX_CONSECUTIVE_DECODE_ERRORS: u32 = 256;
/// Most frames reserved up front from what the stream claims.
const MAX_RESERVE_SAMPLES: usize = 1 << 24;

fn decode_source(source: Box<dyn MediaSource>, name_hint: Option<&str>, limits: &DecodeLimits) -> Result<PcmBuffer> {
    let mss = MediaSourceStream::new(source, symphonia::core::io::MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(name) = name_hint {
        let ext = name.rsplit_once('.').map_or(name, |(_, e)| e);
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AudioError::Unsupported(e.to_string()))?;
    let mut format = probed.format;
    let (track_id, params) = {
        let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or_else(|| AudioError::Unsupported("no audio track".into()))?;
        (track.id, track.codec_params.clone())
    };
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(|e| AudioError::Unsupported(e.to_string()))?;

    let mut out: Option<PcmBuffer> = None;
    let mut scratch: Option<SampleBuffer<f32>> = None;
    let mut errors_in_a_row = 0_u32;
    let mut last_error: Option<String> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::ResetRequired | SymError::IoError(_)) => break,
            Err(e) => {
                last_error = Some(e.to_string());
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(why)) => {
                errors_in_a_row = errors_in_a_row.saturating_add(1);
                last_error = Some(why.to_string());
                if errors_in_a_row >= MAX_CONSECUTIVE_DECODE_ERRORS {
                    break;
                }
                continue;
            }
            Err(SymError::IoError(_)) => continue,
            Err(e) => {
                last_error = Some(e.to_string());
                break;
            }
        };
        errors_in_a_row = 0;
        let spec = *audio.spec();
        let channels = u16::try_from(spec.channels.count()).unwrap_or(0);
        if spec.rate == 0 || spec.rate > MAX_SAMPLE_RATE || channels == 0 || channels > MAX_CHANNELS {
            return Err(AudioError::InvalidFormat(format!("{} Hz, {channels} channels", spec.rate)));
        }
        let buf = out.get_or_insert_with(|| {
            let estimate = params
                .n_frames
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_mul(usize::from(channels)))
                .unwrap_or(0)
                .min(MAX_RESERVE_SAMPLES)
                .min(limits.max_samples);
            PcmBuffer::new(spec.rate, channels, Vec::with_capacity(estimate))
        });
        if buf.sample_rate != spec.rate || buf.channels != channels {
            // A mid-stream format change is not something timelines can use: keep what was decoded.
            break;
        }
        let frames = audio.capacity();
        let sb = match &mut scratch {
            Some(sb) if sb.capacity() >= frames.saturating_mul(usize::from(channels)) => sb,
            slot => slot.insert(SampleBuffer::<f32>::new(u64::try_from(frames).unwrap_or(0), spec)),
        };
        sb.copy_interleaved_ref(audio);
        let chunk = sb.samples();
        if buf.samples.len().saturating_add(chunk.len()) > limits.max_samples {
            return Err(AudioError::OutputTooLarge { limit: limits.max_samples });
        }
        buf.samples.extend_from_slice(chunk);
    }

    out.map_or_else(
        || Err(last_error.map_or_else(|| AudioError::Unsupported("no audio frames".into()), AudioError::Corrupt)),
        |mut buf| {
            buf.sanitize();
            Ok(buf)
        },
    )
}

#[cfg(test)]
pub mod testutil {
    //! Builders for small WAV files, used by tests in this crate.

    use crate::buffer::PcmBuffer;

    /// A canonical 16-bit PCM WAV file.
    pub fn wav16(buf: &PcmBuffer) -> Vec<u8> {
        let data: Vec<u8> = buf
            .samples
            .iter()
            .flat_map(|s| {
                let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                v.to_le_bytes()
            })
            .collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&buf.channels.to_le_bytes());
        out.extend_from_slice(&buf.sample_rate.to_le_bytes());
        out.extend_from_slice(&(buf.sample_rate * u32::from(buf.channels) * 2).to_le_bytes());
        out.extend_from_slice(&(buf.channels * 2).to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    /// A sine wave at `hz`, `amp` amplitude.
    pub fn sine(rate: u32, channels: u16, frames: usize, hz: f32, amp: f32) -> PcmBuffer {
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for i in 0..frames {
            let v = amp * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
            for _ in 0..channels {
                samples.push(v);
            }
        }
        PcmBuffer::new(rate, channels, samples)
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::{sine, wav16};
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn wav_round_trip() {
        let src = sine(22_050, 2, 2000, 440.0, 0.5);
        let bytes = wav16(&src);
        let got = decode_bytes(&bytes, Some("clip.wav")).unwrap();
        assert_eq!(got.sample_rate, 22_050);
        assert_eq!(got.channels, 2);
        assert_eq!(got.frames(), 2000);
        for (a, b) in src.samples.iter().zip(&got.samples) {
            assert!((a - b).abs() < 1.0 / 16384.0, "{a} vs {b}");
        }
    }

    #[test]
    fn wav_without_hint_and_from_path() {
        let src = sine(8000, 1, 800, 100.0, 0.25);
        let bytes = wav16(&src);
        assert_eq!(decode_bytes(&bytes, None).unwrap().frames(), 800);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        std::fs::write(&p, &bytes).unwrap();
        let got = decode_path(&p).unwrap();
        assert_eq!((got.sample_rate, got.channels, got.frames()), (8000, 1, 800));
        assert!(matches!(decode_path(&dir.path().join("missing.wav")), Err(AudioError::Io { .. })));
    }

    #[test]
    fn truncated_wav_keeps_what_was_decoded() {
        let src = sine(8000, 1, 4000, 100.0, 0.25);
        let bytes = wav16(&src);
        let cut = bytes.get(..bytes.len() / 2).unwrap();
        let got = decode_bytes(cut, None).unwrap();
        assert!(got.frames() > 1000 && got.frames() < 4000, "{}", got.frames());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_bytes(&[], None).is_err());
        assert!(decode_bytes(b"this is definitely not audio at all", Some("mp3")).is_err());
        let zeros = vec![0_u8; 4096];
        assert!(decode_bytes(&zeros, None).is_err());
    }

    #[test]
    fn limits_are_enforced() {
        let src = sine(8000, 1, 4000, 100.0, 0.25);
        let bytes = wav16(&src);
        let tiny_input = DecodeLimits { max_input_bytes: 100, ..DecodeLimits::default() };
        assert!(matches!(decode_bytes_with(&bytes, None, &tiny_input), Err(AudioError::InputTooLarge { .. })));
        let tiny_output = DecodeLimits { max_samples: 1000, ..DecodeLimits::default() };
        assert!(matches!(decode_bytes_with(&bytes, None, &tiny_output), Err(AudioError::OutputTooLarge { .. })));
    }

    #[test]
    fn raw_pcm_formats() {
        let le = [0x00, 0x40, 0x00, 0xC0, 0xFF, 0x7F, 0x00, 0x80];
        let got = decode_raw(&le, RawFormat::pcm16(44_100, 2)).unwrap();
        assert_eq!(got.frames(), 2);
        assert_eq!(got.samples.first().copied(), Some(0.5));
        assert_eq!(got.samples.get(1).copied(), Some(-0.5));
        assert_eq!(got.samples.get(3).copied(), Some(-1.0));
        let be = RawFormat { codec: RawCodec::Pcm16Be, sample_rate: 8000, channels: 1 };
        assert_eq!(decode_raw(&[0x40, 0x00], be).unwrap().samples, vec![0.5]);
        let u8f = RawFormat { codec: RawCodec::Pcm8, sample_rate: 8000, channels: 1 };
        assert_eq!(decode_raw(&[128, 255, 0], u8f).unwrap().samples.first().copied(), Some(0.0));
        let f32f = RawFormat { codec: RawCodec::Float32Le, sample_rate: 8000, channels: 1 };
        assert_eq!(decode_raw(&0.25_f32.to_le_bytes(), f32f).unwrap().samples, vec![0.25]);
        // A trailing partial frame is dropped.
        assert_eq!(decode_raw(&[0, 0, 0, 0, 0], RawFormat::pcm16(8000, 2)).unwrap().frames(), 1);
    }

    #[test]
    fn raw_format_validation() {
        assert!(matches!(decode_raw(&[0, 0], RawFormat::pcm16(0, 1)), Err(AudioError::InvalidFormat(_))));
        assert!(matches!(decode_raw(&[0, 0], RawFormat::pcm16(8000, 0)), Err(AudioError::InvalidFormat(_))));
        assert!(matches!(decode_raw(&[0, 0], RawFormat::pcm16(8000, 9999)), Err(AudioError::InvalidFormat(_))));
    }

    #[test]
    fn flash_format_text() {
        let f = FlashSoundFormat::parse("44kHz Stereo 16 bit").unwrap();
        assert_eq!((f.sample_rate, f.channels, f.bits), (44_100, 2, 16));
        let f = FlashSoundFormat::parse("22kHz Mono 8 bit").unwrap();
        assert_eq!((f.sample_rate, f.channels, f.bits), (22_050, 1, 8));
        assert_eq!(FlashSoundFormat::parse("48kHz Stereo 16 bit").unwrap().sample_rate, 48_000);
        assert_eq!(FlashSoundFormat::parse("11kHz Mono").unwrap().bits, 16);
        assert!(FlashSoundFormat::parse("loud").is_none());
        assert!(FlashSoundFormat::parse("44kHz 16 bit").is_none());
    }

    #[test]
    fn flash_sound_falls_back_to_raw_pcm() {
        // A real file is probed first.
        let wav = wav16(&sine(8000, 1, 100, 100.0, 0.5));
        assert_eq!(decode_flash_sound(&wav, None, Some("22kHz Mono 16 bit")).unwrap().sample_rate, 8000);
        // Headerless data uses the format text.
        let raw = [0_u8; 400];
        let got = decode_flash_sound(&raw, Some("x.dat"), Some("22kHz Mono 16 bit")).unwrap();
        assert_eq!((got.sample_rate, got.channels, got.frames()), (22_050, 1, 200));
        assert!(decode_flash_sound(&raw, None, None).is_err());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048), hint in prop::option::of("[a-z0-9.]{0,8}")) {
            let _ = decode_bytes(&bytes, hint.as_deref());
        }

        #[test]
        fn mutated_wav_never_panics(flips in proptest::collection::vec((0_usize..2000, any::<u8>()), 1..16)) {
            let mut bytes = wav16(&sine(8000, 2, 300, 100.0, 0.5));
            for (i, v) in flips {
                if let Some(b) = bytes.get_mut(i) { *b = v; }
            }
            let _ = decode_bytes(&bytes, Some("wav"));
        }

        #[test]
        fn random_raw_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512), rate in 0_u32..100_000, ch in 0_u16..80) {
            let _ = decode_raw(&bytes, RawFormat::pcm16(rate, ch));
        }
    }
}
