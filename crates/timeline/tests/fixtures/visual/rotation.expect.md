# rotation: CW with extra turns, auto across the 0/360 wrap, CCW the long way

Written before rendering. Stage 100x100, white. Symbol `bar` (graphic): a red bar `x 0..30, y -3..3` with a blue
6x6 marker centred on the tip (`x 24..30`), so the tip is at distance 27 from the registration point. Every instance
sits at translate(50, 50). Tweens span frames 0..10 (end keyframe at 10), linear, so progress p = f / 10.

Screen angle theta is clockwise-positive (y down): the tip centre is (50 + 27 cos theta, 50 + 27 sin theta).

## rot_cw2: start 0 degrees, end 90 degrees, rotate = clockwise, 2 extra turns

Total travel = 90 + 2 * 360 = 810 degrees, theta(f) = 81 f degrees (mod 360):

| f | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
|---|---|---|---|---|---|---|---|---|---|---|----|
| theta | 0 | 81 | 162 | 243 | 324 | 45 | 126 | 207 | 288 | 9 | 90 |

- P1: at every frame the pixel under the tip centre is BLUE and the pixel under the mirrored point
  (50 - 27 cos, 50 - 27 sin) is WHITE.
- P2: the matrix has unit determinant at every frame (no shrink or skew creeps in), scale stays 1.
- P3: frame 10 is exactly the end keyframe (rotation 90), frame 0 exactly the start.
- P4: frame 5 pixel (69, 69) is BLUE (tip at 45 degrees = (69.09, 69.09)), (31, 31) is WHITE.

## rot_auto_wrap: start 350 degrees, end 10 degrees, rotate = auto

Shortest way is +20 degrees through 0, theta(f) = 350 + 2 f.

- P5: f = 5 is exactly 0 degrees: tip pixel (77, 50) BLUE, (23, 50) WHITE.
- P6: at no frame is the tip left of x = 50 (it never swings through 180 degrees): for all f the tip centre has
  x >= 50 + 27 cos(10 degrees) = 76.59.
- P7: f = 2 is 354 degrees: tip centre (76.85, 47.18) -> pixel (76, 47) BLUE.

## rot_ccw_long: start 350 degrees, end 10 degrees, rotate = counter-clockwise, 0 turns

Counter-clockwise (anticlockwise on screen) from 350 to 10 is 340 degrees backwards: theta(f) = 350 - 34 f.

- P8: f = 5 is 180 degrees: tip pixel (23, 50) BLUE, (77, 50) WHITE.
- P9: f = 10 is exactly 10 degrees (end keyframe).
- P10: f = 1 is 316 degrees: tip centre (69.42, 31.24), pixel (69, 31) BLUE.

## Verdicts

Rendered (rot_cw2.png, rot_auto_wrap.png, rot_ccw_long.png) and inspected; asserted in `tests/visual_tweens.rs`.
P1-P4 hold (bar tip at the predicted angle each frame, unit determinant, exact end frames). P5-P7 hold (the auto
tween never swings through 180 degrees). P8-P10 hold. No disagreement.
