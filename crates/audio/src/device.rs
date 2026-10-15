//! cpal backends for the playback and capture actors. Nothing here runs in tests (CI has no audio device); the
//! protocol around it is tested with fake backends.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

use crate::capture::{ChunkSink, InputBackend, InputFormat};
use crate::error::{AudioError, Result};
use crate::playback::{OutputBackend, OutputFormat, PlaybackEvent, Renderer};

fn device_err(e: impl std::fmt::Display) -> AudioError {
    AudioError::Device(e.to_string())
}

/// Names of the output devices of the default host.
#[must_use]
pub fn list_output_devices() -> Vec<String> {
    cpal::default_host().output_devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

/// Names of the input devices of the default host.
#[must_use]
pub fn list_input_devices() -> Vec<String> {
    cpal::default_host().input_devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

/// A callback period of about 10 ms, so the first sound is not delayed by a default buffer of hundreds of ms.
fn preferred_buffer_frames(rate: u32) -> u32 {
    (rate / 100).clamp(256, 2048)
}

fn frames_from_duration(d: std::time::Duration, rate: u32) -> usize {
    usize::try_from(anim_core::num::round_f64_to_i64(d.as_secs_f64() * f64::from(rate)).max(0)).unwrap_or(0)
}

/// The cpal output device.
pub struct CpalOutput {
    device: cpal::Device,
    config: StreamConfig,
    sample_format: SampleFormat,
    format: OutputFormat,
}

impl CpalOutput {
    /// Opens the default output device.
    ///
    /// # Errors
    /// [`AudioError::Device`] when there is none or its configuration cannot be read.
    pub fn open_default() -> Result<Self> {
        let device = cpal::default_host().default_output_device().ok_or_else(|| AudioError::Device("no default output device".into()))?;
        Self::from_device(device)
    }

    /// Opens the output device called `name` (one of [`list_output_devices`]).
    ///
    /// # Errors
    /// [`AudioError::Device`] when no such device exists.
    pub fn open_named(name: &str) -> Result<Self> {
        let device = cpal::default_host()
            .output_devices()
            .map_err(device_err)?
            .find(|d| d.name().is_ok_and(|n| n == name))
            .ok_or_else(|| AudioError::Device(format!("no output device called {name}")))?;
        Self::from_device(device)
    }

    fn from_device(device: cpal::Device) -> Result<Self> {
        let supported = device.default_output_config().map_err(device_err)?;
        let mut config: StreamConfig = supported.config();
        if cfg!(target_os = "linux") {
            let want = preferred_buffer_frames(config.sample_rate.0);
            config.buffer_size = match supported.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } if min <= max => cpal::BufferSize::Fixed(want.clamp(*min, *max)),
                _ => cpal::BufferSize::Default,
            };
        }
        let format = OutputFormat { sample_rate: config.sample_rate.0, channels: config.channels };
        Ok(Self { device, config, sample_format: supported.sample_format(), format })
    }

    fn build<T: SizedSample + FromSample<f32> + Send + 'static>(&self, mut renderer: Renderer, config: &StreamConfig) -> Result<Stream> {
        let rate = self.format.sample_rate;
        let events = renderer.event_sender();
        let mut scratch: Vec<f32> = Vec::new();
        self.device
            .build_output_stream(
                config,
                move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
                    scratch.resize(out.len(), 0.0);
                    let ts = info.timestamp();
                    let latency = ts.playback.duration_since(&ts.callback).map_or(0, |d| frames_from_duration(d, rate));
                    renderer.render(&mut scratch, latency);
                    for (o, s) in out.iter_mut().zip(&scratch) {
                        *o = T::from_sample_(s.clamp(-1.0, 1.0));
                    }
                },
                move |e| {
                    let _ = events.try_send(PlaybackEvent::Error(format!("audio output: {e}")));
                },
                None,
            )
            .map_err(device_err)
    }
}

impl OutputBackend for CpalOutput {
    type Stream = Stream;

    fn format(&self) -> OutputFormat {
        self.format
    }

