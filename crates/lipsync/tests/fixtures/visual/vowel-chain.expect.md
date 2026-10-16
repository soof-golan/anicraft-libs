# vowel-chain

Five synthetic vowels (glottal pulse train at a steady 120 Hz through a formant cascade) joined with a 5 ms
cross-fade and no silence between them: Ah (800/1300/2600 Hz), Ee (300/2250/3000), Oh (540/880/2500),
WOo (320/830/2500), Uh (620/1250/2500). 0.35 s each, preceded by 0.3 s and followed by 0.4 s of digital silence.
Rendered at 16 kHz and at 44.1 kHz. The failure mode: the incumbent-viseme hysteresis or the minimum-hold rule
swallowing or delaying a vowel that follows another one without a pause.

Boundaries: b0 = 0.30 (sound starts), b1 = 0.65, b2 = 1.00, b3 = 1.35, b4 = 1.70, end = 2.05 s.

## Predictions

1. The track is exactly `Neutral, Ah, Ee, Oh, WOo, Uh, Neutral` (7 keys, no extra visemes in between, at both rates).
2. The first key is at Tick 0 with Neutral; keys strictly increase in time; consecutive keys differ.
3. Ah starts in [b0 - 25 ms, b0 + 40 ms]. Each later vowel starts in [b - 30 ms, b + 60 ms] of its boundary.
4. The final Neutral starts in [end - 30 ms, end + 60 ms].
5. `viseme_at` at the middle of every vowel (b + 0.175 s) is that vowel.
6. 16 kHz and 44.1 kHz tracks agree on the sequence and every start within 20 ms.

## Verdicts
Points 2-6 confirmed in the PNG (starts 0.3075 / 0.6575 / 1.0075 / 1.3575 / 1.6975, Neutral 2.0675).
Point 1 FAILED first: a 20 ms `D` flashed before the first Ah. Root cause: the first, mostly empty analysis window
of an abrupt vowel is an unvoiced rise out of silence, which the stop-burst rule turned into D (held 2 frames).
Fixed in `engine.rs`: a burst is judged one frame late and dropped when the next frame is voiced. Same fix cleared
the D before Ah/Ee/WOo in the gain, pitch, alternating-vowel and 8 kHz fixtures.
