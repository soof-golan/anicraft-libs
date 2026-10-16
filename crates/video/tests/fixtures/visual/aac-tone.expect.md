# aac-tone

MP4 (H.264 32x32, 24 fps, 24 frames = 1.000 s, solid grey) with AAC-LC audio. Cases:
(a) 48 kHz stereo, left channel a 1000 Hz sine amplitude 0.5, right channel a 3000 Hz sine amplitude 0.25 (different on purpose to catch swapped or mixed channels).
(b) 44.1 kHz mono 440 Hz amplitude 0.5. (c) 22.05 kHz mono 2000 Hz amplitude 0.5. (d) 8 kHz mono 1000 Hz amplitude 0.5.
All audio exactly 1.000 s long.

## Predictions (decoded with ffmpeg to float PCM at the original rate and channel count)

1. ffprobe: audio stream codec aac, profile LC, channels as given, sample_rate as given. Video stream and audio stream both start at 0 (|start_time| <= 0.03 s).
2. Frequency by positive zero-crossing count over 0.1 s .. 0.9 s: (a) left 1000 Hz +-0.5 percent, right 3000 Hz +-0.5 percent; (b) 440 Hz; (c) 2000 Hz; (d) 1000 Hz, each +-0.5 percent.
3. Spectral purity: the Goertzel power at the nominal frequency is more than 200 times the power at 0.8x and 1.25x that frequency, and the other channel's
   frequency is more than 100 times weaker than the channel's own in (a) (no crosstalk, no swap).
4. RMS amplitude over 0.1 .. 0.9 s: left 0.5/sqrt 2 = 0.354 (+-8 percent), right 0.177 (+-8 percent); mono cases 0.354 (+-8 percent).
5. Duration: the audio stream duration is 1.000 s (+-0.03 s), and the total decoded sample count is within 1100 frames of rate * 1 s.
6. Onset: the first sample with |x| > 0.1 occurs within 30 ms of the decoded start, i.e. the decoder's priming (1024 samples = 21 ms) is declared in the
   container (edit list or start offset) rather than left as a 21 ms silence-plus-pre-echo at the beginning; and the tail is not cut: the last 50 ms before the end still hold
   the tone (RMS > 0.25 on the left channel of (a)).
7. Clip free: no sample outside -1..1 after decode, and peak <= 0.6 for amplitude 0.5 input.

## Verdicts

Cases extended with 16 kHz mono, 32 kHz stereo and 96 kHz mono.

BUG 1 (fixed): every sample rate of 26.6 kHz and below failed. `open_audio` asked the AAC encoder for a fixed 160 kbps per channel, but an AAC frame holds at most
6144 bits per channel, i.e. 6 x sample rate per second per channel: 22.05 kHz allows 132 kbps, 8 kHz only 48 kbps. `VideoSettings::validate` accepts 8000..=192000 Hz, so the
encode failed on the worker thread with "bitrate out of range for the channel count and sample rate" after the file had been started. Observed for the 22.05 kHz
and 8 kHz cases (and 16 kHz, 24 kHz by the same arithmetic). Fixed by capping the per-channel bitrate at 5 x sample rate.

Everything else held: codec aac LC, channels and rate, start times 0, duration 1.000 s, frequency by zero crossings within 0.5 percent, spectrally pure, no crosstalk
or swap between the 1000 Hz left and the 3000 Hz right channel, RMS within 8 percent, onset within 30 ms (the priming is declared in the container), tail present.

Prediction 7 was too strict at the very end, and it exposed a real quality problem that is NOT in our crate: the pinned `filmcraft-aac` encoder overshoots just before
an abrupt digital stop. A 3 kHz tone of amplitude 0.5 that stops at the end of the stream decodes with peaks up to 1.76 (3.5x) in the last ~1500 samples, depending on where
the stop falls inside the 1024-sample frame; stopping in the middle of a stream gives up to 0.86; ffmpeg's own AAC encoder gives 1.003x for the same signal. Mid-tone
quality is excellent (error below 0.001), so this is limited to the last tens of ms before a hard stop (pre-echo with no block switching). Reported for the filmcraft
owner; `aac_abrupt_stop_overshoot` and `aac_overshoot_at_end_of_stream` only bound it (1.0 and 2.0) so it cannot get worse. The clip and peak checks in
`aac_tones` therefore look at 0.1 .. 0.9 s.