    fn start(self, renderer: Renderer) -> Result<Stream> {
        // Some hosts reject a fixed period; retry with the default one before giving up. The renderer moves into the
        // first attempt, so the retry uses a fresh renderer on the same channels (nothing has played yet).
        let first = self.build_for(renderer.fresh(), &self.config);
        let stream = match first {
            Ok(s) => s,
            Err(_) if self.config.buffer_size != cpal::BufferSize::Default => {
                let mut config = self.config.clone();
                config.buffer_size = cpal::BufferSize::Default;
                self.build_for(renderer, &config)?
            }
            Err(e) => return Err(e),
        };
        stream.play().map_err(device_err)?;
        Ok(stream)
    }
}

impl CpalOutput {
    fn build_for(&self, renderer: Renderer, config: &StreamConfig) -> Result<Stream> {
        match self.sample_format {
            SampleFormat::F32 => self.build::<f32>(renderer, config),
            SampleFormat::I16 => self.build::<i16>(renderer, config),
            SampleFormat::U16 => self.build::<u16>(renderer, config),
            SampleFormat::I32 => self.build::<i32>(renderer, config),
            SampleFormat::U8 => self.build::<u8>(renderer, config),
            SampleFormat::I8 => self.build::<i8>(renderer, config),
            SampleFormat::F64 => self.build::<f64>(renderer, config),
            other => Err(AudioError::Device(format!("unsupported sample format {other:?}"))),
        }
    }
}

/// The cpal input device.
pub struct CpalInput {
    device: cpal::Device,
    config: StreamConfig,
    sample_format: SampleFormat,
    format: InputFormat,
}

impl CpalInput {
    /// Opens the default input device. Pass this function to [`crate::CaptureActor::spawn_with`] to record.
    ///
    /// # Errors
    /// [`AudioError::Device`] when there is no microphone or its configuration cannot be read.
    pub fn open_default() -> Result<Self> {
        let device = cpal::default_host().default_input_device().ok_or_else(|| AudioError::Device("no default input device".into()))?;
        Self::from_device(device)
    }

    /// Opens the input device called `name` (one of [`list_input_devices`]).
    ///
    /// # Errors
    /// [`AudioError::Device`] when no such device exists.
    pub fn open_named(name: &str) -> Result<Self> {
        let device = cpal::default_host()
            .input_devices()
            .map_err(device_err)?
            .find(|d| d.name().is_ok_and(|n| n == name))
            .ok_or_else(|| AudioError::Device(format!("no input device called {name}")))?;
        Self::from_device(device)
    }

    fn from_device(device: cpal::Device) -> Result<Self> {
        let supported = device.default_input_config().map_err(device_err)?;
        let config: StreamConfig = supported.config();
        let format = InputFormat { sample_rate: config.sample_rate.0, channels: config.channels };
        Ok(Self { device, config, sample_format: supported.sample_format(), format })
    }

    fn build<T: SizedSample + Send + 'static>(&self, mut sink: ChunkSink) -> Result<Stream>
    where
        f32: FromSample<T>,
    {
        let reporter = sink.reporter();
        let mut scratch: Vec<f32> = Vec::new();
        self.device
            .build_input_stream(
                &self.config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    scratch.clear();
                    scratch.extend(data.iter().map(|s| f32::from_sample_(*s)));
                    sink.push(&scratch);
                },
                move |e| reporter.report(format!("audio input: {e}")),
                None,
            )
            .map_err(device_err)
    }
}

impl InputBackend for CpalInput {
    type Stream = Stream;

    fn format(&self) -> InputFormat {
        self.format
    }

    fn start(self, sink: ChunkSink) -> Result<Stream> {
        let stream = match self.sample_format {
            SampleFormat::F32 => self.build::<f32>(sink),
            SampleFormat::I16 => self.build::<i16>(sink),
            SampleFormat::U16 => self.build::<u16>(sink),
            SampleFormat::I32 => self.build::<i32>(sink),
            SampleFormat::U8 => self.build::<u8>(sink),
            SampleFormat::I8 => self.build::<i8>(sink),
            SampleFormat::F64 => self.build::<f64>(sink),
            other => Err(AudioError::Device(format!("unsupported sample format {other:?}"))),
        }?;
        stream.play().map_err(device_err)?;
        Ok(stream)
    }
}
