# alternating-vowels

Fast speech: Ah and Ee alternating directly (no silence), 8 segments of 100 ms (0.3 s of silence before, 0.4 s
after), then again with 60 ms segments, then three isolated 80 ms Ah syllables separated by 150 ms silence.
16 kHz, f0 120 Hz, rms 0.1. Failure mode: the two-frame confirmation and four-frame minimum hold swallow fast
alternations; vowel mixtures at the boundaries are read as a third vowel.

## Predictions

1. 100 ms segments: at 80 ms into each segment (the evidence is in by then: half a window plus confirmation is
   about 45 ms) the viseme is the segment's vowel, for at least 7 of the 8 segments.
2. Only Ah, Ee and Neutral appear anywhere in the 100 ms track (no Uh/Oh/WOo from the transitions): the transient
   frames are shorter than the confirmation window.
3. For both alternation speeds every pair of consecutive keys is at least 40 ms apart (the stated minimum hold),
   keys are strictly increasing, consecutive visemes differ.
4. The three 80 ms Ah syllables each produce an Ah key whose start lies in [t - 25 ms, t + 40 ms] (t = 0.2, 0.43,
   0.66 s from their silence-separated starts); the viseme between syllables (middle of each gap) is Neutral.

## Verdicts
All confirmed after the D fix: 100 ms segments switch exactly 10 ms after each boundary, 60 ms segments alternate
8 times at 0.3075 + 0.06 n, the three 80 ms syllables give Ah at 0.2075 / 0.4375 / 0.6675 with Neutral between.
