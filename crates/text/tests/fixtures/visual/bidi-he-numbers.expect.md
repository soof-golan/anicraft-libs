# bidi-he-numbers

Hebrew sentence (auto direction) with a phone number `054-1234567`, a decimal `3.14`, a hyphenated year `ב-2024`.
DejaVu Sans 28 px, laid out unwrapped (`None` width) and at width 900.

Prediction (UAX #9, base level 1 because the first strong character is Hebrew):

1. P1. Paragraph and line are RTL; the first character (offset 0) is the rightmost glyph. Caret at offset 0
   (downstream) is at the line's right edge x1 (+-0.5 px).
2. P2. `054-1234567` is one left-to-right run (EN ES EN stays together: ES between two ENs becomes EN). Its glyph run
   has `rtl == false`, its characters are in increasing x order (glyph x strictly increasing) and the run text range is
   exactly the 11 characters.
3. P3. `3.14` is one LTR run (CS between two ENs), `2024` is one LTR run, none split.
4. P4. The `-` in `ב-2024` is NOT part of the number (ES not between two ENs): it resolves to neutral, between R and EN,
   so it is RTL level 1. Visually, left to right: `2024`, `-`, `ב`.
5. P5. Visual order of the three numbers on the line, left to right: `2024`, then `3.14`, then `054-1234567`
   (reverse of logical order). Run x ranges do not overlap.
6. P6. Caret at the offset of the `0` that starts the phone number: downstream caret x is the left edge of the number
   run, upstream caret x is the right edge of the number run (it attaches to the preceding RTL space).
7. P7. Caret at offset text.len() (end) has x equal to the line's left edge x0 (+-0.5).
8. P8. The phone number run's width equals the width of the same digits laid out alone (no extra spacing).
9. P9. Wrapped at width 900 is not needed (fits); at the same text with width 400 every line's right-most glyph run
   ends at x = 400 (start-aligned RTL, trailing space hangs on the left) -- see rtl-wrap.

Verdicts (rendered, inspected in bidi-he-numbers.png and asserted in tests/visual.rs):
- P1 to P8 confirmed: one RTL line, each number one LTR run with increasing glyph x, `ב-2024` reads `2024-ב`
  (the hyphen sits in the RTL run), numbers in reverse logical order, caret at the number start is the number's left edge
  (downstream) or right edge (upstream), caret at the end is the line's left edge, number advance equals natural width.
- P9 (wrapping at 400) is covered by rtl-wrap. No defect found in the layout itself.
- Found while checking P8 on the multi-paragraph unwrapped case (see rtl-number-alignment): fixed.
