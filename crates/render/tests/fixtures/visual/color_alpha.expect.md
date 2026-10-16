# color_alpha (180x120, white background)

Verdict: all points asserted in tests/visual/colors.rs and passing; render inspected (overlapping translucent squares
darken only where they overlap, transforms clamp as predicted). No discrepancy.

Six cells of 60x60 (3 columns, 2 rows). Colour transform out = in * mul + add on straight colour (unit space).
Probe the cell centre (cx, cy); backdrop is white.

1. Group A (alpha x0.5) > group B (mul rgb 0.5, add red 0.2) > fill (200,100,50) opaque.
2. Group A > group B (add alpha 0.25) > fill (200,100,50) with alpha 128.
3. Group (alpha x0.5) containing two overlapping red fills, each alpha 128: rect (10,10)-(40,40) and (25,25)-(55,55) of the
   cell.
4. Group (add alpha +1.0, mul 1) containing a fill (200,100,50) alpha 64; plus transparent area around.
5. Group (mul rgb 0, add rgb (1,0,0)) containing a blue fill: becomes pure red.
6. Group (mul rgb 2.0, add rgb 0.5) containing fill (200,100,50): channels clamp to 1.

## Predictions (tolerance 3)

- P1 After B: r = 200/255*0.5+0.2 = 0.592 -> 151, g = 100/255*0.5 = 0.196 -> 50, b = 50/255*0.5 -> 25. After A (alpha 0.5)
  over white: (151+255)/2 = 203, (50+255)/2 = 152.5, (25+255)/2 = 140. Pixel: 203,152,140.
- P2 alpha 128/255 = 0.502, plus 0.25 = 0.752. Straight colour unchanged (200,100,50); A multiplies by 1. Over white:
  r = 200*0.752+255*0.248 = 213.6; g = 100*0.752+63.2 = 138.4; b = 50*0.752+63.2 = 100.8. Pixel: 214,138,101.
- P3 Each group is composited as a unit: overlap alpha = 1-(1-0.502)^2 = 0.752, times 0.5 = 0.376 -> over white:
  255, 255*(1-0.376) = 159, 159. Non-overlapping parts: 0.251 -> 255,191,191. Pixels: overlap (cell 40,40 -> local
  (32,32)) 255,159,159; single (15,15) -> 255,191,191; (50,50) -> 255,191,191. (Non-isolated drawing would give 0.4375 for
  the overlap: 255,143,143.)
- P4 Transparent pixels stay transparent whatever the alpha offset (cell corner (2,2) is white, alpha stays 0); the
  fill, alpha 64 + 1.0 -> 1.0, is opaque 200,100,50.
- P5 Pure red 255,0,0 in the fill area.
- P6 r = min(1, 0.784*2+0.5) = 255, g = min(1, 0.392*2+0.5) = 255, b = 0.196*2+0.5 = 0.892 -> 227. Pixel 255,255,227.
