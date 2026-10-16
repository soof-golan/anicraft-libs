//! Encoding a video in independent segments, so that many cores can each encode a run of frames.
//!
//! One H.264 encoder is a serial pipeline with a little parallelism inside each picture, which scales poorly beyond a
//! few cores. A key frame resets the encoder's references completely, so the stretch between two key frames (a segment)
//! can be encoded by its own encoder at the same time as the others, and the packets are muxed in order afterwards. With
//! constant-quality rate control the picture quality does not depend on which encoder coded a frame; bitrate-targeted
//! rate control keeps state across the whole video, so it stays in one segment.
//!
//! The caller renders the frames of a segment on the thread that encodes them (memory stays at a few frames per core),
//! then hands the finished [`Segment`] to [`EncoderHandle::send_segment`](crate::EncoderHandle::send_segment) in frame
//! order:
//!
//! ```no_run
//! use anim_core::FrameRate;
//! use anim_display::Image;
//! use anim_video::{Codec, H264Rate, SegmentEncoder, VideoSettings, start};
//!
//! # fn main() -> Result<(), anim_video::VideoError> {
//! let settings = VideoSettings::new(64, 48, FrameRate::FPS_24, Codec::H264 { rate: H264Rate::default() });
//! let plan = settings.segment_plan(100, 8);
//! let mut job = start("out.mp4", settings.clone())?;
//! let mut encoder = SegmentEncoder::new(&settings, plan.threads_each)?;
//! let mut first = 0;
//! while first < 100 {
//!     encoder.start(first)?;
//!     for _ in first..(first + plan.frames_per_segment).min(100) {
//!         encoder.push(&Image::new(64, 48, vec![0; 64 * 48 * 4]).map_err(|e| anim_video::VideoError::InvalidFrame(e.to_string()))?)?;
//!     }
//!     job.send_segment(encoder.finish()?)?;
//!     first += plan.frames_per_segment;
//! }
//! job.finish()?;
//! # Ok(())
//! # }
//! ```

use anim_display::Image;

use crate::encoder::{Packet, VideoEncoder, h264_keyint};
use crate::{Codec, H264Rate, VideoError, VideoSettings};

type Result<T> = std::result::Result<T, VideoError>;

/// Frames per `ProRes` segment: every `ProRes` frame is a key frame, so the length only trades hand-off cost for balance.
const PRORES_SEGMENT_FRAMES: u64 = 8;

/// How an export is cut into segments for a given number of cores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentPlan {
    /// Frames in every segment but the last, which may be shorter.
    pub frames_per_segment: u64,
    /// Number of segments.
    pub segments: u64,
    /// Threads each segment's encoder should use, so that segments running at the same time fill the cores.
    pub threads_each: usize,
}

impl VideoSettings {
    /// Cuts an export of `total_frames` into segments for `cores` cores.
    ///
    /// H.264 segments are one key-frame interval long, which is exactly the structure a single encoder produces, and
    /// bitrate-targeted H.264 is one segment. When there are fewer segments than cores the remaining cores go to the
    /// encoders inside each picture.
    #[must_use]
    pub fn segment_plan(&self, total_frames: u64, cores: usize) -> SegmentPlan {
        let total = total_frames.max(1);
        let frames_per_segment = match self.codec {
            Codec::H264 { rate: H264Rate::Quality { .. } } => u64::from(h264_keyint(self.fps)),
            Codec::H264 { rate: H264Rate::Bitrate { .. } } => total,
            Codec::ProRes4444 { .. } => PRORES_SEGMENT_FRAMES,
        };
        let segments = total.div_ceil(frames_per_segment).max(1);
        let cores = cores.max(1);
        let running = usize::try_from(segments).map_or(cores, |s| s.min(cores)).max(1);
        SegmentPlan { frames_per_segment, segments, threads_each: cores.checked_div(running).unwrap_or(1).max(1) }
    }
}

/// Encoded packets of a run of consecutive frames, ready for [`EncoderHandle::send_segment`](crate::EncoderHandle::send_segment).
#[derive(Debug)]
pub struct Segment {
    first_frame: u64,
    frames: u64,
    packets: Vec<Packet>,
}

impl Segment {
    /// Index of the first frame.
    #[must_use]
    pub const fn first_frame(&self) -> u64 {
        self.first_frame
    }

    /// Number of frames.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    pub(crate) fn into_parts(self) -> (u64, Vec<Packet>) {
        (self.frames, self.packets)
    }
}

/// Encodes segments one after another on the calling thread. Make one per worker thread.
pub struct SegmentEncoder {
    settings: VideoSettings,
    threads: usize,
    current: Option<Current>,
}

struct Current {
    encoder: VideoEncoder,
    first_frame: u64,
    frames: u64,
    packets: Vec<Packet>,
}

impl std::fmt::Debug for SegmentEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentEncoder").field("threads", &self.threads).field("busy", &self.current.is_some()).finish_non_exhaustive()
    }
}

impl SegmentEncoder {
    /// An encoder for `settings`. `threads` is how many threads one H.264 picture may use (see
    /// [`SegmentPlan::threads_each`]); 1 encodes on the calling thread alone.
    ///
    /// # Errors
    /// [`VideoError::InvalidSettings`] for settings the codecs cannot take.
    pub fn new(settings: &VideoSettings, threads: usize) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings: settings.clone(), threads: threads.max(1), current: None })
    }

    /// Begins a segment whose first frame has the global index `first_frame`. A segment still being built is dropped.
    ///
    /// # Errors
    /// [`VideoError::Encode`] when the codec cannot be set up.
    pub fn start(&mut self, first_frame: u64) -> Result<()> {
        let encoder = VideoEncoder::new(&self.settings, self.threads)?;
        self.current = Some(Current { encoder, first_frame, frames: 0, packets: Vec::new() });
        Ok(())
    }

    /// Encodes the next frame of the segment.
    ///
    /// # Errors
    /// [`VideoError::WrongFrameSize`] / [`VideoError::InvalidFrame`] for a bad image, [`VideoError::Encode`] when the
    /// codec fails, [`VideoError::EncoderStopped`] when no segment was started.
    pub fn push(&mut self, image: &Image) -> Result<()> {
        crate::encoder::check_frame(self.settings.width, self.settings.height, image)?;
        let fps = self.settings.fps;
        let current = self.current.as_mut().ok_or(VideoError::EncoderStopped)?;
        let index = current.first_frame.saturating_add(current.frames);
        let packets = current.encoder.encode(image, index, fps)?;
        current.packets.extend(packets);
        current.frames = current.frames.saturating_add(1);
        Ok(())
    }

    /// Flushes the segment's encoder and returns the packets.
    ///
    /// # Errors
    /// [`VideoError::NoFrames`] when no frame was pushed, [`VideoError::EncoderStopped`] when no segment was started.
    pub fn finish(&mut self) -> Result<Segment> {
        let Current { mut encoder, first_frame, frames, mut packets } = self.current.take().ok_or(VideoError::EncoderStopped)?;
        if frames == 0 {
            return Err(VideoError::NoFrames);
        }
        packets.extend(encoder.flush());
        Ok(Segment { first_frame, frames, packets })
    }
}
