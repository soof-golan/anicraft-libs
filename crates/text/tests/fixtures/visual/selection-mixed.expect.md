# selection-mixed

DejaVu Sans 36 px. P0 (LTR): `abc שלום def`. P1 (RTL): `שלום abc עולם`.
Let H = the Hebrew run in P0 (its x0..x1), a(ch) the advance of a character.

P0, select from after `של` (offset 4+2*2=... bytes: `abc ` = 4 bytes, ש ל = 4 bytes, so offset 8) to after `de`
(`abc שלום de` = 4 + 8 + 1 + ... `שלום`=8 bytes, space 1: offset 4+8+1+2 = 15):

1. P1. Two rectangles (selection crosses LTR -> RTL -> LTR) on one line, same y0/y1 (the line box).
2. P2. Rect A covers the LEFT part of the Hebrew word: x0 = H.x0 (left edge of the word), x1 = H.x0 + a(ם) + a(ו)
   (the two selected characters are the leftmost glyphs because the word is RTL).
3. P3. Rect B starts at H.x1 (the right edge of the Hebrew word, where the following space begins) and ends at
   x of `f` = the caret before `f`: width = a(space) + a(d) + a(e).
4. P4. Gap between A and B = a(ש) + a(ל), the unselected part of the word.

Selection from `c` (offset 2) to offset 14 (inside the Hebrew word? no: offsets 2..6 = `c ` + ש):
5. P5. Selecting range 2..6 (`c ` + ש) gives two rects: one over `c ` (x: caret2 .. caret4 = H.x0-ish; actually
   `c`+space) and a second rect over ש, which is the RIGHTMOST glyph of the Hebrew word: x1 == H.x1, width a(ש).

P1 (RTL base): logical `שלום abc עולם`; visual (left to right): `םלוע abc םולש`.
6. P6. Selecting the whole text gives rects that tile the line without gaps (>= 0.5 px) or overlap, union == line rect.
7. P7. Selecting just `abc` gives one rect with width a(a)+a(b)+a(c) at the centre of the line.
8. P8. Selecting from the middle of `שלום` (after ש) to the middle of `עולם` (after ע) gives three rects: right part of
   the first word (ל ו ם = the left 3 letters of the first Hebrew run... precisely the characters after ש), the whole
   ` abc `, and the characters of the second word up to ע (the right 1 letter of the second word).

Verdicts (selection-mixed.png and selection-mixed-overlay.png, which draws the P1 and P8 selections): P1 to P8
confirmed. The mixed-direction selection splits into the left two letters of the word and the space plus `de`; the
RTL line's selection is one contiguous band from the left edge of the ayin to the left edge of the first shin.
