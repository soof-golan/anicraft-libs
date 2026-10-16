# sample-rate-sweep

The same utterance, synthesised natively at 8000, 11025, 16000, 22050, 32000, 44100, 48000 and 96000 Hz:
0.3 s silence, Ee (300/2250/3000) 0.3 s, Ah (800/1300/2600) 0.3 s, WOo (320/830/2500) 0.3 s, 0.4 s silence. For
rates of 16 kHz and above a 0.2 s sibilant (noise band at 6.5 kHz) comes first as well. Failure mode: the integer
box decimation (rate / 16 kHz, rounded down), the window/hop rounding at non-integer ratios (11025, 22050, 44100)
or the clamp at 8 kHz distort the labels or the time axis.

## Predictions

1. At every rate the vowels come out as `Ee, Ah, WOo` in this order, wrapped by Neutral at both ends; with the
   sibilant, `S, Ee, Ah, WOo`.
2. Every key time is within 25 ms of the 16 kHz reference run (the time axis does not drift with the rate: Tick
   maths is exact `samples / rate`).
3. The last key is Neutral and lies within [end - 30 ms, end + 60 ms], with end = 1.2 s (1.4 s with the sibilant).
4. No key lies before 0.3 - 0.03 s (nothing is invented in the leading silence).

## Verdicts
Confirmed in the stacked PNG: all eight strips line up with the prediction rows within a few pixels (1 px = 1.7 ms).
Before the fix the 8 kHz run had a leading D. Remaining limitation, accepted: at 22.05 and 44.1 kHz the window
phase gives the sibilant's first partial window a 20 ms D label before S (one frame cannot tell a click from a
fricative onset; deferring one more frame would drop real clicks, tried and rejected). The test removes a D
directly followed by S within 25 ms before comparing.
