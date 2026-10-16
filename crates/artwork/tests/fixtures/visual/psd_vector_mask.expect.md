# psd_vector_mask

Canvas 24x24 on white. A vector mask (`vmsk` block) clips a layer to a path; path coordinates are fractions of the
document size.

- `square` : full-canvas black layer with a vector mask: one closed rectangle (0.25,0.25)-(0.75,0.75), which is
  canvas (6,6)-(18,18).
- Second document: `donut`: a black layer with two subpaths: the big rectangle (0.1..0.9) combined, and a small
  rectangle (0.4..0.6) with the subtract operation. The result is a frame with a hole at canvas (9.6..14.4).
- Third document: `inverted`: the flag "invert" set with the 0.25..0.75 rectangle: black outside, hole inside.
- Fourth: `disabled`: the disabled flag is set: the layer shows in full.

## Predictions

| document | point | want |
|---|---|---|
| square | (12,12) | black |
| square | (3,3) | white |
| square | (5,12) | white; (6,12) black; (17,12) black; (18,12) white |
| square | (12,5) | white; (12,6) black; (12,17) black; (12,18) white |
| donut | (4,12) | black (frame) |
| donut | (12,12) | white (hole) |
| donut | (1,12) | white (outside, 0.1*24 = 2.4: pixel 1 outside) |
| inverted | (12,12) | white; (2,2) black |
| disabled | (2,2), (12,12) | black |

Edge pixels at a fractional boundary (donut at 2.4, 9.6, 14.4, 21.6) are antialiased: only probe pixel centres
that are clearly inside or outside.

## Verdicts

BUG 5 (fixed): first render showed the whole 24x24 black layer: vector masks (`vmsk`/`vsms`, the mask every
Photoshop shape layer has) were ignored without a warning. Implemented in `src/vmask.rs`: the subpaths become an
`ArtMask::Vector` (combine paints, subtract erases, intersect keeps the overlap, invert flag, initial fill,
disabled flag). Re-rendered at 16x: the square is exactly x/y 6..18, the donut has a frame with a hole and
antialiased corners at the fractional 2.4/9.6/14.4/21.6 positions, the inverted one is black outside / white
inside, the disabled one is fully black. The "exclude" operation is approximated by combine and warns.
A layer with both a pixel mask and a vector mask: the pixel mask is baked, the vector mask clips (layers); for
groups only the pixel mask is kept, with a warning.
