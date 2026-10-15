//! The encoder actor and the muxing it drives.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;

use anim_core::num::round_f64_to_i32;
use anim_core::{FrameRate, Tick};
use anim_display::Image;
use crossbeam_channel::{Receiver as ProgressReceiver, Sender as ProgressSender, unbounded};
use filmcraft_isobmff::{Brand, FourCc, Mp4Writer, PcmConfig, SampleEntry, TrackConfig, WriteSample, WriterOptions};

use crate::convert::{rgba_to_prores, rgba_to_yuv420};
use crate::{AudioTrack, Codec, H264Rate, Progress, Summary, VideoError, VideoSettings};

/// Frames that may wait for the encoder before `send_frame` blocks.
const QUEUE_DEPTH: usize = 4;
/// Samples per PCM chunk written to a MOV.
const PCM_CHUNK_FRAMES: usize = 4096;

type Result<T> = std::result::Result<T, VideoError>;

enum Command {
    Frame(Image),
    Finish,
    Cancel,
}

/// A running encode job. Dropping it without calling [`finish`](Self::finish) cancels the job and removes the file.
#[derive(Debug)]
pub struct EncoderHandle {
    commands: SyncSender<Command>,
    progress: ProgressReceiver<Progress>,
    thread: JoinHandle<Result<Summary>>,
    width: u32,
    height: u32,
    sent: u64,
}

/// Validates `settings` and starts the encoder thread, which creates `path` when the first frame arrives.
///
/// # Errors
/// [`VideoError::InvalidSettings`] for settings the codecs cannot take, [`VideoError::Io`] if the thread cannot start.
pub fn start(path: impl AsRef<Path>, settings: VideoSettings) -> Result<EncoderHandle> {
    settings.validate()?;
    let path = path.as_ref().to_path_buf();
    let (width, height) = (settings.width, settings.height);
    let (commands, rx) = sync_channel(QUEUE_DEPTH);
    let (progress_tx, progress) = unbounded();
    let thread = std::thread::Builder::new().name("anim-video-encoder".to_owned()).spawn(move || run(&path, settings, &rx, &progress_tx))?;
    Ok(EncoderHandle { commands, progress, thread, width, height, sent: 0 })
}

/// Encodes every frame of `frames` to `path` and returns the summary. A convenience over [`start`].
///
/// # Errors
/// Anything [`start`], [`EncoderHandle::send_frame`] or [`EncoderHandle::finish`] can return.
pub fn encode_to_file(path: impl AsRef<Path>, settings: VideoSettings, frames: impl IntoIterator<Item = Image>) -> Result<Summary> {
    let mut job = start(path, settings)?;
    for frame in frames {
        if job.send_frame(frame).is_err() {
            break;
        }
    }
    job.finish()
}

impl EncoderHandle {
    /// Queues a frame, blocking while the queue is full.
    ///
    /// # Errors
    /// [`VideoError::WrongFrameSize`] / [`VideoError::InvalidFrame`] for a bad image (nothing is queued), and
    /// [`VideoError::EncoderStopped`] if the thread died; [`finish`](Self::finish) then returns the real cause.
    pub fn send_frame(&mut self, frame: Image) -> Result<()> {
        if (frame.width, frame.height) != (self.width, self.height) {
            return Err(VideoError::WrongFrameSize { width: self.width, height: self.height, got_width: frame.width, got_height: frame.height });
        }
        if Image::byte_len(frame.width, frame.height) != Some(frame.premultiplied_rgba.len()) {
            return Err(VideoError::InvalidFrame(format!("{} bytes do not match {}x{} RGBA", frame.premultiplied_rgba.len(), frame.width, frame.height)));
        }
        self.commands.send(Command::Frame(frame)).map_err(|_| VideoError::EncoderStopped)?;
        self.sent = self.sent.saturating_add(1);
        Ok(())
    }

    /// Frames accepted by [`send_frame`](Self::send_frame) so far.
    #[must_use]
    pub const fn frames_sent(&self) -> u64 {
        self.sent
    }

