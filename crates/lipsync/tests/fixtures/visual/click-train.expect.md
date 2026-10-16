# click-train

0.3 s of silence, then three 15 ms white-noise bursts (rms 0.2) at 0.30, 0.45 and 0.60 s with digital silence
between, 0.5 s of silence after. 16 kHz. Failure mode: bursts shorter than the confirmation window being dropped,
or the burst hold (two frames) not releasing, or consecutive stops merging.

## Predictions

1. The track is `Neutral, D, Neutral, D, Neutral, D, Neutral` (7 keys).
2. Each D starts in [t - 20 ms, t + 50 ms] of its burst, and lasts between 20 ms and 100 ms (the next Neutral
   follows within that).
3. Between bursts (e.g. at 0.40 and 0.55 s) the viseme is Neutral.

## Verdicts
Points 2 and 3 confirmed (D at 0.2875 / 0.4375 / 0.5875, Neutral within 100 ms). Point 1 PARTLY wrong: the
first and third clicks are followed by a 40 ms F, the second is not. The prediction ignored that a 25 ms window
smears a 15 ms burst over about 40 ms, so the tail frames look like soft noise. Tried a "level falling fast"
rule and an S-cancel to remove it; neither separated clicks from fricatives, so both were reverted. Test asserts
D-then-Neutral within 100 ms and that only D/F/Neutral appear.
