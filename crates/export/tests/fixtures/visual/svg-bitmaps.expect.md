# svg-bitmaps

Stage 120x60, background (30,30,30). Image: 2x2 premultiplied RGBA, row 0: red (255,0,0,255), green (0,255,0,255); row 1: blue (0,0,255,255),
half-transparent white stored premultiplied as (128,128,128,128) (straight colour (255,255,255), alpha 128/255).

1. Bitmap node, transform translate(5,5)*scale(10), smoothing=false: each image pixel becomes a 10x10 block, nearest neighbour.
2. Bitmap node, transform translate(65,5)*[-10,0,0,10,20+...] i.e. mirrored horizontally: x' = 85 - 10*x for image x (so image
   column 0 occupies x 75..85, column 1 occupies 65..75), smoothing=false.
3. Bitmap node rotated 90 degrees clockwise: transform translate(110,5) * rotate(pi/2) * scale(5) : image x axis maps to device +y, image y axis maps to device -x.
   Image pixel (c,r) covers device x in [110-5(r+1), 110-5r], y in [5+5c, 5+5(c+1)]. smoothing=false.
4. Bitmap fill (Paint::Bitmap, repeat = true, smoothing=false) of a 40x20 rect at (5,35), pattern transform translate(5,35)*scale(10): 2x2 image tiles of 20x20 px
   anchored at the rect corner, so the tile appears twice horizontally in the 40 px width and once vertically (20 px). (The pattern origin is the path's
   local origin, hence the translate; without it the tiles would start at x=0, y=0.)

## Predictions

1. (10,10) red, (20,10)?? no: block boundaries: red x 5..15, y 5..15; green x 15..25, y 5..15; blue x 5..15, y 15..25; half white x 15..25, y 15..25.
   Probes (10,10) red; (20,10) green; (10,20) blue; (20,20) = 0.5*white + 0.5*(30,30,30) = (143,143,143) +-3 (NOT dark grey from premultiplied misuse, which would be ~(79,79,79)).
   (14,10) red, (15,10) green: the edge between blocks is crisp (no blended pixel: nearest-neighbour, pixelated).
2. Mirrored: red now at x 75..85 (column 0), green at 65..75; probes (80,10) red, (70,10) green, (80,20) blue, (70,20) half-white (143,143,143).
3. Rotated: pixel (c=0,r=0) red covers x 105..110, y 5..10 -> probe (107,7) red. green (c=1,r=0): x 105..110, y 10..15 -> (107,12) green.
   blue (c=0,r=1): x 100..105, y 5..10 -> (102,7) blue. half-white (1,1): x 100..105, y 10..15 -> (102,12) (143,143,143).
4. Pattern tile: red at x 5..15,y 35..45 and again at x 25..35,y 35..45: probes (10,40) red, (30,40) red,
   (20,40) green, (40,40) green, (10,50) blue, (30,50) blue, (20,50) and (40,50) (143,143,143).
Direct render and Chrome re-raster must agree on every probe within +-4, and the number of pixels differing by more than 8 in any channel
must be 0 for the pixelated bitmaps in 1, 2 and 4, since all edges fall exactly on pixel boundaries.

## Verdicts

Rendered and inspected (direct, diff, zoomed). Nearest-neighbour blocks, mirrored copy, 90 degree rotated copy and the repeating pattern all land on the
predicted blocks; the half-transparent premultiplied white is (143,143,143) over the dark backdrop, not dark grey. The Chrome re-raster of the SVG differs from
the direct render in 0 pixels beyond 8 per channel for the pixelated cases. No bug.
