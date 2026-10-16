# niqqud-carets

P0: `שָׁלוֹם עוֹלָם` (RTL, 44 px, DejaVu Sans) = shin + qamats + shin-dot, lamed, vav + holam, final mem, space, ayin,
vav + holam, lamed + qamats, final mem. Grapheme clusters: 4 + 1 + 4 = 9.
P1: `é ạ̈ x` (LTR) with e+U+0301, a+U+0308+U+0323: clusters é, space, ạ̈, space, x = 5.

1. P1. Pressing Left from the RTL start (offset 0) nine times reaches the end of paragraph 0 (offset = its text length)
   and each intermediate caret offset is a grapheme-cluster boundary (never in the middle of base + marks). The visited
   caret x values strictly increase leftwards-decrease: each press moves the caret left (x decreases) by the width of one
   cluster, more than 4 px.
2. P2. Same for P1 (LTR): Right pressed 5 times from the start reaches the end, offsets are only cluster boundaries,
   x strictly increases.
3. P3. hit_test at any x inside a cluster never returns an offset inside the cluster: for every probe x in a fine sweep
   across the line, the returned offset is a cluster boundary.
4. P4. The mark glyphs have zero (or near zero) advance: the line width of `שָׁלוֹם` (first word) equals the sum of the
   advances of its 4 base letters: the width of the word without marks (ש ל ו ם, laid out alone) +-0.5 px.
5. P5. Selection of the first word (offsets 0..14 bytes) is exactly one rectangle whose width equals that word width.
6. P6. Mark placement (visual): each mark glyph sits inside its base letter's horizontal extent (+-6 px) and the
   vav's holam is above the vav (y above the baseline minus the x-height), the shin-dot above the shin's top-right,
   the qamats under the shin. (Inspect the zoomed crop.)

Verdicts (niqqud-carets.png, crops niqqud-carets-shin-x8.png and niqqud-carets-latin-x4.png):
- P1 to P5 confirmed: nine caret steps in P0, five in P1, offsets only at cluster boundaries (shin + qamats + shin dot
  is one step), hit tests never land inside a cluster, the marks add no width, one selection rectangle for the word.
- P6 confirmed in the crops: shin dot above the right arm of the shin, qamats a bar under it, holam above the vav,
  acute above the e, diaeresis above and dot below the a.
- Fixture fix (the fixture, not the engine): the first version of the JSON had the Latin text written precomposed
  (U+00E9 and U+1EA1 U+0308), so the cluster count was 6, not 8 characters. The fixture now spells the decomposed
  sequences with JSON escapes (`é ạ̈ x`).
