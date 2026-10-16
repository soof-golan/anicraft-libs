# svg-strokes

Stage 200x120, white background, strokes in near-black (20,20,20).

1. Miter apex: polyline (20,100) -> (40,20) -> (60,100), width 6, Join::Miter, miter_limit 10, Cap::None.
   Half apex angle a = atan(20/80) = 14.04 deg, miter ratio 1/sin(a) = 4.12 (< 10) so the join is a sharp miter.
   The miter tip lies at distance 3/sin(a) = 12.37 above the apex: y = 7.63.
2. Same polyline shifted +60 in x (apex (100,20)) with miter_limit 3 (< 4.12): the join falls back to a BEVEL, the tip is cut flat.
3. Dashed horizontal line y=110 (centre of a 4px line: rows 108..112), x 10..190, dashes [10,5], butt caps: ink on x in [10+15k, 20+15k).
4. Dashed line y=90 width 4, same dashes, Cap::Square: each dash extends 2px at both ends: ink on [8+15k, 22+15k) so 1px gaps remain.
5. Group with transform scale(4): (a) path (30,5)->(45,5) with StrokeScale::None width 2, (b) path (30,10)->(45,10) with StrokeScale::Normal width 2.

## Predictions

1. Pixel (40,10) is ink (the miter tip, y 7.63..): ink. Pixel (40,6) is white (above tip).
   Pixel (40,22) is ink (centre of the stroke near apex). Pixel (30,60) is ink (on the left leg: leg x at y=60 is 30, width 6 -> the leg is 6/cos(14 deg)=6.2 wide).
2. Pixel (100,10) is white (bevel: the tip would be at y 7.63 but is cut; bevel edge is at distance 3*cos... the cut line passes ~ y=17.1+;
   rows above y=15 are white). Pixel (100,19) is ink.
3. Pixels (15,110) ink, (22,110) white (gap 20..25), (27,110) ink, (37,110) white (gap 35..40), (42,110) ink; (15,106) white; (15,112) white.
4. Pixels (9,90) ink, (21,90) ink (square cap extends to 22), (22,90) white (gap [22,23)), (24,90) ink, (7,90) white.
5. (a) horizontal line is device y=20 spanning x 120..180, thickness exactly 2 px (rows 19 and 20 ink, rows 18 and 21 white).
   (b) device y=40, thickness 8 px (rows 36..43 ink, rows 35 and 44 white), spanning x 120..180.
Direct render and Chrome re-raster of the SVG must agree on all of these.

## Verdicts

Rendered and inspected. The first apex has the long sharp miter, the second (limit 3 below the ratio 4.12) is cut flat; dashes with butt caps leave the 5 px gaps and
with square caps only 1 px gaps; the scale(4) group draws the StrokeScale::None line 2 px thick and the Normal one 8 px thick. The diff image shows only edge
antialiasing on the leg outlines. All probes hold in direct and Chrome. No bug.
