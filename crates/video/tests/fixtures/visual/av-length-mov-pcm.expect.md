# av-length-mov-pcm

How the exporters reconcile audio and video length, and PCM in a MOV.
Cases (32x32 solid-grey video, 24 fps, 440 Hz amplitude 0.5 sine audio at 48 kHz mono unless stated):
(a) MP4, 12 frames (0.5 s) with 2.0 s of audio: truncated to the video duration.
(b) MP4, 24 frames (1.0 s) with only 0.25 s of audio.
(c) MOV ProRes 4444 (alpha false), 24 frames with 1.0 s of 16-bit PCM audio, stereo 440 Hz left, 880 Hz right, 48 kHz.
(d) MOV, 7 frames at 29.97 fps (0.2336 s) with audio 0.5 s: truncated.
(e) MOV with 24 frames, audio rate 44.1 kHz mono 1.0 s.

## Predictions (ffprobe + ffmpeg decode to f32)

1. (a) audio stream duration 0.500 s (+-0.03 s); decoded audio between 0.47 and 0.53 s of samples; the video is 12 frames; the container duration is 0.5 s (+-0.03).
2. (b) audio stream duration 0.25 s (+-0.03 s) and the video is still 24 frames (1.0 s); the video is NOT shortened or stretched, and the tone has the right frequency.
   The tone is not time-stretched: its frequency is 440 Hz +-0.5 percent.
3. (c) audio codec pcm_s16le, 2 channels, 48000 Hz; decoded sample count is exactly 48000 (1.000 s) per channel; left 440 Hz and right 880 Hz (+-0.2 percent), RMS 0.354
   (+-3 percent: PCM is exact); the first sample of the left channel is 0 and sample 1 is sin(2 pi 440 / 48000) * 0.5 = 0.0287 (+-0.002), i.e. the first sample is not dropped or shifted.
4. (d) sample count = floor(7 * 1001 / 30000 * 48000) = 11211 frames (exactly) per channel; duration ffprobe 0.2336 s (+-0.003).
5. (e) 44100 Hz mono PCM, 44100 samples, 440 Hz.
6. In all MOV cases the video track duration is frames / fps and `nb_frames` is the frame count, and the audio does not delay the first video frame (|video start_time| < 0.001).

## Verdicts

All of (a) to (e) hold at the first run: long audio is cut to 0.5 s, short audio stays 0.25 s without stretching or shortening the 24 frame video, the stereo
PCM MOV has exactly 48000 samples per channel with 440 Hz left and 880 Hz right and an unshifted first sample, the 29.97 fps MOV holds exactly 11211 samples, the 44.1 kHz
case is exact, and the video track never starts late. No bug.
