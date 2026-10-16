# arabic-joining

Nine Arabic paragraphs, DejaVu Sans 40 px, each its own paragraph (auto -> RTL).

- P1. `بببب` gives 4 glyphs. In x order (left to right) they are `final, medial, medial, initial`: ids [c, b, b, a] with
  a, b, c pairwise different (contextual forms), so glyph 1 == glyph 2 and no other pair is equal.
- P2. A lone `ب` is the isolated form: 1 glyph, whose id differs from a, b and c.
- P3. `لا` (lam + alef) is the lam-alef ligature: ONE glyph, not equal to the glyph for lam or alef alone. The glyph
  covers two characters (4 bytes), and there is a caret stop between ل and ا at the middle of the ligature glyph
  (within +-1.5 px of the centre): the three offsets 0, 2, 4 give three distinct caret x, evenly spaced (+-1.5 px).
- P4. `ل` ZWNJ `ا` is NOT ligated: two letters (zero-width ZWNJ glyph may add a third): more glyphs than P3, and neither
  glyph equals the P3 ligature.
- P5. `ب` ZWJ `ب` forces the joined forms: glyph ids in x order [c, a] (final on the left, initial on the right),
  the same as the first two letters of... precisely the same pair as laying out `بب`.
- P6. `ب` ZWNJ `ب` shows two isolated forms: both ids equal the P2 isolated id.
- P7. `بببب` with letter_spacing 6 behaves exactly like P1: same ids and same advances (spacing is not applied to
  cursive scripts, so joins stay connected).
- P8. `عام ٢٠٢٤`: the digits (Arabic-Indic, AN) form one LTR run left of the word; glyph x order is ٢ ٠ ٢ ٤ so ids[0] ==
  ids[2], ids[1] != ids[0], ids[3] differs from both. The word `عام` is to the right of the digits.
- P9. `بلا ببب`: the lam-alef ligature inside a word keeps joining to the preceding beh (beh is initial-form before
  lam-alef's final-form ligature): beh glyph id in `بلا` (first word) equals a of P1 (initial) hmm only if the glyph is the
  initial form; assert it equals the initial form id `a`.
  The second word `ببب` has glyph ids [c, b, a] in x order.

Verdicts: (filled in after inspection)
