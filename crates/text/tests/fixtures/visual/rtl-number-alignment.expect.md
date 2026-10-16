# rtl-number-alignment

Seven RTL paragraphs `יש לי 3 תפוחים` ("I have 3 apples": Hebrew, a number, then Hebrew that begins with a space in
logical order), DejaVu Sans 28 px, one line each. The ink of a line is [min run x0, max run x1] over its glyph runs; the
ink width A is the sum of the run advances and does not depend on alignment. Last paragraph has one extra trailing space.

At width 400:
1. P1. Start (right for RTL): ink x1 == 400 (+-0.3).
2. P2. End (left for RTL): ink x0 == 0 (+-0.3).
3. P3. Left: ink x0 == 0. P4. Right: ink x1 == 400.
4. P5. Center: the ink centre == 200 (+-0.3).
5. P6. Justify on a one-line paragraph is the last line: behaves like Start (ink x1 == 400, x0 = 400 - A).
6. P7. With a trailing space (hanging whitespace) Start: the ink x1 == 400 and the last ink glyph is not shifted by it.

Unwrapped (`None` width):
7. P8. The text box encloses all of the ink: layout.bounds.x0 <= every run x0 and every run x1 <= bounds.x1; the widest
   line's ink starts at x >= 0 (no ink at negative x, which would be clipped by the box).

Verdicts (rtl-number-alignment.png): P1 to P7 confirmed at width 400 (ink x1 = 400 for Start, Right, Justify and
the trailing-space case; x0 = 0 for End and Left; centre 200).
- P8 BUG found and fixed: in the unwrapped multi-paragraph box the right-to-left lines were aligned in a box 0.03 px
  wider than `bounds` (the widest paragraph was broken at `width * 1.0001 + 0.01` to dodge rounding, and the alignment
  used that width), so the run `0..10` ended at 188.37 while `bounds.x1` was 188.34 (ink outside the box). Fix: break
  at the exact container width and fall back to the slack only when that changes the line count.
