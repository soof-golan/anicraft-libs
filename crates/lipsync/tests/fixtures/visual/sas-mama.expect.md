# sas-mama

Two consonant-vowel-consonant words with no gaps: "sas" = sibilant noise (6.5 kHz band, rms 0.08, 0.22 s), Ah
(0.3 s), sibilant (0.22 s); "mama" shape = a pure 140 Hz hum (rms 0.05, 0.25 s), Ah (0.3 s), hum (0.25 s). Each
preceded by 0.3 s and followed by 0.4 s silence, separated by 0.5 s. 16 kHz. Failure mode: a consonant adjacent to
a loud vowel inheriting the vowel's label through the incumbent bonus and the minimum hold.

Times: "sas": S at 0.30, Ah at 0.52, S at 0.82, end 1.04 s. Second word starts at 1.54: M at 1.54, Ah at 1.79,
M at 2.09, end 2.34.

## Predictions

1. Word 1 is `Neutral, S, Ah, S, Neutral`; word 2 is `Neutral, M, Ah, M, Neutral`; the track overall is these two
   sequences joined at the shared Neutral (9 keys: `N S Ah S N M Ah M N`).
2. Each consonant/vowel boundary key is within [boundary - 30 ms, boundary + 60 ms].
3. The viseme in the middle of every segment is the one named.

## Verdicts
All points confirmed: N S Ah S N M Ah M N at 0.2975, 0.5175, 0.8175, 1.0475, 1.5275, 1.7875, 2.0875, 2.3275.
