# svg-groups

Stage 240x120, background (200,230,250) (light blue). Every shape sits in a Group.

A. (x 10..70, y 10..60) Group with ColorTransform::alpha(0.5) containing an opaque red rect (10,10,40x40 local = abs 10..50)
   and an overlapping opaque blue rect (30..70, 30..70 abs... i.e. children in absolute coordinates, group transform identity).
   Group alpha applies to the flattened group, NOT per child: in the overlap you see blue at 50 percent over the backdrop, never red.
B. Group with mask: mask = one rect (90..130 x 10..50) with alpha 128; child = opaque green (0,160,0) rect 80..140 x 0..60.
   The display-list contract: "drawn only where the mask's alpha is non-zero". The mask is a coverage mask, not a fractional multiplier.
C. Group with BlendMode::Multiply, child opaque (255,128,0) rect 150..200 x 10..50 over the backdrop (200,230,250).
D. Group with ColorTransform::tint((255,0,255), 0.3) and child opaque green (0,255,0) rect 10..50 x 80..110.
E. Group transform rotate(30deg about (120,95)) skew-free, child rect 100..140 x 85..105 solid (120,0,0), no filter.
F. Group with Filter::Blur{ x:3, y:3, quality 3 } of an opaque black rect 190..230 x 80..110.

## Predictions

A. Backdrop pixel (5,5) = (200,230,250). Red-only region pixel (15,15): 0.5*(255,0,0)+0.5*bg = (228,115,125) +-3.
   Overlap pixel (40,40): 0.5*(0,0,255)+0.5*bg = (100,115,252) +-3 (it must NOT be a mix containing red).
   Blue-only pixel (60,60): (100,115,252) +-3.
B. Pixel (110,30) inside mask: green (0,160,0) at full strength (binary coverage mask) = (0,160,0) +-3.
   Pixel (85,30) outside the mask rect: backdrop. Pixel (110,5): backdrop.
   (If the renderer multiplies by the mask alpha instead, (110,30) would be (100,195,125); this is the failure to look for.)
C. Pixel (170,30) = multiply: (255*200/255, 128*230/255, 0*250/255) = (200,115,0) +-3. Pixel outside = backdrop.
D. tint 0.3 of (255,0,255): green (0,255,0) -> 0.7*(0,255,0)+0.3*(255,0,255) = (77,179,77) +-4. Pixel (30,95).
E. Pixel at the group's rotated centre (120,95) = (120,0,0). The unrotated corner (101,86) (top-left of the unrotated rect) is backdrop
   because rotation by +30 deg (clockwise on a y-down stage) moved that corner to around (104.9, 71.2)... compute: corner offset (-19,-9)
   -> rotated (-19cos30 + 9 sin30, -19 sin30 - 9 cos30) = (-11.95, -17.3) => (108.05, 77.7). So (101,86) is outside: backdrop,
   and (108.5,78.5) is just inside the rotated corner: dark red.
F. Blur sigma 3 of a rect: centre (210,95) fully black (0,0,0)+-6. Edge column x=190 (rect edge at 190.0): ~50 percent -> pixel (190,95) = mid between black and
   backdrop, 100+-25 on the red channel; pixel (180,95) lighter than (185,95) lighter than (189,95) monotone; pixel (170,95) is backdrop +-4.
Chrome's re-raster of the SVG must reproduce A..E within +-6 and F within +-25.

## Verdicts

Rendered and inspected. A: the overlap shows blue only, so group alpha applies to the flattened group. C multiply and D tint match. E: rotated rectangle, its
corner pixels fall where computed. F: blur is monotone, with the 36/64 percent coverage values of three box passes of width 3.

B was wrong in the prediction, not in the code. I derived "binary coverage" from the doc comment on `Group::mask` ("drawn only where the mask's alpha is
non-zero"), but `anim-render` documents and implements that the mask's alpha multiplies the content (group.rs, `MaskType::Alpha`), and the SVG export emits
`mask-type: alpha`. Both agree: (110,30) is 50 percent green over the backdrop, (100,195,125). The wording of the display-list comment is the loose part; the
renderer is the reference. The test asserts the multiplying behaviour. No export bug.
