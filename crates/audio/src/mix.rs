//! Mixing several placed sounds into one buffer, with gain and pan.

use anim_core::Tick;

use crate::buffer::{MAX_CHANNELS, MAX_SAMPLE_RATE, PcmBuffer, tick_to_frame};
use crate::error::{AudioError, Result};
use crate::resample::resample;

/// Most samples a mix may produce (all channels).
const MAX_MIX_SAMPLES: usize = 1 << 30;

/// One sound placed on the timeline.
#[derive(Clone, Copy, Debug)]
pub struct MixInput<'a> {
    /// The audio.
    pub buffer: &'a PcmBuffer,
    /// When the first frame sounds. Negative starts cut off the beginning of the sound.
    pub start: Tick,
    /// Linear gain (1.0 = unchanged).
    pub gain: f32,
    /// -1.0 (left) to 1.0 (right), 0.0 = centre. Mono sources use a constant-power law, stereo sources a balance.
    pub pan: f32,
}

impl<'a> MixInput<'a> {
    /// A centred sound at unit gain starting at `start`.
    #[must_use]
    pub const fn at(buffer: &'a PcmBuffer, start: Tick) -> Self {
        Self { buffer, start, gain: 1.0, pan: 0.0 }
    }
}

/// An input converted to stereo at the output rate.
struct Prepared {
    stereo: PcmBuffer,
    /// Frame in the sound where playback starts (cut off by a negative `start`).
    skip: usize,
    /// Frame in the output where the sound lands.
    at: usize,
    gain_l: f32,
    gain_r: f32,
}

/// Output format of a [`mix`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixFormat {
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels. Mono receives the average of left and right; more than two get silence beyond the first two.
    pub channels: u16,
}

impl MixFormat {
    /// 48 kHz stereo, the usual export format.
    pub const STEREO_48K: Self = Self { sample_rate: 48_000, channels: 2 };
}

