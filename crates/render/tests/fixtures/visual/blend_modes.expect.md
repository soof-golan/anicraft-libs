# blend_modes (180x150, white background)

Verdict: all 28 populated cells match the hand-computed values within the stated tolerance (asserted in
tests/visual/blends.rs); the 5x render and grid crop were inspected: Alpha shows only the centre, Erase leaves a hole
with the ring intact, cells 23 and 24 are blank. No discrepancy.

Thirty cells of 30x30, 6 per row, in the order listed below. Each cell: a backdrop rectangle B = (200,100,50) filling the
cell, then a group with the blend mode containing an inset (4 px) rectangle S = (100,150,200). Cells for Alpha and Erase
wrap backdrop and blended group in a parent group with blend Layer, so there is a layer to alpha-mask or erase. Rows 1-4 are
opaque sources. Row 5 uses S with alpha 128 (0.502).

All numbers hand computed with the W3C compositing formulas for an opaque backdrop. In 0..1: B = (0.7843, 0.3922, 0.1961),
S = (0.3922, 0.5882, 0.7843), Lum(C) = 0.3 r + 0.59 g + 0.11 b: Lum(B) = 0.4883, Lum(S) = 0.5510, Sat(B) = 0.5882,
Sat(S) = 0.3922.

Probe the cell centre (inside the source), and for Alpha / Erase also the 2 px border ring.

| cell | mode | centre pixel | working |
|---|---|---|---|
| 0 | Normal | 100,150,200 | S |
| 1 | Multiply | 78,59,39 | B*S |
| 2 | Screen | 222,191,211 | B+S-BS |
| 3 | Overlay | 188,118,78 | HardLight(S as backdrop): r screen(0.3922, 2*0.7843-1) ; g 2*0.3922*0.5882 ; b 2*0.1961*0.7843 |
| 4 | Darken | 100,100,50 | min |
| 5 | Lighten | 200,150,200 | max |
| 6 | ColorDodge | 255,243,232 | B/(1-S): 1.29->1, 0.952, 0.909 |
| 7 | ColorBurn | 115,0,0 | 1-(1-B)/S: 0.45, 1-1.03 -> 0, 0 |
| 8 | HardLight | 157,127,167 | r mult(B, 2S)=0.6151; g screen(B, 2S-1)=0.4995; b 0.6532 |
| 9 | SoftLight | 191,111,86 | W3C: r 0.7474, g 0.4332, b 0.3369 |
| 10 | Difference | 100,50,150 | abs |
| 11 | Exclusion | 143,132,172 | B+S-2BS |
| 12 | Hue | 64,139,214 | SetLum(SetSat(S, Sat B), Lum B): sat part (0, 0.2941, 0.5882) lum 0.2382 -> +0.2501 |
| 13 | Saturation | 175,108,75 | SetLum(SetSat(B, Sat S), Lum B): (0.3922, 0.1307, 0) lum 0.1948 -> +0.2935 |
| 14 | Color | 84,134,184 | SetLum(S, Lum B): S + (0.4883-0.5510) |
| 15 | Luminosity | 216,116,66 | SetLum(B, Lum S): B + 0.0627 |
| 16 | Add | 255,250,250 | clamp(B+S) |
| 17 | Subtract | 100,0,0 | max(B-S, 0) |
| 18 | Invert | 55,155,205 | 1-B |
| 19 | Alpha | centre 200,100,50; border ring transparent (white on page) | parent keeps its pixels scaled by source alpha, loses the rest |
| 20 | Erase | centre transparent (white); ring 200,100,50 | parent loses what the source covers |
| 21 | Layer | 100,150,200 | like Normal |
| 22 | PassThrough | 100,150,200 | like Normal |
| 23-24 | (blank) | white | no cell |
| 25 | Multiply, S alpha 0.502 | 139,79,45 | 0.498 B + 0.502 BS |
| 26 | Screen, S alpha 0.502 | 211,146,131 | |
| 27 | Add, S alpha 0.502 | 250,175,150 | B + 0.502 S |
| 28 | Subtract, S alpha 0.502 | 150,50,25 | 0.498 B + 0.502 max(B-S,0) |
| 29 | Invert, S alpha 0.502 | 127,128,128 | 0.498 B + 0.502 (1-B) |

Layout: cells 0..22 fill rows 1-4 left to right; cells 23 and 24 stay blank; cells 25..29 are row 5. Tolerance 3 (4 for
SoftLight and the three non-separable modes, which take different intermediate roundings).