    /// Progress events. Receive from this on the UI thread; the channel closes when the job ends.
    #[must_use]
    pub const fn progress(&self) -> &ProgressReceiver<Progress> {
        &self.progress
    }

    /// Flushes the encoders, writes the container and waits for the thread.
    ///
    /// # Errors
    /// The first error the thread hit, or [`VideoError::NoFrames`] if no frame was sent.
    pub fn finish(self) -> Result<Summary> {
        // A send failure means the thread already ended; its result carries the reason.
        let _ = self.commands.send(Command::Finish);
        self.thread.join().unwrap_or(Err(VideoError::EncoderStopped))
    }

    /// Stops the job and removes the partial file.
    pub fn cancel(self) {
        let _ = self.commands.send(Command::Cancel);
        let _ = self.thread.join();
    }
}

fn run(path: &Path, settings: VideoSettings, commands: &Receiver<Command>, progress: &ProgressSender<Progress>) -> Result<Summary> {
    let result = drive(path, settings, commands, progress);
    match &result {
        Ok(summary) => {
            let _ = progress.send(Progress::Done(summary.clone()));
        }
        Err(VideoError::Cancelled) => {
            let _ = std::fs::remove_file(path);
            let _ = progress.send(Progress::Cancelled);
        }
        Err(e) => {
            let _ = std::fs::remove_file(path);
            let _ = progress.send(Progress::Failed(e.to_string()));
        }
    }
    result
}

fn drive(path: &Path, settings: VideoSettings, commands: &Receiver<Command>, progress: &ProgressSender<Progress>) -> Result<Summary> {
    let mut job = Job::new(path, settings)?;
    loop {
        match commands.recv() {
            Ok(Command::Frame(image)) => {
                job.push(&image)?;
                let _ = progress.send(Progress::Encoded { frames: job.frames });
            }
            Ok(Command::Finish) => return job.finish(),
            // An explicit cancel, or the handle was dropped.
            Ok(Command::Cancel) | Err(_) => return Err(VideoError::Cancelled),
        }
    }
}

fn enc_err(e: impl std::fmt::Display) -> VideoError {
    VideoError::Encode(e.to_string())
}

fn mux_err(e: impl std::fmt::Display) -> VideoError {
    VideoError::Mux(e.to_string())
}

/// One encoded video sample.
struct Packet {
    data: Vec<u8>,
    key: bool,
    /// pts minus dts, in the track timescale.
    cto: i32,
}

enum VideoEncoder {
    H264 { enc: Box<filmcraft_h264enc::Encoder>, y: Vec<u8>, u: Vec<u8>, v: Vec<u8> },
    ProRes { enc: filmcraft_prores::Encoder, profile: filmcraft_prores::Profile, alpha: bool },
}

impl VideoEncoder {
    fn new(settings: &VideoSettings) -> Result<Self> {
        let (w, h) = (settings.width, settings.height);
        match settings.codec {
            Codec::H264 { rate } => {
                let num = u32::try_from(settings.fps.num).map_err(enc_err)?;
                let den = u32::try_from(settings.fps.den).map_err(enc_err)?;
                let mut cfg = filmcraft_h264enc::EncoderConfig::new(w, h, num, den);
                cfg.format = filmcraft_h264enc::PacketFormat::LengthPrefixed;
                cfg.aud = false;
                // No B-frames: presentation order equals decode order, so no composition offsets or edit lists.
                cfg.bframes = 0;
                cfg.keyint = num.saturating_mul(2).checked_div(den).unwrap_or(1).max(1);
                cfg.rate = match rate {
                    H264Rate::Bitrate { kbps } => filmcraft_h264enc::RateControl::Vbr { target_kbps: kbps, max_kbps: kbps.saturating_add(kbps / 2) },
                    H264Rate::Quality { crf } => filmcraft_h264enc::RateControl::Crf(crf),
                };
                let enc = filmcraft_h264enc::Encoder::new(cfg).map_err(enc_err)?;
                Ok(Self::H264 { enc: Box::new(enc), y: Vec::new(), u: Vec::new(), v: Vec::new() })
            }
            Codec::ProRes4444 { alpha } => {
                let profile = filmcraft_prores::Profile::P4444;
                let mut cfg = filmcraft_prores::EncoderConfig::new(profile, w, h);
                cfg.encode_alpha = alpha;
                Ok(Self::ProRes { enc: filmcraft_prores::Encoder::with_config(cfg), profile, alpha })
            }
        }
    }

