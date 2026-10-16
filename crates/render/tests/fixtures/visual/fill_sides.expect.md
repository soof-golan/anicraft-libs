# fill_sides (300x200, white background) - XFL edges through EdgeShape::to_display

Verdict: P1..P6 asserted in tests/visual/fills.rs and passing; render inspected (both ring spellings identical,
three nested squares, scrambled/short/open contours closed, undefined style draws nothing). P7 FAILS and is kept as an
ignored test (fill_sides_seam): the 16x crop shows a lighter column between green and blue where white background
bleeds through (anti-aliased conflation: each fill is composited separately, so a shared edge that falls mid-pixel
gets 50% + 50% x 50% coverage = 75% instead of 100%). Not fixed: needs coverage-union rendering of abutting regions
(render one path per style over a shared mask, or expand fills by a fraction of a pixel); reported as a known bug.

Fill styles: 1 red 255,0,0; 2 green 0,160,0; 3 blue 0,0,255. fill_style0 is the fill on the LEFT of the direction of
travel and fill_style1 the fill on the RIGHT (screen coordinates, y down). Coordinates are written in twips (px x 20).

Row 1 (y 0..100):
- Cell 1 (x 0..100): ring. Outer square (5,5)-(95,95) travelled clockwise on screen with fill1=1; inner square
  (30,30)-(70,70) travelled counter-clockwise with fill1=1.
- Cell 2 (x 100..200): the same ring written the other way round: outer square (105,5)-(195,95) travelled
  counter-clockwise with fill0=1; inner square (130,30)-(170,70) travelled clockwise with fill0=1.
- Cell 3 (x 200..300): three nested squares: blue outer (205,5)-(295,95) cw fill1=3; red (225,25)-(275,75) cw with
  fill1=1 fill0=3; green (240,40)-(260,60) cw with fill1=2 fill0=1. Plus an interior edge (230,30)->(270,30) with
  fill0=fill1=1 and no stroke across the red ring.

Row 2 (y 100..200):
- Cell 4 (x 0..100): red square (5,105)-(95,195) given as four edges in scrambled order, the east side travelled
  northwards with fill0=1 (left of northward travel is west = inside), ends missing by 0.4 twip.
- Cell 5 (x 100..200): green square (105,105)-(195,195) with only three sides given (top, right, bottom): the
  contour is closed with a straight line.
- Cell 6 (x 200..300): top half (y 105..155): a square whose edge says fill1=9 (no such style): draws nothing. Bottom
  half (y 160..195): two squares sharing the vertical edge x=250.5: left green (fill 2), right blue (fill 3); the
  shared edge is one edge with fill0=3, fill1=2 travelling south.

## Predictions

- P1 Cell 1: (15,50) red, (50,15) red, (50,50) white.
- P2 Cell 2: (115,50) red, (150,15) red, (150,50) white. Identical image to cell 1 shifted by 100 (max channel diff 0).
- P3 Cell 3: (210,50) blue, (230,50) red, (250,50) green, (250,15) blue. Interior edge leaves no seam: (250,30) and
  the pixels (250,29), (250,31) are all pure red 255,0,0.
- P4 Cell 4 is an unbroken red square: (50,150) red, (6,106) red, (93,193) red.
- P5 Cell 5 is an unbroken green square: (150,150) 0,160,0; (106,106) and (193,193) green.
- P6 Cell 6: (225,130) white (the fill-9 square is not drawn). (225,180) green, (275,180) blue.
- P7 The shared edge at x=250.5 splits pixel column 250 half green and half blue. Flash shows no background through
  abutting fills, so (250,180) is about 0,80,128 (average of green 0,160,0 and blue 0,0,255), not lightened by white:
  its red channel is below 20.
