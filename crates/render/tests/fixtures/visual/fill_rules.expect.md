# fill_rules (240x160, white background, all fills pure red 255,0,0)

Aim: non-zero versus even-odd on self-intersecting and overlapping contours.

Cells of 80x80. Row 1: A = five-point star (centre 40,40, R=30, vertices at -90+144k degrees, one path) non-zero;
B = the same star centred 120,40, even-odd; C = two squares in opposite directions, non-zero: cw (170,10)-(210,50) and
ccw (190,30)-(230,70). Row 2: D = same-direction squares (10,90)-(50,130) and (30,110)-(70,150), non-zero;
E = same squares shifted by +80 x, even-odd; F = outer cw square (170,90)-(230,150) with inner cw square
(185,105)-(215,135) drawn ccw (a hole by direction), non-zero.

## Predictions

- P1 A: the centre pentagon has winding 2 so with non-zero it is filled: pixel (40,40) red. Each of the 5 arms is
  filled: the pixel at 16 px from the centre along each tip direction is red. The notch below the inner pentagon,
  (40,58), is white.
- P2 B: even-odd: the centre pentagon is a hole: (120,40) white. Arms (16 px from the centre toward each tip) red.
  (120,58) white.
- P3 C: opposite directions cancel under non-zero: the overlap (200,40) is white. Square-only parts (180,20) and
  (220,60) red. (220,20) and (180,60) white (outside both).
- P4 D: same direction under non-zero: the overlap (40,120) red, and (20,100), (60,140) red. (60,100) and (20,140) white.
- P5 E: same direction under even-odd: the overlap (120,120) white; (100,100) and (140,140) red; (140,100), (100,140) white.
- P6 F: the inner contour runs against the outer, winding 0 inside it, so non-zero leaves a hole: (200,120) white,
  (175,95) red.
- P7 Edges: the star outline is anti-aliased (some pixels with alpha strictly between 0 and 255 exist).

## Verdicts

P1..P7 asserted in tests/visual/fills.rs and passing. The 3x render and star crops were inspected: the non-zero
star is solid, the even-odd star has a pentagonal hole, opposite-direction squares cancel to a white overlap, same
direction squares merge (non-zero) or cancel (even-odd), the reversed inner contour leaves a hole. Star edges are
anti-aliased. No discrepancy.
