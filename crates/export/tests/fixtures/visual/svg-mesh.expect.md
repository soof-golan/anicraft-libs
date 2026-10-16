# svg-mesh

Stage 100x60, white background. One Mesh node: content = two opaque rects in the content's local space, red (200,40,40) on source x 0..20 and
blue (40,40,200) on source x 20..40, both y 0..40. Source quad (0,0) (40,0) (0,40) (40,40); target quad v0 (10,10), v1 (70,15), v2 (5,50), v3 (80,55);
triangles [0,1,2] and [1,3,2] (the two share the diagonal v1-v2 from (70,15) to (5,50)).

The warp is piecewise affine per triangle: p = v_a + s*(v_b - v_a) + t*(v_c - v_a).

## Predictions

1. Source (10,10) (tri 0, s=.25, t=.25) -> target (23.75, 21.25): red. Probe pixel (23,21).
2. Source (30,5) (tri 0, s=.75, t=.125) -> (54.4, 18.75): blue. Probe pixel (54,18).
3. Source (30,30) (tri 1) -> (58.75, 43.75): blue. Probe pixel (58,43).
4. Source (10,30): x+y = 40, on the shared diagonal -> target on the segment v1-v2, parameter .75 from v1... skip exact; use source (10,25) (tri 1: below the diagonal) red.
5. No seam: along the shared diagonal, pixels whose centres lie within the quad and within 0.3 px of the diagonal line (70,15)-(5,50) must still be an opaque mix of
   the content colours (alpha 255), i.e. NEVER show the white backdrop at more than 12 percent: every pixel on the diagonal, on the red or blue side, has all channels <= 235 and
   alpha equivalent to full coverage. Specifically the pixels (37,32), (30,36), (50,25), (60,20) are not "lighter than 80 percent coverage" in either the direct render or the Chrome render of the SVG.
6. Outside the target quad (2,2), (95,5), (50,58) is white.
Direct and Chrome renders agree on 1..4 and 6 (class red/blue/white); item 5 is the discriminating one: per-triangle `clip-path` antialiasing typically leaves a faint light line.

## Verdicts

1-4 and 6 held from the first render. Item 5 FAILED in the Chrome re-raster: the SVG draws each triangle as a clipped copy of the content, and the
two antialiased clip edges along the shared diagonal each cover half a pixel, so the backdrop shows through. Pixel (37,32) was (156,109,172), i.e. the white
backdrop at about 35 percent. Fixed in `svg/emit.rs`: an edge shared by two triangles is grown by one device pixel into the neighbour (`grown`). With the fix
every diagonal probe is saturated in Chrome and in the direct render. Verified the regression by switching the growth off again (test fails) and back on.
