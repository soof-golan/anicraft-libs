# shape_tween: square -> ring (hole appears), ring -> square, off-centre hole, hints

Written before rendering. Stage 100x100, white. Shape tweens run over frames 0..10 (end keyframe at 10), linear, so
t = f / 10. Rendering is non-antialiased; a pixel (x, y) is the unit square [x, x+1] x [y, y+1], tested at its
centre (x + 0.5, y + 0.5).

## ring_grow: red square (20,20)-(80,80) -> red ring: same outer square plus a hole (40,40)-(60,60)

The outer contour pairs with the outer contour (same centroid, nearly same size); the hole has no partner, it grows
from its own centroid (50, 50) to its final size. Half-size of the hole is 10 t.

- P1: f = 0: solid, (50, 50) RED.
- P2: f = 1: hole (49..51)^2: (50, 50) WHITE, (47, 50) RED.
- P3: f = 5: hole (45..55): (50, 50) WHITE, (46, 50) WHITE, (44, 50) RED, (50, 44) RED, (21, 21) RED, (19, 50) WHITE.
- P4: f = 9: hole 41..59: (42, 50) WHITE, (40, 50) RED, (58, 58) WHITE, (60, 60) RED.
- P5: f = 10: the end shape, (41, 50) WHITE, (39, 50) RED.
- P6: the hole is an actual hole in ONE filled region: no red fill is drawn inside it at any frame 1..10 (all of P2..P5
  centre probes WHITE), and the outer edge stays at 20..80 throughout (pixel (19, 50) WHITE and (20, 50) RED at every
  frame).

## ring_shrink: the reverse (ring -> solid square)

Hole half-size is 10 (1 - t) and it collapses on its centroid.

- P7: f = 5: (50, 50) WHITE, (46, 50) WHITE, (44, 50) RED. f = 9: (50, 50) WHITE, (48, 50) RED. f = 10: solid (50, 50) RED.

## ring_offcentre: ring whose hole is (30,40)-(45,60) (centroid (37.5, 50)), square -> ring

The hole grows around its OWN centroid, not the shape's: half-sizes 7.5 t (x) and 10 t (y).

- P8: f = 5: hole x 33.75..41.25, y 45..55. (37, 50) WHITE, (34, 50) WHITE, (33, 50) RED, (40, 50) WHITE, (41, 50) RED,
  (37, 44) RED, (37, 56) RED, (50, 50) RED (the middle of the ring is solid on the right of the hole).

## ring_colour: red square -> blue ring

- P9: f = 5: the solid part is the midpoint colour (127/128, 0, 127/128) (tolerance 2). f = 0 pure red, f = 10 pure blue.
- P10: the hole stays a hole while colour changes: (50, 50) WHITE at f = 8.

## hint_diamond: the same red square (20,20)-(80,80) at both ends, one hint: start (20,20) -> end (80,20)

Without hints the best alignment is the identity: the square stays a square for every t (pixel (22, 22) RED at
f = 5). A hint pins the start's top-left corner to the end's top-right corner; the remaining corners follow in the
same direction (TL -> TR, TR -> BR, BR -> BL, BL -> TL). At t = 0.5 the vertices are the midpoints: (50,20), (80,50),
(50,80), (20,50), i.e. the diamond |x - 50| + |y - 50| <= 30.

- P11 (no hints): f = 5: (22, 22) RED, (78, 78) RED.
- P12 (hint): f = 5: (22, 22) WHITE, (78, 22) WHITE, (22, 78) WHITE, (78, 78) WHITE, (50, 50) RED, (50, 25) RED,
  (25, 50) RED, (75, 50) RED, (50, 82) WHITE.
- P13 (hint): f = 0 is the square ((22, 22) RED) and f = 10 the square again.
- P14 (hint): the area at f = 5 is the diamond's, about 1800 px^2 (|diag| 60: 60 * 60 / 2); between 1500 and 2100.

## Verdicts

ring_grow, ring_shrink, ring_offcentre: P1-P8 hold; the hole appears and collapses around its own centroid.

BUG 1 (P9/P10, ring_colour.png): when the start and end fills differ (red square -> blue ring), the growing hole
was drawn as a separate filled blue square in the hole (51, 0, 204 at (50, 50), f8) instead of a hole. Root cause:
`MorphPlan::at` grouped in-between contours by (start style, end style); the unmatched hole has (None, end style) and
landed in its own group, so it was filled instead of subtracted from the ring. Fixed: a contour that exists at one end
only joins the matched group with the same style on that side. Now (50, 50) is white and the ring colour is the
midpoint colour.

BUG 2 (P12, hint_diamond.png): one hint pinned the arc position but the direction was chosen by minimum vertex
distance, which reversed the end contour; TL -> TR then paired TR -> TL and the in-betweens collapsed into a sliver
(f5 (50, 50) was white). Fixed: hints pin points not direction, so the keyframes' winding is kept unless the contours
wind oppositely or the hints are only consistent reversed. Now P11-P14 hold: the in-betweens rotate and f5 is the
diamond. Distributive resampling bows the diamond edges very slightly (visible at 8x), which is within the
documented softening of that blend mode and inside the area bound of P14.
