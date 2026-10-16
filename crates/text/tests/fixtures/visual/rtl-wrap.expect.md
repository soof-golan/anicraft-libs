# rtl-wrap

A long Hebrew paragraph with an English parenthesised name, a phone number, a year and a decimal. DejaVu Sans 24 px,
auto direction (RTL), start alignment, wrapped at width 330 px (about 4-6 lines).

1. P1. Every line is RTL; line ranges are contiguous, ordered top to bottom, and cover the whole text: line[0].range
   starts at 0, line[i].range.end == line[i+1].range.start, the last ends at text.len().
2. P2. On every line the right-most glyph run ends at x = 330 (+-0.6): lines are flush right, the first logical
   character of each line is at the right edge. A soft-wrapped trailing space does NOT push the text: it hangs on the
   left, outside the ink.
3. P3. The ink of no line crosses the box: every run's [origin.x, origin.x+advance] lies within [0, 330 + 0.6].
4. P4. `054-1234567` is never split across lines (hyphen before a digit is not a break opportunity) and `3.14` is not
   split; so each of these strings lies entirely inside one line's range. Likewise `(Daniel Cohen)` may break after the
   space inside it, but the `(` stays with `Daniel` (never ends a line) and `)` stays with `Cohen` (never starts a line).
5. P5. Inside a wrapped line, the visual order of mixed runs is the bidi order for the line only: for the line holding
   the phone number, the number run's left neighbour is later text in logical order and the right neighbour is earlier.
6. P6. A line never starts with `)` or `.` or `,` (closing punctuation cannot begin a line, UAX #14).
7. P7. Caret at the end of line k (upstream) and at the start of line k+1 (downstream) have the same offset but sit on
   different lines: the upstream caret has y of line k, the downstream y of line k+1. The downstream caret of line k+1
   is at x = right edge 330 (+-0.6) (start of an RTL line), the upstream caret on line k is at the visual left end of
   line k's text (including the hanging space: left of the last ink, x <= line ink x0).

Verdicts: (filled in after inspection)
