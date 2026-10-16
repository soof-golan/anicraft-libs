# bitmap_fills (three canvases, white background)

Verdict: every point below is an assertion in tests/visual/bitmaps.rs and passes; all four renders and the 8x tile crops
were opened and inspected (hard texel blocks with smoothing off, bilinear blend with smoothing on, rotated tiling
seamless, clamp edges as predicted). No discrepancy.

Image: 4x4 opaque texels, texel (i, j) (column i, row j) has colour (60 i + 30, 60 j + 30, 128). 16 distinct colours.

Canvas A (160x100): rect (0,0)-(160,100) filled with a repeating bitmap paint, smoothing off, matrix
translate(100,20) * rotate(90 deg) * scale(10): each texel is a 10x10 px block, the image rotated a quarter turn
clockwise (x axis of the image points down the screen). rotate(90 deg) maps (x,y) -> (-y, x).
Canvas B (160x100): the same bitmap with smoothing off, rotated by 30 degrees (matrix translate(80,50) *
rotate(30 deg) * scale(7)), repeating, rect fills the canvas.
Canvas C (160x100): the same as A but smoothing on (bilinear).
Canvas D (100x100): bitmap not repeated (clamped), matrix translate(30,30) * scale(10), smoothing off, rect (0,0)-(100,100).

## Predictions

- PA The texel (i,j) occupies device x 100-10(j+1)..100-10j, y 20+10i..20+10(i+1) (and the same plus multiples of 40
  in each direction). Its centre (95-10j, 25+10i) has colour (60i+30, 60j+30, 128) exactly (tolerance 0):
  (95,25) = (30,30,128); (85,55) i=3,j=1 -> (210,90,128); (55,25) is the next tile, texel (0,0) again; (95,65) the tile
  below, texel (0,0) again.
- PA2 With nearest sampling every pixel of the canvas is exactly one of the 16 texel colours: zero off-palette pixels.
- PB Rotated by 30 degrees with nearest sampling still gives zero off-palette pixels over the whole canvas, and all 16
  palette colours occur.
- PC Smoothed (bilinear) rendering of A has off-palette pixels along every texel boundary: more than 500 of them, but the
  centres of interior texels still equal the palette colour. (Prediction corrected before it was checked against
  code: the first draft probed texel (0,0) with tolerance 3. A pixel centre is 0.5 px = 0.05 texel from the texel centre
  and picks up 5 per cent of a neighbour; texel (0,0) at the tile corner has the wrapped texel (3,3 side) as neighbour,
  180 apart per channel, so up to 9 off. Interior texels (1,1), (2,2), (1,2) differ from neighbours by 60: at most 3 off,
  tolerance 8.)
- PD Clamp: the area outside the bitmap takes the nearest edge texel: (5,5) -> texel (0,0) = (30,30,128); (95,95) ->
  texel (3,3) = (210,210,128); (5,50) -> texel (0,2) = (30,150,128); (50,5) -> texel (2,0) = (150,30,128); inside,
  (35,35) -> texel (0,0); (65,65) -> texel (3,3). No white anywhere (the fill covers the whole canvas).
