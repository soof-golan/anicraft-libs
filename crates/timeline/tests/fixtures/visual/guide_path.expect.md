# guide_path: classic tween along a motion guide, with and without orient-to-path

Written before rendering. Stage 100x100, white. Symbol `bar` as in rotation.expect.md (red bar x 0..30, blue tip
marker centred 27 from the registration point). Layer `guide` (kind Guide) holds a stroked path; layer `mover`
(parent = guide) tweens an instance from one end of the path to the other over frames 0..10 (end keyframe at 10),
linear ease, rotation 0 in both keyframes, snap on. A guided tween moves the registration point along the path at
constant speed (equal arc length per frame); the guide layer itself draws nothing.

## guide_polyline: (10,10) -> (10,30) -> (90,30)

Lengths 20 and 80, total 100, so s = 10 f along the path.

| f | 0 | 1 | 2 | 3 | 5 | 7 | 10 |
|---|---|---|---|---|---|---|----|
| position | (10,10) | (10,20) | (10,30) | (20,30) | (40,30) | (60,30) | (90,30) |

- P1: the registration point is on that position at every listed frame (matrix translation to 1e-6).
- P2 (parametrisation): f = 5 is at (40,30), NOT at the corner (10,30) (which a per-segment t would give).
- P3 (guide invisible): the stroked guide is not drawn: pixel (10, 15) is white at f = 8.
- P4 (orient off): the bar stays horizontal, tip marker at position + (27, 0).
- P5 (orient on): baseline parallel to the path, absolute: while on the vertical segment (f = 0, 1) the bar points
  down (theta = 90 degrees clockwise), tip at position + (0, 27); after the corner (f = 3, 5, 7, 10) theta = 0.
  f = 1: tip pixel (10, 47) BLUE; f = 5: tip pixel (67, 30) BLUE and the pixel at position + (0, 27) = (40, 57)
  is WHITE.

## guide_circle: clockwise circle r = 40 about (50,50), start and end at (90,50), 12 frames

Position p = f / 12 of the circumference: (50 + 40 cos(2 pi p), 50 + 40 sin(2 pi p)).

- P6: f = 3: (50, 90); f = 6: (10, 50); f = 9: (50, 10); f = 12: (90, 50), each within 0.3 px (the four-cubic
  circle approximation is within 0.03% of r).
- P7: with orient on, the tangent at f = 3 points along -x, so theta = 180 degrees: tip centre (23, 90): pixel
  (23, 90) BLUE; at f = 6 theta = 270 (tip up): pixel (10, 23) BLUE; at f = 9 theta = 0 (0 + 360): tip pixel
  (77, 10).

## Verdicts

FIRST RENDER DISAGREED: the tween ignored the guide layer completely and moved the instance in a straight line
between its keyframe positions (P1, P2, P6 failed). Root cause: motion guides were not implemented in anim-timeline.
Fixed by adding `guide.rs` (arc-length parametrisation of the guide's first stroke, orient-to-path, snap) and
`tween::classic_along`; the evaluator now uses the guided tween when the layer's parent is a guide layer. After the fix
guide_polyline.png, guide_circle.png and guide_orient.png match P1-P7 (positions to 1e-6 on the polyline, 0.3 px on
the circle; orientation 90 degrees on the vertical leg and 0 after the corner; the guide itself is not drawn).