    fn encode(&mut self, image: &Image, index: u64, fps: FrameRate) -> Result<Vec<Packet>> {
        let (px, _) = image.premultiplied_rgba.as_chunks::<4>();
        let width = anim_core::num::u32_to_usize(image.width);
        match self {
            Self::H264 { enc, y, u, v } => {
                rgba_to_yuv420(px, width, y, u, v);
                let frame = filmcraft_h264enc::YuvFrame { y, u, v, y_stride: width, uv_stride: width.div_ceil(2) };
                let pts = i64::try_from(index).ok().and_then(|i| i.checked_mul(fps.den)).ok_or_else(|| enc_err("timestamp overflow"))?;
                let packets = enc.try_encode(&frame, pts).map_err(enc_err)?;
                Ok(packets.into_iter().map(h264_packet).collect())
            }
            Self::ProRes { enc, alpha, .. } => {
                let mut frame = filmcraft_prores::Frame::new(image.width, image.height, filmcraft_prores::ChromaFormat::Yuv444, 10, *alpha);
                rgba_to_prores(px, width, *alpha, true, &mut frame);
                let data = enc.encode(&frame).map_err(enc_err)?;
                Ok(vec![Packet { data, key: true, cto: 0 }])
            }
        }
    }

    fn flush(&mut self) -> Vec<Packet> {
        match self {
            Self::H264 { enc, .. } => enc.flush().into_iter().map(h264_packet).collect(),
            Self::ProRes { .. } => Vec::new(),
        }
    }

    fn sample_entry(&self, width: u32, height: u32) -> Result<SampleEntry> {
        let w = u16::try_from(width).map_err(enc_err)?;
        let h = u16::try_from(height).map_err(enc_err)?;
        match self {
            Self::H264 { enc, .. } => {
                let cfg = filmcraft_isobmff::AvcConfig::parse(&enc.avcc()).or_else(|_| {
                    let (sps, pps) = enc.sps_pps();
                    Ok::<_, filmcraft_isobmff::Error>(filmcraft_isobmff::AvcConfig::new(vec![sps], vec![pps], 4))
                });
                Ok(SampleEntry::avc(cfg.map_err(mux_err)?, w, h))
            }
            Self::ProRes { profile, .. } => Ok(SampleEntry::prores(FourCc(profile.fourcc()), w, h)),
        }
    }
}

fn h264_packet(p: filmcraft_h264enc::Packet) -> Packet {
    Packet { data: p.data, key: p.keyframe, cto: i32::try_from(p.pts.saturating_sub(p.dts)).unwrap_or(0) }
}

enum AudioEncoder {
    Aac(Box<filmcraft_aac::Encoder>),
    Pcm,
}

/// The audio side of the muxer: the soundtrack, the next sample frame to write and the track it goes to.
struct AudioMux {
    track: AudioTrack,
    encoder: AudioEncoder,
    mux_track: usize,
    /// Sample frames already written.
    cursor: usize,
}

struct Muxer {
    mux: Mp4Writer<File>,
    video_track: usize,
    audio: Option<AudioMux>,
    frame_duration: u32,
}

struct Job {
    path: PathBuf,
    settings: VideoSettings,
    encoder: VideoEncoder,
    muxer: Option<Muxer>,
    frames: u64,
}

