# stroke_huge (400x440, white background, black strokes)

Verdict: all points asserted in tests/visual/strokes.rs and passing; render and cap-corner crop inspected: the 200 px
round-capped stroke is a stadium, the square-capped one a square with sharp corners. No discrepancy.

Row 1: line (100,100)-(300,100), width 200, round caps: a stadium of radius 100 reaching x=0 and x=400.
Row 2: line (100,330)-(300,330), width 200, caps none (butt): a rectangle 100..300 x 230..430.
Strokes share the canvas, so the first row spans y 0..200 and the second y 230..430.

## Predictions

- P1 Row 1: (100,100), (200,5), (200,195) black; (5,100) and (395,100) black (the caps reach the canvas edges);
  (60,40) black (72 px from the cap centre, inside radius 100); (10,10) white (127 px away) and (390,10) white,
  (10,190), (390,190) white.
- P2 Row 2 (butt): (200,235), (200,425), (101,330), (299,330) black; (99,330) and (301,330) white; (50,330) white; (200,229)
  and (200,431) white.
- P3 Row 1's disc edge at the left: pixel (30,100) black; (3,60) is outside radius 100 from (100,100)? distance
  = sqrt(97.5^2+40.5^2) = 105.6 so white.
- P4 The pixels between the rows, (200,215), are white.
