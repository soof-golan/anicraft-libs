# emoji-fallback

Liberation Sans 36 px (has no emoji, no Hebrew? it has Hebrew), text `Hi 😀 שלום 👍🏽 🇮🇱 ✓ 👩‍👩‍👧 end`, plus a second paragraph
`a`, `😀` (a 60 px run) and `b`.

1. P1. Latin letters use Liberation Sans; the emoji are shaped with a fallback font (a different font face id than the
   Latin runs). No glyph in the layout is `.notdef` (id 0).
2. P2. Cluster sizes: `👍🏽` (thumbs up + skin tone) is ONE cluster (8 bytes), `🇮🇱` (two regional indicators) is ONE
   cluster (8 bytes), `👩‍👩‍👧` (ZWJ sequence, 18 bytes) is ONE cluster. Moving the caret right across each of them is one step
   (so offsets visited never lie strictly inside those byte ranges).
3. P3. A visible mark: each emoji cluster must draw something. After rendering, the pixel region of the 😀 cluster
   (its advance box x range, from baseline-ascent to baseline+descent) contains at least 30 non-background pixels at 2x
   (the emoji is not silently blank), and the glyph for 😀 has width at least 0.8 em.
4. P4. `✓` (U+2713) is rendered from some installed font (not tofu): its glyph id is not 0.
5. P5. Paragraph 1: the line height of the second paragraph is at least that of the 60 px run (>= 60 px), and the
   baseline is shared by `a`, `😀`, `b` (all on one line, same baseline).
6. P6. Caret x of offset before `b` minus caret x before `😀` equals the 😀 advance (+-0.5).

Verdicts: (filled in after inspection)
