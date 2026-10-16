# sheet-layouts

Source: 7 frames, stage 24x16, fps 12, transparent stage. Frame i draws a solid rect of colour C_i (distinct, alpha 255) at size and offset:

| i | rect (x,y,w,h) | colour |
|---|---|---|
| 0 | (0,0,24,16) full frame | (255,0,0) |
| 1 | (4,3,10,6) | (0,255,0) |
| 2 | (20,12,4,4) bottom-right corner | (0,0,255) |
| 3 | nothing (fully transparent) | |
| 4 | (11,7,1,1) single pixel | (255,255,0) |
| 5 | (2,2,20,3) wide strip | (255,0,255) |
| 6 | (0,10,3,6) tall strip at left bottom | (0,255,255) |

Option sets: (a) Grid{columns:None}, padding 2, trim off; (b) Grid{columns:Some(3)}, padding 0, trim off; (c) MaxRects, padding 1, trim on;
(d) MaxRects, padding 0, trim on, power_of_two; (e) MaxRects trim on with max_size 12 (too small); (f) Grid max_size 20.

## Predictions

For every option set that succeeds:
1. Each frame entry `frames[name].frame` is a rect fully inside the sheet, rects do not overlap, and with padding p the gap between any two rects is >= p (in at least one axis).
2. Reconstructing frame i by compositing the sheet's `frame` rect at `spriteSourceSize` offset inside a `sourceSize` transparent canvas gives pixels IDENTICAL to the direct render of frame i.
3. `sourceSize` is always 24x16; `trimmed` is true exactly for frames whose rect is smaller than the stage (when trim on): 1,2,3?,4,5,6 true; frame 0 false.
   Frame 3 (empty) is stored as a single transparent pixel with `spriteSourceSize` (0,0,1,1).
4. Frame names are `frame_0001`.. `frame_0007` (start_number 1, min_digits 4), the `animations["all"]` lists them in order, and `meta.fps` = 12, `meta.size` equals the PNG size.
5. (a) Grid cols None, padding 2, no trim: cells 24x16; the column count minimises the sheet area (7 cells: 3 columns -> 3x3 grid area (3*24+2*2)*(3*16+2*2)=76*52=3952; 4 cols ->
   (4*24+6)*(2*16+2)=102*34=3468; 5 cols->(5*24+8)*(2*16+2)=128*34=4352; 7 cols -> 7*24+12=180 x16 = 2880 BEST) so expect 7 columns, one row, size 180x16 (if max_size 4096 allows).
   Frame i's rect x = i*(24+2).
   (b) 3 columns, padding 0: rect of frame 4 at (24,16), frame 6 at (0,32), sheet size 72x48.
6. (d) power_of_two: sheet sides are powers of two.
7. (e) fails with `ExportError::SheetTooSmall` (frame 0's trimmed content is 24 wide > 12). (f) Grid max_size 20 fails likewise.
8. No pixel on the sheet outside any `frame` rect is non-transparent (padding stays clear; no bleed from neighbours).

## Verdicts

Rendered the four successful layouts (`sheet-a..d`) and inspected them; rects do not overlap, padding stays clear, reconstructing every frame from its atlas
rect and `spriteSourceSize` reproduces the direct render exactly, the empty frame is a 1x1 transparent cell, power-of-two sides hold, and the too-small sheets
fail with SheetTooSmall. Numbers in the JSON (fps, names, sizes, animation list) match. No bug.
