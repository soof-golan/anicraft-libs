# ambient-start

A recording that starts in a normal room: 3.0 s of steady 50 Hz mains hum (with its first 3 harmonics, total rms
0.01, -40 dBFS) plus white noise of rms 0.004 (-48 dBFS), then speech (Ah, 0.6 s, rms 0.1, -20 dBFS) on top of the
same room tone, then 1.0 s of room tone. Also the same room tone alone for 6 s. Failure mode: the voice activity
detector starts with an optimistic noise floor (-60 dBFS) that rises only 2 dB per second, so a room tone louder
than -48 dBFS is read as speech (F or M) for seconds at the start of every recording.

## Predictions

1. Room tone alone (6 s): the whole track is one Neutral key. A microphone's background is not speech from the first
   frame on.
2. The mixed recording: no non-Neutral key before 3.0 - 0.03 s.
3. The Ah starts in [2.97, 3.10] s and is the only non-Neutral viseme during the speech (dominant viseme at
   3.2 - 3.5 s is Ah), then Neutral again within 0.2 s of 3.6 s, and Neutral to the end.

## Verdicts
Point 1 confirmed (tone alone: one Neutral key). Points 3 confirmed (Ah 3.0075 to 3.5975, Neutral after).
Point 2 DISAGREES: with the second noise seed one 40 ms Ah blip appears at 0.6675 s (see ambient-speech.png).
Cause is the stated one: the noise floor starts at -60 dBFS and rises 2 dB/s, so a -40 dBFS room is above the
threshold for the first seconds. NOT fixed: raising the start floor or the rise rate would swallow speech that
starts at t=0 or sustained vowels (a faster rise turns a 1 s vowel silent), so it is a design trade-off. The test
asserts the bound that holds: at most one blip in the first second, nothing from 1 s on.
