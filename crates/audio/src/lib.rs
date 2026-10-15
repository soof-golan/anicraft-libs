//! Audio for `AniCraft` and `CharCraft`.
//!
//! - [`decode`]: MP3, WAV/PCM, AAC, FLAC and Vorbis to [`PcmBuffer`] (interleaved `f32`), from bytes or paths, with
//!   input and output caps; [`decode_raw`] and [`decode_flash_sound`] cover the payloads of FLA/XFL sound items.
//! - [`resample`], [`mix`] (placement by [`anim_core::Tick`], gain, pan), [`PcmBuffer::slice_ticks`] and waveform
//!   [`waveform_peaks`] / [`PeakPyramid`] for the timeline.
//! - [`PlaybackActor`] plays a sound from a thread that owns the output device and reports the playhead; without a
//!   device it runs a silent wall clock. [`CaptureActor`] records the microphone and delivers chunks. Devices are
//!   behind the [`OutputBackend`] / [`InputBackend`] traits, so the protocols are tested with fakes; the cpal backends
//!   live in the `device` module (feature `device`, on by default).

#![cfg_attr(
    test,
    allow(
        clippy::as_conversions,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::float_cmp,
        clippy::items_after_statements,
        clippy::needless_range_loop
    )
)]

mod buffer;
mod capture;
mod decode;
#[cfg(feature = "device")]
pub mod device;
mod error;
mod mix;
mod peaks;
mod playback;
mod resample;

pub use buffer::{MAX_CHANNELS, MAX_SAMPLE_RATE, PcmBuffer, frame_to_tick, tick_to_frame};
pub use capture::{CaptureActor, CaptureCommand, CaptureEvent, ChunkSink, ErrorReporter, InputBackend, InputFormat, Recorder};
pub use decode::{
    DecodeLimits, FlashSoundFormat, RawCodec, RawFormat, decode_bytes, decode_bytes_with, decode_flash_sound, decode_path, decode_path_with, decode_raw,
    decode_raw_with,
};
pub use error::{AudioError, Result};
pub use mix::{MixFormat, MixInput, mix};
pub use peaks::{Peak, PeakPyramid, waveform_peaks, waveform_peaks_frames, waveform_peaks_range};
pub use playback::{ClockSource, OutputBackend, OutputFormat, PlaybackActor, PlaybackClock, PlaybackCommand, PlaybackEvent, Renderer, prepare};
pub use resample::resample;