impl Job {
    fn new(path: &Path, settings: VideoSettings) -> Result<Self> {
        let encoder = VideoEncoder::new(&settings)?;
        Ok(Self { path: path.to_path_buf(), settings, encoder, muxer: None, frames: 0 })
    }

    fn push(&mut self, image: &Image) -> Result<()> {
        let packets = self.encoder.encode(image, self.frames, self.settings.fps)?;
        self.frames = self.frames.saturating_add(1);
        // The encoders finalise their parameter sets on the first frame, so the tracks are created after it.
        if self.muxer.is_none() {
            self.muxer = Some(self.open_muxer()?);
        }
        let frames = self.frames;
        let fps = self.settings.fps;
        let Some(muxer) = self.muxer.as_mut() else { return Err(VideoError::EncoderStopped) };
        muxer.write_packets(packets)?;
        muxer.write_audio_until(audio_frames_at(frames, fps, muxer.audio.as_ref().map_or(1, |a| a.track.sample_rate)))
    }

    fn open_muxer(&self) -> Result<Muxer> {
        let s = &self.settings;
        let brand = match s.codec {
            Codec::H264 { .. } => Brand::Mp4,
            Codec::ProRes4444 { .. } => Brand::Mov,
        };
        // Read access is needed by `finish_faststart`, which moves the media data in place.
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&self.path)?;
        let mut mux = Mp4Writer::new(file, WriterOptions::new(brand)).map_err(mux_err)?;
        let timescale = u32::try_from(s.fps.num).map_err(mux_err)?;
        let frame_duration = u32::try_from(s.fps.den).map_err(mux_err)?;
        let video_track = mux.add_track(TrackConfig::new(self.encoder.sample_entry(s.width, s.height)?, timescale)).map_err(mux_err)?;
        let audio = s.audio.as_ref().map(|a| open_audio(&mut mux, brand, a)).transpose()?;
        Ok(Muxer { mux, video_track, audio, frame_duration })
    }

    fn finish(mut self) -> Result<Summary> {
        if self.frames == 0 {
            return Err(VideoError::NoFrames);
        }
        let tail = self.encoder.flush();
        let (frames, fps) = (self.frames, self.settings.fps);
        let Some(mut muxer) = self.muxer.take() else { return Err(VideoError::NoFrames) };
        muxer.write_packets(tail)?;
        let rate = muxer.audio.as_ref().map_or(1, |a| a.track.sample_rate);
        muxer.write_audio_until(audio_frames_at(frames, fps, rate))?;
        muxer.finish_audio()?;
        let mut file = muxer.mux.finish_faststart().map_err(mux_err)?;
        std::io::Write::flush(&mut file)?;
        drop(file);
        let bytes = std::fs::metadata(&self.path)?.len();
        let duration = Tick::from_units(i64::try_from(frames).map_err(mux_err)?.saturating_mul(fps.den), fps.num);
        Ok(Summary {
            path: self.path,
            frames,
            width: self.settings.width,
            height: self.settings.height,
            fps,
            duration,
            bytes,
            has_audio: self.settings.audio.is_some(),
        })
    }
}

/// Sample frames of audio that fit in `frames` video frames: `frames * den * rate / num`, rounded down.
fn audio_frames_at(frames: u64, fps: FrameRate, rate: u32) -> usize {
    let num = u128::try_from(fps.num).unwrap_or(1).max(1);
    let den = u128::try_from(fps.den).unwrap_or(1);
    let total = u128::from(frames).saturating_mul(den).saturating_mul(u128::from(rate)).checked_div(num).unwrap_or(0);
    usize::try_from(total).unwrap_or(usize::MAX)
}

