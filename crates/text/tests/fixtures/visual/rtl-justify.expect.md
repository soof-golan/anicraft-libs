# rtl-justify

Justified Hebrew (direction forced RTL) with a phone number, a year and a decimal; DejaVu Sans 24 px, width 340.

1. P1. Every line except the last spans exactly the box: ink x0 == 0 and x1 == 340 (+-0.8).
2. P2. The last line is NOT stretched: it is flush right (x1 == 340 +-0.6) and its left edge is well inside the box.
3. P3. Justification adds space only at spaces (word gaps), never inside words or numbers: the `054-1234567` run, the
   `2024` run and the `3.14` run have exactly the advance they have in an unjustified, unwrapped layout (+-0.05 px) if
   they appear whole on a justified line.
4. P4. Gaps between words on one justified line are all equal to each other (+-0.1 px): the extra is distributed evenly
   over the spaces. The leftover width is 0 <= extra, so no word gap is narrower than the natural space width.
5. P5. The right edge is still where the first logical character is: the first glyph of every line begins at x = 340
   (its right edge).
6. P6. Selection rectangle of one whole justified line has width 340 (+-0.8), a single rect per run group: for a line
   with LTR numbers, rectangles tile the line with no overlap and no more than a 0.5 px gap, total width 340.

Verdicts: (filled in after inspection)
