# narrow-width

DejaVu Sans 24 px at widths 1, 0, -5 (all clamped to the 1 px minimum). P0 (RTL auto): `אבג 12 שָׁם`. P1 (LTR):
`ab لا é 😀 cd` (é written as e + U+0301).

Prediction: a box narrower than any cluster forces an emergency break after every grapheme cluster ("overflow-wrap:
anywhere"), but never inside a cluster, and spaces hang on the line before them (no line consists only of a space).

1. P1. No line range starts or ends inside a grapheme cluster: `שָׁ` (shin + qamats + shin-dot) stays whole, `لا` (a
   ligature) stays whole, `e` + U+0301 stays whole, the emoji stays whole.
2. P2. Line counts: P0 has clusters א ב ג (3) + 1 2 (2) + שָׁ ם (2) = 7 lines; P1 has a b (2) + لا (1) + é (1) +
   😀 (1) + c d (2) = 7 lines. 14 lines in all.
3. P3. Lines have strictly increasing y with a constant pitch (a line is never empty), and the line ranges of each
   paragraph concatenate to the full paragraph (no cluster dropped or duplicated).
4. P4. Widths 1.0, 0.0, -5.0, NaN and +inf give finite geometry and no panic (NaN/inf handled like "no sensible width":
   no panic, finite numbers).
5. P5. P0 is RTL and start-aligned: every line's glyph ink starts at the right edge of the 1 px box: ink right edge
   == 1 (+-0.5) and the ink extends leftwards beyond x = 0 (overflow to the left, x0 < 0).

Verdicts (narrow-width.png, which is drawn including the ink that overflows the 1 px box to the left):
- BUG found and fixed: without an overflow-wrap setting a box narrower than the word left every word whole on one line.
  Observed: 3 lines for P0 (words `אבג`, `12`, `שָׁם` unbroken), predicted 7. Fix: the engine sets
  `OverflowWrap::BreakWord` on every run, so an over-long word is broken between grapheme clusters.
- P1 to P5 now confirmed: 7 + 7 lines, `שָׁ` (shin + qamats + shin dot), `لا`, `e` + acute and the emoji never cut,
  constant line pitch, ranges concatenate, finite geometry for widths 0, -5, NaN and infinity, RTL lines are
  right-aligned at x = 1 and overflow leftwards.
