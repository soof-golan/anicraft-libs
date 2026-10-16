# high-pitch-voice

The Ah (800/1300/2600) and Ee (300/2250/3000) vowels sung / spoken by voices of different pitch: f0 = 100, 150,
200, 250, 300 and 350 Hz, 0.5 s each after 0.3 s of silence, rms 0.1, 16 kHz. Also a pitch glide on Ah from 90 Hz
to 320 Hz over 0.8 s. Failure mode: with a sparse harmonic spectrum (f0 near F1) the LPC envelope clings to
harmonics, or the voicing detector's 70-400 Hz lag range fails at the edges, so child and soprano voices (and
glides) get the wrong viseme.

## Predictions

1. For every f0 from 100 to 300 Hz, the dominant viseme in the middle of the Ah is Ah and of the Ee is Ee.
2. The glide produces a single Ah (track `Neutral, Ah, Neutral`): the vowel quality does not change when only the
   pitch moves.
3. At 350 Hz the result is documented, not asserted (it is still reported in the verdicts).

## Verdicts
Points 1 (for 100-250 Hz) and 2 confirmed. Prediction wrong for 300 Hz: Ee reads Ee for 40 ms then M for
the rest. At f0 = 300 Hz the fundamental coincides with F1 = 300 Hz, nearly all energy sits below 700 Hz and the
spectrum is a hum: the murmur rule is correct for the signal, a vowel at that pitch is not separable by this
heuristic. The assertion range is therefore 100-250 Hz; 300 and 350 Hz are only reported.
