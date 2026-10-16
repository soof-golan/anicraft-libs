# masks: animated masks over animated content, mask inside mask, blank mask frames

Written before rendering. Stage 100x100, white background. Layer order top to bottom: `m` (mask), `content` and
`bar` (both masked by `m`).

## mask_tween

- `m`: a 30x30 square at y = 35..65, classic-tweened from x = 0..30 (frame 0) to x = 70..100 (frame 10), linear.
  At frame f the square spans x = 7 f .. 7 f + 30.
- `content`: ten static vertical stripes, 10 px wide, x = 0..100, full height: stripe i red if i even, green if odd.
- `bar` (above content): a blue full-width bar 100 x 20 whose top edge tweens from y = 0 (frame 0) to y = 80
  (frame 10): top = 8 f.

Frame 5: mask x = 35..65, y = 35..65; bar y = 40..60.

- P1: (45, 50) BLUE (inside mask, on the bar); (45, 38) RED (stripe 4, above the bar); (37, 38) GREEN (stripe 3);
  (20, 50) WHITE (outside mask in x); (45, 70) WHITE (below the mask); (45, 30) WHITE (above the mask).
- P2: edges to the pixel: the bar covers y 40..60, so (35, 50) BLUE; (34, 50) WHITE; (64, 50) BLUE; (65, 50) WHITE;
  below the bar, stripes show: (35, 62) GREEN (stripe 3), (34, 62) WHITE; (50, 64) GREEN (stripe 5); (50, 65) WHITE.
- P3: frame 0: mask x = 0..30; the bar is at y = 0..20, outside the mask in y, so only stripes show: (5, 50) RED,
  (15, 50) GREEN, (25, 50) RED, (35, 50) WHITE.
- P4: frame 10: mask x = 70..100; bar y = 80..100 outside mask: (75, 50) GREEN (stripe 7), (95, 50) GREEN (stripe 9),
  (85, 50) RED (stripe 8), (65, 50) WHITE.
- P5: the mask is not drawn itself: (0..30 square at frame 0) shows only stripes, never a mask colour.

## mask_blank_frames

Same stripes masked by a layer with a square at frames 0..2, a BLANK keyframe at frames 3..5 and the square again at
frames 6..8.

- P6: frames 3..5 draw no stripes at all (everything white): a blank mask hides what it masks.
- P7: frames 0..2 and 6..8 show stripes inside the square.

## mask_in_mask

Symbol `inner` (graphic, 5 frames: kf 0 dur 4 classic-tweened, kf 4 dur 1): a mask layer with a 40x40 square at
x = 0..40 (frame 0) tweening to x = 40..80 (frame 4), y = 0..40, masking a full-stage GREEN rectangle. Stage: a mask
layer with a static rectangle x = 20..80, y = 0..100; the masked layer holds an instance of `inner` (loop). Stage has
10 frames. Visible region at stage frame f is the green in (inner square at local f mod 5) intersect (x 20..80):
inner square x range is [10 L, 10 L + 40] for local L = f mod 5.

| f | 0 | 1 | 2 | 3 | 4 | 5 | 9 |
|---|---|---|---|---|---|---|---|
| L | 0 | 1 | 2 | 3 | 4 | 0 | 4 |
| visible x | 20..40 | 20..50 | 20..60 | 30..70 | 40..80 | 20..40 | 40..80 |

- P8: green exactly at y 0..40 inside the visible x range, white everywhere else: f = 2: (25, 20) GREEN,
  (59, 20) GREEN, (60, 20) WHITE, (19, 20) WHITE, (30, 45) WHITE; f = 3: (29, 20) WHITE, (30, 20) GREEN,
  (69, 20) GREEN, (70, 20) WHITE; f = 5: (39, 20) GREEN, (40, 20) WHITE.

## Verdicts

mask_tween.png, mask_blank.png, mask_in_mask.png inspected. P1-P8 hold: the animated mask clips both the static
stripes and the tweening bar to the pixel at every probed frame, blank mask keyframes hide the masked layers, and a
mask inside a graphic symbol intersects with the stage mask. No disagreement.
