# stroke_scale (340x120, white background)

Verdict: all points asserted in tests/visual/strokes.rs and passing; render and corner crop inspected: scale mode
normal thickens only one axis, none keeps uniform width under the non-uniform scale, corners stay mitred. No
discrepancy.

Four cells of 170x60. In each, a rectangle outline with local corners (4,20)-(48,100) is drawn under the group matrix
translate(cell) * scale(3, 0.5), so it lands on device x 12..144, y 10..50 inside its cell. Width 6, black, cap none,
miter joins. Cells: (0,0) scale mode Normal, (170,0) None, (0,60) Horizontal, (170,60) Vertical.

"Horizontal" is Animate's "do not scale thickness horizontally" (SWF NoHScale); "Vertical" the other axis.

Measures: V = coverage (sum of alpha/255) of the row at the cell's y+30 over the cell's x..x+30 (the left vertical
bar, centred on x=12: a horizontal measure of its thickness). H = coverage of the column at the cell's x+80 over the
cell's y..y+30 (the top horizontal bar, centred on y=10: a vertical measure of its thickness).

## Predictions

| mode       | V (vertical bar width) | H (horizontal bar height) | why |
|------------|-----------------------:|--------------------------:|-----|
| Normal     | 18 | 3 | pen scales with the object: 6*3 and 6*0.5 |
| None       |  6 | 6 | pen is 6 device pixels in both directions |
| Horizontal |  6 | 3 | horizontal scale (3) is not applied to the thickness; vertical scale (0.5) is |
| Vertical   | 18 | 6 | vertical scale (0.5) is not applied; horizontal scale (3) is |

Tolerance 0.25 on every measure.
- P1 The geometry itself (the centre lines) does not depend on the mode: the left bar's centre is at x=12 in every cell,
  so the coverage-weighted centroid of the V row is 12.0 +-0.1 (cell offset subtracted), likewise the top bar's centroid
  in y is 10.0 +-0.1 for every mode.
- P2 The corners are mitred (sharp, not rounded): in the Normal cell the outer corner spans x 3..21, y 8.5..11.5, so
  pixel (cell.x+3, cell.y+9) is opaque black (a round join would leave it with alpha well below 250).