fn open_audio(mux: &mut Mp4Writer<File>, brand: Brand, track: &AudioTrack) -> Result<AudioMux> {
    let channels = usize::from(track.channels);
    let (encoder, entry, media_start) = if brand == Brand::Mp4 {
        let bitrate = 160_000_u32.saturating_mul(u32::from(track.channels));
        let cfg = filmcraft_aac::EncoderConfig::cbr(track.sample_rate, channels, bitrate);
        let enc = filmcraft_aac::Encoder::new(cfg).map_err(enc_err)?;
        let entry = SampleEntry::aac(enc.audio_specific_config(), u32::from(track.channels), track.sample_rate);
        let priming = i64::from(enc.priming_samples());
        (AudioEncoder::Aac(Box::new(enc)), entry, Some(priming))
    } else {
        let pcm = PcmConfig {
            bits: 16,
            float: false,
            big_endian: false,
            signed: true,
            channels: u32::from(track.channels),
            sample_rate: f64::from(track.sample_rate),
        };
        (AudioEncoder::Pcm, SampleEntry::pcm(pcm), None)
    };
    let mut cfg = TrackConfig::new(entry, track.sample_rate);
    cfg.media_start = media_start;
    let mux_track = mux.add_track(cfg).map_err(mux_err)?;
    Ok(AudioMux { track: track.clone(), encoder, mux_track, cursor: 0 })
}

impl Muxer {
    fn write_packets(&mut self, packets: Vec<Packet>) -> Result<()> {
        for p in packets {
            let sample = WriteSample { data: &p.data, duration: self.frame_duration, composition_offset: p.cto, is_sync: p.key };
            self.mux.write_sample(self.video_track, sample).map_err(mux_err)?;
        }
        Ok(())
    }

    /// Writes soundtrack sample frames up to (not including) `target`, clamped to the soundtrack's length.
    fn write_audio_until(&mut self, target: usize) -> Result<()> {
        let Some(audio) = self.audio.as_mut() else { return Ok(()) };
        let end = target.min(audio.track.frames());
        let channels = usize::from(audio.track.channels);
        while audio.cursor < end {
            let take = end.saturating_sub(audio.cursor).min(PCM_CHUNK_FRAMES);
            let from = audio.cursor.saturating_mul(channels);
            let to = audio.cursor.saturating_add(take).saturating_mul(channels);
            let Some(chunk) = audio.track.samples.get(from..to) else { break };
            match &mut audio.encoder {
                AudioEncoder::Aac(enc) => {
                    let planar: Vec<Vec<f32>> = (0..channels).map(|c| chunk.iter().skip(c).step_by(channels).copied().collect()).collect();
                    let refs: Vec<&[f32]> = planar.iter().map(Vec::as_slice).collect();
                    for unit in enc.encode(&refs) {
                        write_aac_unit(&mut self.mux, audio.mux_track, &unit)?;
                    }
                }
                AudioEncoder::Pcm => {
                    let bytes: Vec<u8> = chunk.iter().flat_map(|&s| pcm16(s)).collect();
                    let sample = WriteSample { data: &bytes, duration: 0, composition_offset: 0, is_sync: true };
                    self.mux.write_sample(audio.mux_track, sample).map_err(mux_err)?;
                }
            }
            audio.cursor = audio.cursor.saturating_add(take);
        }
        Ok(())
    }

    fn finish_audio(&mut self) -> Result<()> {
        if let Some(AudioMux { encoder: AudioEncoder::Aac(enc), mux_track, .. }) = self.audio.as_mut() {
            for unit in enc.flush() {
                write_aac_unit(&mut self.mux, *mux_track, &unit)?;
            }
        }
        Ok(())
    }
}

fn write_aac_unit(mux: &mut Mp4Writer<File>, track: usize, unit: &[u8]) -> Result<()> {
    mux.write_sample(track, WriteSample { data: unit, duration: 1024, composition_offset: 0, is_sync: true }).map_err(mux_err)
}

/// One sample as little-endian signed 16-bit PCM (clamped, `NaN` is silence).
fn pcm16(s: f32) -> [u8; 2] {
    let scaled = round_f64_to_i32(f64::from(s.clamp(-1.0, 1.0)) * 32767.0);
    i16::try_from(scaled).unwrap_or(0).to_le_bytes()
}
