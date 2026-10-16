# png-alpha-scale

Stage 7x5 (deliberately not a multiple of anything), one frame. Content: a rect (1,1)-(4,4) of straight colour (200,100,50) alpha 128, over a TRANSPARENT stage,
a second variant over a Solid background (10,20,30).

## Predictions

1. scale 1.0, Transparent: output 7x5. Pixel (2,2) is straight (200,100,50) with alpha 128, each channel within +-3 (un-premultiplying 8-bit premultiplied values;
   it must not be the premultiplied (100,50,25)). Pixel (0,0) is (0,0,0,0).
2. scale 1.5: output ceil(10.5)=11 by ceil(7.5)=8. The rect covers 1.5..6.0 x 1.5..6.0: pixel (3,3) is the same straight colour, alpha 128.
   Pixel (1,1) covers 1..2 and the rect begins at 1.5, so it has quarter coverage: alpha 32 +-3 (colour still (200,100,50) +-8 since colour is straight).
3. scale 1.5 with ExportBackground::Solid((10,20,30)): the WHOLE 11x8 canvas is the background colour (the last column x=10 and last row y=7, which lie past the
   scaled stage edge 10.5 / 7.5, are NOT half-transparent): pixel (10,7) is (10,20,30,255) and pixel (10,0) is (10,20,30,255). Pixel (3,3) = 0.5*(200,100,50)+0.5*(10,20,30) = (105,60,40) +-2.
4. Stage-background: with the stage background (255,255,255) and scale 1.5 the last column is also opaque white (255,255,255,255) — a seam of 50 percent alpha at the right/bottom edge is a bug.
5. png sequence naming: 12 frames, prefix "walk_", start_number 1, min_digits 2: names walk_01..walk_12; 120 frames start_number 0 min_digits 1: 000..119 (padded to fit largest); start_number 98, 3 frames: 98, 99, 100 padded to 3 digits ("098","099","100").
6. frames range 5..99 on a 10-frame source clamps to 5..10 (5 files); range 12..20 on 10 frames errors with FrameOutOfRange.

## Verdicts

Points 1-6 hold (assertions in `tests/visual_bitmap.rs`): straight colour (200,100,50,128) at the rect, 11x8 output at scale 1.5, a fully painted background
including the overhang column and row, quarter-coverage edge alpha 32, and the sequence naming/clamping rules. No bug.
