# colour_alpha_onion: colour-transform tweens, nested alpha, onion skins

Written before rendering. Stage 100x100 white. Symbol `sq` (graphic): a red (255,0,0) 40x40 square at the origin
(drawn at 0.4 scale where needed; instances are 40x40 at x = 0, y = row * 25 unless noted). Colour tweens run
frames 0..10, linear (t = f / 10) unless an ease is given. Pixel values are what appears over the white stage.

## colour_alpha: instance alpha 100% -> 0%

- P1: a(f) = 1 - f / 10; over white the pixel is (255, 255 (1 - a), 255 (1 - a)) = (255, 25.5 f, 25.5 f), tolerance 2.
  f = 5: (255, 128, 128). f = 0 pure red, f = 10 pure white (square invisible).

## colour_tint: tint blue 0% -> blue 100% (mul 1 -> 0, add 0 -> (0,0,1))

- P2: f = 5: red (1,0,0) -> mul 0.5 add (0,0,0.5): (127.5, 0, 127.5) = (128, 0, 128) +- 2. f = 10: blue (0,0,255).

## colour_advanced: mul (1,1,1,1) add 0 -> mul (0.5, 2, 1, 0.5) add (0, 0.2, 0, 0)

- P3: p = 0.5: mul (0.75, 1.5, 1, 0.75), add (0, 0.1, 0, 0): straight colour (0.75, 0.1, 0), alpha 0.75, over white:
  R = 0.75 * 0.75 + 0.25 = 0.8125 -> 207; G = 0.75 * 0.1 + 0.25 = 0.325 -> 83; B = 0.25 -> 64. Tolerance 3.

## colour_ease: alpha 100% -> 0% with a `Color` target ease cubicIn, position linear

- P4: f = 5: progress 0.125, alpha 0.875, pixel (255, 32, 32) +- 2 (255 * 0.125). The same keyframe's position
  tween (a box moving 0 -> 100) is linear: x(5) = 50.

## colour_nested: outer instance alpha 100% -> 0%, containing a symbol whose inner instance has alpha 50% static

- P5: f = 5: outer 0.5 * inner 0.5 = 0.25: (255, 191, 191) +- 2. f = 0: 0.5 -> (255, 128, 128). f = 10: white.

## onion: 8-frame scene, 10x10 box at y = 45 tweening x = 10 f (f = 0..7: kf0 dur 7, kf7 dur 1)

`Evaluator::onion(scene, frame, before, after)` returns frames in time order, each with opacity
0.5 * (1 - (d - 1) / n) for distance d (1 = nearest) of n requested.

| call | frames | alphas |
|------|--------|--------|
| (3, before 2, after 2) | 1, 2, 4, 5 | 0.25, 0.5, 0.5, 0.25 |
| (0, before 3, after 0) | none | |
| (7, before 0, after 2) | none | |
| (6, before 0, after 3) | 7 | 0.5 |
| (9, before 3, after 0) | 6, 7 | 0.1667, 0.3333 |
| (20, before 3, after 3) | none | |
| (3, before 1, after 1) | 2, 4 | 0.5, 0.5 |
| (0, before 0, after 8) | 1..7 | 0.5 * (1 - (d - 1) / 8): 0.5, 0.4375, 0.375, 0.3125, 0.25, 0.1875, 0.125 |

- P6: the table above, alphas to 1e-6.
- P7 (composited, frame 3 with before 2, after 2 drawn under the frame itself): box 10f..10f+10 wide at y 45..55:
  current frame x = 30..40 pure RED at (35, 50); frame 2 (x 20..30) alpha 0.5 -> (255, 128, 128) at (25, 50); frame 1
  (x 10..20) alpha 0.25 -> (255, 191, 191) at (15, 50); frame 4 (x 40..50) 0.5 -> (255, 128, 128) at (45, 50); frame 5
  (x 50..60) 0.25 -> (255, 191, 191) at (55, 50); (5, 50) and (65, 50) WHITE. +- 2.

## Verdicts

colour_alpha.png and onion.png inspected. P1-P7 hold (alpha, tint, advanced transform, per-property colour ease,
nested alpha multiplication, onion frame lists and opacities). No disagreement.
