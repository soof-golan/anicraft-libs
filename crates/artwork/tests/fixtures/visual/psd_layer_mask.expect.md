# psd_layer_mask

Canvas 32x24 on white. Every layer is solid black so the pixel value shows the mask value directly:
visible grey = 255 - mask.

- `ramp` at (0,0) 16x16 with a mask rect equal to the layer: mask(x,y) = 17*x
- `partial` at (16,0) 16x8, mask rect (20,2) 4x4 all 255, default colour 0 (outside the mask rect: hidden)
- `holey` at (16,8) 16x8, mask rect (24,10) 4x4 all 0, default colour 255 (outside: shown)
- `disabled` at (0,16) 8x8, mask all 0 but the mask is disabled: layer fully shown
- `feather` at (8,16) 8x8, mask rect same, left four columns 0, right four 255, with mask parameters
  feather 3.0 (a flag only; the stored raster is the authoritative mask data)
- `shifted` at (16,16) 8x8, mask rect (14,16) 8x8 whose local columns 0..4 are 0 and 4..8 are 255, default 0.
  Mask local column c is canvas column 14+c: shown for canvas x 18..22, hidden elsewhere (x 16,17 by data, 22,23
  because they are outside the mask rect where the default 0 applies)

## Predictions

| point | want |
|---|---|
| (0,5) | white (255) |
| (8,5) | 255-136 = 119 grey, tol 2 |
| (15,5) | black (0), tol 2 |
| (3,5) | 204 grey (255-51) tol 2 |
| (17,1) | white: outside partial's mask rect |
| (21,3) | black: inside |
| (23,5) | black: the last pixel of the mask rect |
| (24,3) | white: right of the mask rect |
| (21,1) | white: above the rect |
| (16,9) | black: default colour 255 shows the layer |
| (25,11) | white: the hole |
| (27,13) | white: last hole pixel; (28,13) black |
| (3,19) | black: disabled mask ignored |
| (9,19) | white; (11,19) white; (12,19) black; (15,19) black: hard edge at x=12 (the feather parameter does not blur the stored raster) |
| (16,20), (17,20) | white; (18,20) black; (21,20) black; (22,20), (23,20) white |

Tree outline: `partial`, `holey` rasters with the full 16x8 size (masks are baked into pixels, layer rect unchanged).

## Verdicts

All points matched on the first render (inspected `psd_layer_mask@12x.png`): a clean 16 step ramp, the
partial/hole rectangles to the pixel, the disabled mask ignored, the shifted mask offset applied correctly with the
default colour outside its rectangle. No bug. The feather parameter is not applied to the stored raster: this is a
choice, not something verified against Photoshop (we have no Photoshop to compare with); the raster in the file is
used as is, so there is no blur and no density scaling.
