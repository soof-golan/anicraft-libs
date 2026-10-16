# masks_nested (200x200, white background)

Verdict: P1..P8 asserted in tests/visual/masks.rs and passing; the 4x render and edge crops inspected: the disc is
cut by the inner mask at x=50 and x=130 with pixel-aligned edges, arc anti-aliased through both masks. No discrepancy.

G1 (identity) has a mask: a Group (translate(10,0)) containing a disc of radius 60 at local (90,100), so the mask disc
is centred at device (100,100). G1's child is G2 with transform translate(30,0) and its own mask: rectangle
x 20..100, y 0..200 in G2's local space (so device x 50..130). G2's child is a red rectangle (0,0)-(200,200) in G2's local
space (device x 30..230).

The visible region is (red rectangle) AND (G2 mask) AND (G1 mask) = x in [50,130] inside the disc.

A second fixture on the same canvas: a half-transparent mask: a third group G3 at the bottom-left corner
(translate(0,0) with a mask square (10,150)-(40,190) filled with alpha 128) containing a green 20x... rectangle
(0,140)-(60,200) opaque green 0,255,0. Not asserted strictly (see verdicts).

## Predictions

- P1 Centre: (100,100) red 255,0,0. (60,100) red (inside the disc, x>=50). (125,100) red.
- P2 Left of the inner mask: (45,100) white (x<50) even though the disc and the red rectangle cover it.
- P3 Right of the inner mask: (135,100) white (x>130), though the disc covers it (35 px from the centre).
- P4 Disc limits: (100,45) red (55 px up); (100,38) white (62 px up); (100,160) white (60.5 px: outside
  by the pixel centre test 60.5 > 60), (100,157) red.
- P5 Outside the disc but inside the inner mask: (55,60) white (dist sqrt(45^2+40^2)=60.2 => at the edge; avoid) -> use
  (52,50): dist 69 -> white. (128,50): dist 54... sqrt(28^2+50^2)=57.3 inside -> red; (128,45): sqrt(28^2+55^2)=61.7 -> white.
- P6 The disc edge is anti-aliased through the nested masks: at least 30 pixels have alpha strictly between 40 and 215
  in the arc part of the boundary (not the straight cut).
- P7 The straight cut at x=50 is pixel-aligned: (49,100) white, (50,100) fully red (255,0,0); likewise x=130:
  (129,100) red, (130,100) white.
- P8 Nothing is drawn outside the union: (5,5), (195,195) white.
