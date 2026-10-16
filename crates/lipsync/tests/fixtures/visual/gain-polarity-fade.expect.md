# gain-polarity-fade

One Ah (800/1300/2600 Hz, f0 120 Hz, 0.5 s) after 0.3 s of silence and before 0.4 s of silence, at several levels
and polarities: rms 0.01 (-40 dBFS), 0.1 (-20), 0.8 (-2), inverted 0.1, and 0.002 (-54 dBFS, under the fixed
-52 dBFS silence floor). Then a fade-in: the same vowel with its rms rising exponentially from 0.0005 to 0.2 over
2.0 s (0.0005 * 400^(t/2)). Failure mode: classification depending on loudness or sign, or the level tracking
(noise floor, peak decay) producing spurious visemes while the level creeps up.

## Predictions

1. Levels -40, -20, -2 dBFS and the inverted one all give `Neutral, Ah, Neutral` with the Ah start in
   [0.275, 0.34] s.
2. The -54 dBFS version stays one single Neutral key (below the floor it is silence).
3. The fade-in: the level passes -52 dBFS (rms 0.0025) at t = 2 * ln(5) / ln(400) = 0.537 s. The track is
   `Neutral, Ah, ...`: no viseme other than Neutral before 0.45 s, and the first non-Neutral key is Ah, starting
   in [0.45, 0.80] s. After it the track may only contain Ah (a single non-Neutral key) until the end of the sound.
4. (Weak because of the 12 dB noise margin on a noise floor of -120 dBFS: none.)

## Verdicts
Points 1-3 confirmed (Ah at 0.3075 for all four levels and the inverted one, -54 dBFS stays Neutral, fade-in gives
one Ah starting 0.5375 s, exactly where the level crosses -52 dBFS). The first run showed the leading D (fixed, see
vowel-chain). Prediction 4 was empty text and is void.
