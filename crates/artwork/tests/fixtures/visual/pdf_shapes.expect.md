# pdf_shapes

Hand-written PDF, one 200x100 pt page, all content inside one OCG layer `Shapes` (key `L1`). canvas y = 100 - pdf y.

1. Even-odd ring: `0 0 1 rg 10 10 60 60 re 25 25 30 30 re f*`. Outer rect x 10..70, inner x 25..55, pdf y 10..70 and
   25..55 (canvas y 30..90 and 45..75).
2. Non-zero "ring": `1 0 0 rg 100 10 60 60 re 115 25 30 30 re f`. Both `re` run the same direction, so non-zero
   fills the inner rectangle too.
3. Clip: `q 170 10 20 30 re W n 0 0.5 0 rg 165 5 30 80 re f Q`. Dark green only inside x 170..190, pdf y 10..40
   (canvas y 60..90).
4. Rotated bar: `q 0.7071 0.7071 -0.7071 0.7071 150 70 cm 0 0.6 0.6 rg 0 0 20 10 re f Q`: a 20x10 rectangle turned 45
   degrees about (150,70) pdf. Its corners (canvas): (150,30), (164.1,15.9), (157.1,8.8), (142.9,22.9).
5. Dashed stroke: `0 0 0 RG 4 w [6 6] 0 d 10 90 m 90 90 l S`: a 4 pt wide horizontal line at canvas y 10 (covers
   y 8..12) from x 10 to 90, butt caps, 6 on, 6 off starting at x=10.

## Predictions

| point (canvas) | want |
|---|---|
| (15,60) | blue (left side of the ring) |
| (40,60) | white: the even-odd hole |
| (40,35) | blue (top band, canvas y 30..45) |
| (130,60) | red: same-direction inner contour is filled by non-zero |
| (105,35) | red |
| (180,75) | dark green (0,128,0) |
| (180,50) | white: above the clip (green rect would reach canvas y 15..95) |
| (166,75) | white: left of the clip |
| (192,75) | white: right of the clip |
| (153,20) | teal (0,153,153) inside the rotated bar (its centre is about (153.5,19.4)) |
| (145,12) | white: inside the bar's bounding box, outside the bar |
| (13,10) | black (first dash x 10..16) |
| (19,10) | white (gap x 16..22) |
| (25,10) | black (dash x 22..28) |
| (31,10) | white |
| (13,5) | white: outside the 4 pt wide line |

Tree: `Shapes` is a layer; the ring is one vector object with an even-odd fill rule (read from the display nodes).

## Verdicts

Inspected `pdf_shapes@5x.png`: even-odd hole, same-direction non-zero ring filled, clip confines the green to
x 170..190, 45-degree bar has the predicted corners and antialiased edges, dashes alternate 6 on / 6 off from
x=10 with butt caps. All points match. No bug.