/// Sums `inputs` into one buffer of format `format`.
///
/// Every input is resampled to the output rate first. `length` fixes the result's duration; `None` runs to the end of
/// the last sound. Samples are summed without clipping, so loud mixes can exceed `1.0`: players and encoders clamp.
///
/// # Errors
/// [`AudioError::InvalidFormat`] for a bad output format or an input with an unusable format, and
/// [`AudioError::OutputTooLarge`] for a mix beyond about a billion samples.
pub fn mix(inputs: &[MixInput<'_>], format: MixFormat, length: Option<Tick>) -> Result<PcmBuffer> {
    if format.sample_rate == 0 || format.sample_rate > MAX_SAMPLE_RATE || format.channels == 0 || format.channels > MAX_CHANNELS {
        return Err(AudioError::InvalidFormat(format!("{} Hz, {} channels", format.sample_rate, format.channels)));
    }
    // Prepare every input as stereo at the output rate, remembering its first output frame.
    let mut prepared = Vec::with_capacity(inputs.len());
    let mut end = 0_usize;
    for input in inputs {
        if !input.buffer.is_valid_format() {
            return Err(AudioError::InvalidFormat(format!("input at {} Hz, {} channels", input.buffer.sample_rate, input.buffer.channels)));
        }
        let was_mono = input.buffer.channels == 1;
        let at_rate = resample(input.buffer, format.sample_rate)?;
        let stereo = at_rate.remix(2);
        let pan = if input.pan.is_finite() { input.pan.clamp(-1.0, 1.0) } else { 0.0 };
        let gain = if input.gain.is_finite() { input.gain } else { 0.0 };
        let (gl, gr) = if was_mono {
            let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            // Centre pan is -3 dB per side, so a mono sound keeps its loudness on a stereo bus.
            (angle.cos() * std::f32::consts::SQRT_2, angle.sin() * std::f32::consts::SQRT_2)
        } else {
            ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
        };
        let (skip, at) = if input.start.0 < 0 {
            (tick_to_frame(Tick(input.start.0.saturating_neg()), format.sample_rate), 0)
        } else {
            (0, tick_to_frame(input.start, format.sample_rate))
        };
        end = end.max(at.saturating_add(stereo.frames().saturating_sub(skip)));
        prepared.push(Prepared { stereo, skip, at, gain_l: gain * gl, gain_r: gain * gr });
    }
    if let Some(len) = length {
        end = tick_to_frame(len, format.sample_rate);
    }
    if end.saturating_mul(2) > MAX_MIX_SAMPLES {
        return Err(AudioError::OutputTooLarge { limit: MAX_MIX_SAMPLES });
    }
    let mut bus = vec![0.0_f32; end.saturating_mul(2)];
    for p in &prepared {
        let src = p.stereo.samples.get(p.skip.saturating_mul(2)..).unwrap_or_default();
        let dst = bus.get_mut(p.at.saturating_mul(2)..).unwrap_or_default();
        for ([l, r], [sl, sr]) in dst.as_chunks_mut::<2>().0.iter_mut().zip(src.as_chunks::<2>().0) {
            *l = sl.mul_add(p.gain_l, *l);
            *r = sr.mul_add(p.gain_r, *r);
        }
    }
    let stereo = PcmBuffer::new(format.sample_rate, 2, bus);
    Ok(stereo.remix(format.channels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::testutil::sine;

    #[test]
    fn places_sounds_at_their_start() {
        let a = PcmBuffer::new(1000, 1, vec![1.0; 100]);
        let b = PcmBuffer::new(1000, 1, vec![0.5; 100]);
        let out =
            mix(&[MixInput::at(&a, Tick::ZERO), MixInput::at(&b, Tick::from_units(50, 1000))], MixFormat { sample_rate: 1000, channels: 1 }, None).unwrap();
        assert_eq!(out.frames(), 150);
        let near = |i: usize, v: f32| (out.samples[i] - v).abs() < 1e-5;
        assert!(near(10, 1.0) && near(60, 1.5) && near(120, 0.5), "{:?}", [out.samples[10], out.samples[60], out.samples[120]]);
    }

    #[test]
    fn fixed_length_pads_and_truncates() {
        let a = PcmBuffer::new(1000, 1, vec![1.0; 100]);
        let longer = mix(&[MixInput::at(&a, Tick::ZERO)], MixFormat::STEREO_48K, Some(Tick::from_units(2, 1))).unwrap();
        assert_eq!(longer.frames(), 96_000);
        let shorter = mix(&[MixInput::at(&a, Tick::ZERO)], MixFormat { sample_rate: 1000, channels: 2 }, Some(Tick::from_units(10, 1000))).unwrap();
        assert_eq!(shorter.frames(), 10);
        assert_eq!(mix(&[], MixFormat::STEREO_48K, None).unwrap().frames(), 0);
    }

    #[test]
    fn negative_start_skips_the_head() {
        let ramp = PcmBuffer::new(1000, 1, (0..100).map(|i| i as f32 / 100.0).collect());
        let out = mix(&[MixInput::at(&ramp, Tick::from_units(-40, 1000))], MixFormat { sample_rate: 1000, channels: 1 }, None).unwrap();
        assert_eq!(out.frames(), 60);
        assert!((out.samples[0] - 0.40).abs() < 1e-5);
    }

    #[test]
    fn gain_and_pan() {
        let a = PcmBuffer::new(1000, 1, vec![1.0; 10]);
        let fmt = MixFormat { sample_rate: 1000, channels: 2 };
        let centre = mix(&[MixInput::at(&a, Tick::ZERO)], fmt, None).unwrap();
        assert!((centre.samples[0] - 1.0).abs() < 1e-5 && (centre.samples[1] - 1.0).abs() < 1e-5);
        let left = mix(&[MixInput { pan: -1.0, gain: 0.5, ..MixInput::at(&a, Tick::ZERO) }], fmt, None).unwrap();
        assert!((left.samples[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        assert!(left.samples[1].abs() < 1e-5);
        let st = PcmBuffer::new(1000, 2, vec![1.0; 20]);
        let bal = mix(&[MixInput { pan: 1.0, ..MixInput::at(&st, Tick::ZERO) }], fmt, None).unwrap();
        assert!(bal.samples[0].abs() < 1e-5 && (bal.samples[1] - 1.0).abs() < 1e-5);
        let nan = mix(&[MixInput { pan: f32::NAN, gain: f32::NAN, ..MixInput::at(&a, Tick::ZERO) }], fmt, None).unwrap();
        assert!(nan.samples.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn resamples_inputs_to_the_output_rate() {
        let a = sine(22_050, 1, 22_050, 440.0, 0.5);
        let out = mix(&[MixInput::at(&a, Tick::ZERO)], MixFormat::STEREO_48K, None).unwrap();
        assert_eq!(out.frames(), 48_000);
    }

    #[test]
    fn rejects_bad_formats() {
        let a = PcmBuffer::new(1000, 1, vec![1.0; 10]);
        assert!(mix(&[], MixFormat { sample_rate: 0, channels: 2 }, None).is_err());
        let bad = PcmBuffer::new(0, 1, vec![1.0; 10]);
        assert!(mix(&[MixInput::at(&bad, Tick::ZERO)], MixFormat::STEREO_48K, None).is_err());
        let huge = mix(&[MixInput::at(&a, Tick::MAX)], MixFormat::STEREO_48K, None);
        assert!(matches!(huge, Err(AudioError::OutputTooLarge { .. })));
    }
}
