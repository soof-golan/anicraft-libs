# svg-gradients

Stage 240x160, white background. All gradients use `Affine::translate(cx,cy) * scale(s)` so that gradient space +-819.2
maps to +-819.2*s around the centre.

A. Linear, Pad, rect x 0..120 y 0..30, centre (60,15), s=0.05 (span x 19.04 .. 100.96, t = (x-19.04)/81.92),
   stops: 0 red(255,0,0), 0.5 green(0,255,0), 0.5 blue(0,0,255), 1.0 white. Hard stop at x=60.
B. Linear, Reflect, rect x 0..120 y 35..65, centre (60,50), s=0.01 (span 51.808 .. 68.192, 16.384 wide), red -> blue.
C. Same as B with Repeat, rect y 70..100.
D. Radial, Pad, focal 0.5, centre (180,50), s=0.05 (radius 40.96), white -> blue, painted into rect x 140..220, y 10..90.
E. Linear, rect x 0..120 y 105..135, centre (60,120), s=0.05: stop0 opaque red (255,0,0,255), stop1 FULLY TRANSPARENT blue (0,0,255,0).
   Colour and alpha interpolate independently (straight alpha, as SVG and Flash do).
F. Linear, rect x 130..230 y 105..135, centre (180,120), s=0.05, interpolation LinearRgb, red -> green.

## Predictions

A (pixel centres x+0.5):
1. x=10: red (pad). x=110: white (pad).
2. x=40 (t=0.2619): red*(1-0.5238)+green*0.5238 = (121,134,0) +-6.
3. x=58 (t=0.4757): (255*(1-0.9514), 255*0.9514, 0) = (12,243,0) +-6; x=61 (t=0.5123..): just after the hard stop: blue->white at (t-0.5)/0.5 = 0.0246+: (6,6,255) +-8.
   The transition between x=59 and x=61 is a single step, no green-blue blend column wider than one pixel.
B (reflect): x=52: t~0.0 red (255,0,0)+-10. x=76: t=(76.5-51.808)/16.384=1.507 -> reflect 0.493: (129,0,126) +-8.
   x=60: (127,0,128) +-8. x=84: t = 1.98 -> reflect 0.02 -> near red (250,0,5) +-12.
C (repeat): x=76 (t=1.507 -> 0.507): (126,0,129) +-8. x=84: t=1.98 -> 0.98 -> near blue (5,0,250) +-12.
D: focus F=(200.48,50). At (180.5,50) (ray from F through the pixel hits the circle at x=139.04, D=61.44, |P-F|=19.98): t=0.325:
   colour white*(0.675)+blue*(0.325)=(172,172,255) +-8. At (140,50): t~0.98: (5,5,255)+-12. Outside the circle (141,12)... pad -> blue.
   At the focal pixel (200,50): white-ish, (t~0.01) >= (240,240,255).
E (straight interpolation, over white): x=60 (t=0.5): colour (127,0,127), alpha 0.5, composite = (191,128,191) +-8.
   Premultiplied interpolation would give (255,127,127): that is wrong. x=15 (pad): red. x=105 (pad): fully transparent -> white.
F: x=180 (t=0.5): linearRGB mid-point: (188,188,0) +-6 (sRGB-space interpolation would give 128,128,0).
   x=135 (t=0.05+): near red.
Every probe must agree between the direct render and the SVG re-rasterised in Chrome within the stated tolerance.

## Verdicts

Rendered and inspected (direct, Chrome, diff). A: hard stop at x=60 is a single-pixel step; B reflect and C repeat show the predicted mirrored and sawtooth
ramps; D focal radial is brightest at the focus, right of centre; E interpolates straight alpha (red fading into white, no pink-grey premultiplied mid-tone);
F linearRGB is visibly brighter at the middle than sRGB would be. Probes match within tolerance in both renderers, and the diff image is black apart from
faint stop-boundary columns. No bug.
