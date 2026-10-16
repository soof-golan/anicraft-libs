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

Verdicts: (filled in after inspection)
