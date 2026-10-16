# nested_symbols: graphic loop / play-once / single frame with first-frame offsets, inside each other, and movie clips

Written before rendering. Stage 100x100 (24 fps), 14 frames. Symbol `dots` (graphic, 4 frames): frame k has one 10x10
square at x = 10 k, y = 0, colour k0 RED, k1 GREEN, k2 BLUE, k3 BLACK.

Symbol `mid` (graphic, 6 frames): one layer; frames 0..1 are an empty keyframe; from local frame 2 to 5 a keyframe
holds an instance of `dots` (loop, first_frame = 1) at the origin. Symbol `mid_once` is the same but the inner
instance is play-once with first_frame = 1.

Four rows on the stage, each an instance of `mid` / `mid_once` in a keyframe at frame 0 lasting 14 frames, at
y = 0, 25, 50, 75:

| row | symbol | mode | first_frame |
|-----|--------|------|-------------|
| 0 | mid | loop | 4 |
| 1 | mid | play once | 4 |
| 2 | mid | single frame | 3 |
| 3 | mid_once | loop | 4 |

A graphic shows local frame `first_frame + frames since its keyframe`, looped (mod length), clamped (play once) or
fixed (single frame). The inner instance's keyframe starts at local frame 2, so it counts from there.

Row 0: local L(f) = (4 + f) mod 6; inner shows when L >= 2 with k = (1 + (L - 2)) mod 4.

| f | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 |
|---|---|---|---|---|---|---|---|---|---|---|----|----|----|----|
| L | 4 | 5 | 0 | 1 | 2 | 3 | 4 | 5 | 0 | 1 | 2 | 3 | 4 | 5 |
| k (row 0) | 3 | 0 | - | - | 1 | 2 | 3 | 0 | - | - | 1 | 2 | 3 | 0 |

Row 1 (play once from 4): L = min(4 + f, 5): f = 0: L 4, k 3; every f >= 1: L = 5, k = 0 (held for ever).
Row 2 (single frame 3): L = 3 for all f: k = 2 (BLUE at x = 20) for all f.
Row 3 (mid_once, outer loop first 4): inner is play-once from 1: k = min(1 + (L - 2), 3) when L >= 2:
L 2 -> 1, L 3 -> 2, L 4 -> 3, L 5 -> 3. So f = 0 (L 4): 3; f = 1 (L 5): 3; f = 2, 3 (L 0, 1): empty; f = 4: 1;
f = 5: 2; f = 6: 3; f = 7: 3; f = 8, 9 empty; f = 10: 1; f = 11: 2; f = 12: 3; f = 13: 3.

- P1: at each frame, the pixel (10 k + 5, row_y + 5) has the colour of k and the other three dot cells of that row are
  white (probe (10 j + 5, row_y + 5) for j != k is WHITE), for every row and frame in the tables.
- P2: empty frames render the whole row white.
- P3: a looped outer graphic whose inner is play-once: the inner restarts each outer loop (row 3 at f = 4 is k = 1
  again, not k = 3).

## movie clips (second document)

Symbol `clip` (movie clip, 4 frames, same dot artwork as `dots`) and `clipwrap` (movie clip, 4 frames) holding a
graphic `dots` instance (loop, first_frame 0) in a keyframe at its frame 0 lasting 4 frames. Two instances on the
stage at y = 0 and y = 25, `clip` with first_frame = 2 and `clipwrap` with first_frame = 2.

- P4 (authoring default): every frame shows the clip's first_frame: k = 2 (BLUE) for `clip`; for `clipwrap` the
  clip is at local 2, the inner graphic is at since 2 -> k = 2.
- P5 (advance_movie_clips): `clip` shows k = (2 + f) mod 4 and `clipwrap` also k = (2 + f) mod 4, f = 0..9. The
  nested graphic follows the CLIP's own timeline, not the global one (identical here only because of first_frame 0
  in the graphic; to separate them the inner graphic uses first_frame = 1 in a second row at y = 50: k = (1 + (2 + f)
  mod 4) mod 4).

## Verdicts

nested_symbols.png (4 rows x 14 frames) inspected cell by cell against the tables. P1-P5 hold for loop, play-once,
single-frame, play-once nested in a loop (restarts each outer loop), and movie clips (held by default, advancing with
`advance_movie_clips`, nested graphic following the clip's timeline). No disagreement.
