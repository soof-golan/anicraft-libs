# empty-lines-spaces

Six paragraphs at width 400, DejaVu Sans. P0: `abc   ` right aligned 30 px. P1: empty, 60 px. P2: empty, RTL, 30 px.
P3: three spaces, 30 px. P4: `x\n\ny` (forced breaks, 30 px, three lines, middle empty). P5: `end`.

1. P1. Empty paragraphs still make a line: total number of lines = 1 + 1 + 1 + 1 + 3 + 1 = 8.
2. P2. The empty 60 px paragraph's line is taller than the 30 px lines: height ~= 2x (+-1.5 px relative) a 30 px line
   (line height scales with the run size), not the 16 px default.
3. P3. Caret at the empty LTR paragraph is at x = 0; in the empty RTL paragraph, at x = 400 (right edge, start side).
   Caret rectangles have height equal to the line height and distinct y per paragraph, increasing downward.
4. P4. P0 right aligned: the `c` ink ends at x = 400 (the trailing spaces hang outside, they don't shift the text left),
   OR (if trailing spaces count) the line's last advance edge is 400. The test records which one; the prediction is:
   trailing spaces hang, `c` right edge at 400 +-1 (hanging whitespace convention).
   The caret after the final space (offset 6) is to the right of the c ink by 3 space widths: x ~= 400 + 3*space_adv
   is allowed only if hanging; otherwise ~= 400.
5. P5. P3 (only spaces) is one line; caret at its end is 3 space advances right of the caret at its start.
6. P6. P4 forced breaks: three lines at increasing y with equal spacing; the middle one empty with caret at x = 0 and
   the same height as the others. Caret at offset after the first `\n` is on line 2, not line 1.
7. P7. Moving the caret Down from the end of P0 goes to P1 (an empty line), then P2, ... each Down lands on the next line;
   after 7 Downs from P0 we are on `end` line (P5).
8. P8. Selection over the whole text has a rect on non-empty lines, and paragraph-break marker rects (narrow, 0.25
   of the line height) for the breaks of empty paragraphs.

Verdicts (empty-lines-spaces.png):
- P1, P2, P3, P5, P6, P7, P8 confirmed (8 lines; the empty 60 px line is twice as tall; carets at 0 and 400; Down
  visits each line once; empty paragraph breaks give a 0.25-line-height mark).
- P4 prediction was wrong, corrected here before the assertion was written: trailing spaces that FIT inside the box
  count for alignment (as in every text editor: you must see the caret after the space you typed). The line's last
  advance edge is at x = 400 and the `c` ends 3 space widths before it, caret at offset 6 is at 400. They hang outside
  the box only when they would overflow (see rtl-wrap and rtl-justify, where the wrapped-off space sits at x < 0).
