//! Video export: MP4 (H.264 + AAC) and MOV (`ProRes` 4444 with alpha) written with `FilmCraft`'s pure-Rust codecs.
//!
//! Encoding runs in an actor. [`start`] validates the settings, spawns a thread that owns the encoders and the output
//! file, and returns an [`EncoderHandle`]. Frames go over a small bounded channel (the producer blocks when the encoder
//! falls behind, so rendering can never queue up unbounded memory), progress comes back over another channel, and
//! [`EncoderHandle::finish`] / [`EncoderHandle::cancel`] end the job.
//!
//! ```no_run
//! use anim_core::FrameRate;
//! use anim_display::Image;
//! use anim_video::{Codec, H264Rate, VideoSettings, start};
//!
//! # fn main() -> Result<(), anim_video::VideoError> {
//! let settings = VideoSettings::new(640, 360, FrameRate::FPS_24, Codec::H264 { rate: H264Rate::Quality { crf: 20.0 } });
//! let mut job = start("out.mp4", settings)?;
//! for _ in 0..24 {
//!     job.send_frame(Image::new(640, 360, vec![0; 640 * 360 * 4]).map_err(|e| anim_video::VideoError::InvalidFrame(e.to_string()))?)?;
//! }
//! let summary = job.finish()?;
//! assert_eq!(summary.frames, 24);
//! # Ok(())
//! # }
//! ```
//!
//! Pixels: frames are premultiplied RGBA8 ([`anim_display::Image`]). MP4 has no alpha, so the picture is encoded as if
//! composited over black. `ProRes` 4444 with `alpha: true` stores straight colour plus a 10-bit alpha plane;
//! with `alpha: false` it is composited over black like MP4. Colour is BT.709 limited range.
//!
//! Audio: an [`AudioTrack`] holds the whole soundtrack as interleaved `f32`. It is interleaved with the video as frames
//! are written and truncated to the video's duration. MP4 gets AAC-LC (1 or 2 channels), MOV gets 16-bit PCM.

mod convert;
mod encoder;

use std::path::PathBuf;

use anim_core::{FrameRate, Tick};

pub use encoder::{EncoderHandle, encode_to_file, start};

/// Largest accepted frame side, in pixels.
pub const MAX_DIMENSION: u32 = 16_384;
/// Largest accepted frame area, in pixels (8K UHD is 33 177 600).
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// How H.264 trades size for quality.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum H264Rate {
    /// Variable bitrate averaging `kbps` kilobits per second.
    Bitrate { kbps: u32 },
    /// Constant quality, x264-style CRF (0 = best, 51 = worst; 18 to 23 is the useful range).
    Quality { crf: f32 },
}

impl Default for H264Rate {
    fn default() -> Self {
        Self::Quality { crf: 20.0 }
    }
}

/// The video codec and, with it, the container.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Codec {
    /// H.264 High profile in an MP4 (audio: AAC). Width and height must be even.
    H264 { rate: H264Rate },
    /// Apple `ProRes` 4444 in a MOV (audio: PCM). `alpha` stores a transparency plane.
    ProRes4444 { alpha: bool },
}

impl Codec {
    /// The file extension that matches the container, without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::H264 { .. } => "mp4",
            Self::ProRes4444 { .. } => "mov",
        }
    }
}

/// A whole soundtrack: interleaved `f32` samples in -1.0 ..= 1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrack {
    pub samples: Vec<f32>,
    /// Sample frames per second.
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioTrack {
    /// Number of sample frames (one sample per channel each).
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len().checked_div(usize::from(self.channels)).unwrap_or(0)
    }
}

/// Everything the encoder needs to know before the first frame.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    pub codec: Codec,
    pub audio: Option<AudioTrack>,
}

impl VideoSettings {
    /// Settings without audio.
    #[must_use]
    pub const fn new(width: u32, height: u32, fps: FrameRate, codec: Codec) -> Self {
        Self { width, height, fps, codec, audio: None }
    }

    /// Adds a soundtrack.
    #[must_use]
    pub fn with_audio(mut self, audio: AudioTrack) -> Self {
        self.audio = Some(audio);
        self
    }

    /// Checks every limit the encoders impose, so `start` can fail before anything is written.
    ///
    /// # Errors
    /// [`VideoError::InvalidSettings`] naming the first problem.
    pub fn validate(&self) -> Result<(), VideoError> {
        let bad = |msg: String| Err(VideoError::InvalidSettings(msg));
        let (w, h) = (self.width, self.height);
        if w == 0 || h == 0 || w > MAX_DIMENSION || h > MAX_DIMENSION || u64::from(w).saturating_mul(u64::from(h)) > MAX_PIXELS {
            return bad(format!("frame size {w}x{h} is outside 1..={MAX_DIMENSION} per side and {MAX_PIXELS} pixels"));
        }
        if self.fps.num <= 0 || self.fps.den <= 0 || u32::try_from(self.fps.num).is_err() || u32::try_from(self.fps.den).is_err() {
            return bad(format!("frame rate {}/{} is not a positive rational that fits 32 bits", self.fps.num, self.fps.den));
        }
        if let Codec::H264 { rate } = self.codec {
            if w % 2 != 0 || h % 2 != 0 {
                return bad(format!("H.264 needs even dimensions, got {w}x{h}"));
            }
            match rate {
                H264Rate::Bitrate { kbps } if !(1..=2_000_000).contains(&kbps) => return bad(format!("bitrate {kbps} kbps is out of range")),
                H264Rate::Quality { crf } if !(0.0..=51.0).contains(&crf) => return bad(format!("crf {crf} is outside 0..=51")),
                _ => {}
            }
        }
        if let Some(a) = &self.audio {
            let max_channels = match self.codec {
                Codec::H264 { .. } => 2,
                Codec::ProRes4444 { .. } => 8,
            };
            if !(1..=max_channels).contains(&a.channels) {
                return bad(format!("{} audio channels is outside 1..={max_channels} for this container", a.channels));
            }
            if !(8_000..=192_000).contains(&a.sample_rate) {
                return bad(format!("audio sample rate {} is outside 8000..=192000", a.sample_rate));
            }
            if a.samples.len().checked_rem(usize::from(a.channels)) != Some(0) {
                return bad("audio samples are not a whole number of frames".to_owned());
            }
        }
        Ok(())
    }
}

/// What a finished job produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub path: PathBuf,
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    /// `frames / fps`.
    pub duration: Tick,
    /// Size of the file on disk.
    pub bytes: u64,
    pub has_audio: bool,
}

/// Events from the encoder thread, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// A frame was encoded; `frames` counts all frames so far.
    Encoded { frames: u64 },
    /// The file is complete.
    Done(Summary),
    /// The job failed; the partial file was removed.
    Failed(String),
    /// The job was cancelled; the partial file was removed.
    Cancelled,
}

/// Why encoding failed.
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("invalid video settings: {0}")]
    InvalidSettings(String),
    #[error("invalid frame: {0}")]
    InvalidFrame(String),
    #[error("frame is {got_width}x{got_height} but the video is {width}x{height}")]
    WrongFrameSize { width: u32, height: u32, got_width: u32, got_height: u32 },
    #[error("no frames were sent")]
    NoFrames,
    #[error("the encoder thread has stopped; call finish() for the reason")]
    EncoderStopped,
    #[error("cancelled")]
    Cancelled,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("encoder error: {0}")]
    Encode(String),
    #[error("container error: {0}")]
    Mux(String),
}

#[cfg(test)]
mod tests;
