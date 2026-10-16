# rtl-base-latin

DejaVu Sans 32 px, width 600. P0: `Hello World!` forced RTL. P1: `1) First - item (2)` forced RTL. P2: `שלום!` forced LTR.

1. P1. P0: Latin words keep reading left to right; the trailing `!` is a neutral at the end of an RTL paragraph, so it takes
   the base direction and is drawn on the LEFT: visual order `!Hello World`. The run `Hello World` is flush right
   (its right edge at x = 600 +-0.6) and `!` is left of it (x1 of `!` <= x0 of the Latin run + 0.5).
2. P2. Carets on P0: caret at offset 11 (between `d` and `!`): downstream (attached to `!`, RTL) is at the `!` glyph's right
   edge = the Latin run's left edge; upstream (attached to `d`) is at the Latin run's right edge = 600. Caret at the
   end (offset 12) downstream is at the line's left edge (left edge of `!`). Caret at offset 0 downstream (before H) is at
   the LEFT edge of the Latin run.
3. P3. P1 `1) First - item (2)`: the digit `1` is an LTR number (level 2); the `)` right after it is a neutral between a
   number and a Latin letter, so it takes the base direction (RTL) and is mirrored. The final logical character `)`
   is the left-most glyph of the line (nothing follows it), mirrored, so its glyph id equals the id of `(`. The
   first logical character `1` is NOT mirrored and is right of the `)` at offset 1 (x(`1`) > x(`)`)).
4. P4. P2 forced LTR `שלום!`: Hebrew word is RTL inside, `!` is drawn at the right end: x(`!`) >= right edge of the
   Hebrew run. Caret at offset 0 downstream (before ש, the first logical letter) is at the Hebrew run's right edge; the
   caret at the end (offset 9) downstream is at the line's right edge.

Verdicts (rtl-base-latin.png): P1, P2, P4 confirmed (`!Hello World` with the `!` on the left and carets as predicted;
`שלום!` forced LTR draws the `!` at the right end).
- P3 prediction was wrong, corrected here: I wrote that the final logical `)` is the leftmost glyph and mirrored.
  UAX #9 W7 turns the digit in `(2)` into L (it follows the Latin letters), so `First - item (2)` is one LTR run and
  its brackets are NOT mirrored; the `)` after the leading `1` is the one that takes the RTL base direction, is
  mirrored to `(` and is drawn directly left of the `1`. The picture reads `First - item (2) (1` from left to right,
  which is what the rules give; the engine matches that.
