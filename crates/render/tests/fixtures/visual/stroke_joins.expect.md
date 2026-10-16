# stroke_joins (520x200, white background, black strokes, width 10, cap none)

Verdict: all points asserted in tests/visual/strokes.rs and passing; render plus chevron, hairpin, bend and right-angle
crops inspected: miter within the limit is a sharp spike, beyond the limit it bevels flat, round join is round, right
angle with limit 2 stays mitred. No discrepancy.

Row 1: four chevrons with apex A_k = (130k+30, 50), k=0..3, arms of length 80 leaving at +-10 degrees from the +x
axis, so the interior angle is 20 degrees. The miter ratio (miter length / width) is 1/sin(10 deg) = 5.7588.
k=0 miter join, limit 5.5 (exceeded: bevel). k=1 miter join, limit 6.0 (kept: sharp tip). k=2 round join.
k=3 bevel join.

Row 2 (centre line y=150):
- cell 0: polyline (10,150)-(60,150)-(110,152), miter join, limit 3: a 2.3 degree bend.
- cell 1: hairpin (140,150)-(190,150)-(140,150), round join.
- cell 2: hairpin (270,150)-(320,150)-(270,150), miter join, limit 3 (exceeded at 180 degrees: bevel).
- cell 3: right angle (420,110)-(420,170)-(480,170), miter join, limit 2 (ratio 1.414 kept).

## Predictions

- P1 Miter tip distance is (w/2)/sin(10 deg) = 28.79 px from the apex: for k=1 the tip is at x=130+30-28.79 = 131.2.
  Pixels (A.x-20, 49) and (A.x-20, 50) are black (alpha >= 200) for k=1. The same pixels are white for k=0, 2, 3.
- P2 k=0 (limit exceeded) is bevelled: the outline's left extent is A.x - 0.87 (5*sin 10 deg): (A.x-2, 49) white,
  (A.x+1, 49) black. k=3 the same.
- P3 k=2 round: leftmost extent is A.x-5: (A.x-4, 49) black, (A.x-6, 49) white.
- P4 Row 2 cell 0: the bend stays solid: the coverage of the pixel columns 59, 60 and 61 (rows 140..160) is 10.0 +-0.15
  each, and pixel (60,150) is opaque black.
- P5 Hairpin round (cell 1): (193,149) black (3.5 px from the turn point), (196,149) white. Start end is flat:
  (138,149) white, (142,149) black.
- P6 Hairpin with miter beyond limit (cell 2): flat end at the turn: (318,149) black, (322,149) white.
- P7 Right angle with limit 2 (cell 3): the outer corner is a sharp miter: pixel (415,174) black (alpha >= 240); the
  inner corner is not filled beyond: (425,164) white... the pixel is outside the stroke (x>425, y<165).
