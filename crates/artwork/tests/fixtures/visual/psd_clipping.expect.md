# psd_clipping

Canvas 32x16, white background from the renderer.

## Left cluster

- `base` at (2,2) 12x12: columns 0..6 opaque red, columns 6..12 fully transparent (so the shape is x 2..8, y 2..14)
- `paint` (clipping) at (0,0) 16x16, blue (0,0,255)
- `paint-b` (clipping) at (4,4) 6x6, yellow (255,255,0), layer opacity 128 (covers x 4..10, y 4..10)
- `free` (not clipping) at (0,13) 3x3 black, on top

Predictions:

| point | want |
|---|---|
| (3,3) | blue: inside the base shape |
| (5,5) | yellow at 50% over blue: (127,127,127), tol 3 |
| (8,3) | white: right of the base's opaque columns although base's rectangle contains it |
| (9,5) | white: `paint-b` reaches x=9 but the clip shape ends at x=8 |
| (1,1) | white: `paint` is larger than the base and is cut |
| (1,14) | black: `free` is not clipped, it paints in full |
| (5,12) | blue: still inside the base shape, below the yellow square |

## Right cluster

- `base2` at (18,2) 12x12 opaque green (0,160,0), layer opacity 128
- `paint2` (clipping) at (16,0) 16x16 blue (0,0,255)

Photoshop composites the clipping group (base plus layers clipped to it) as one unit and the base layer's opacity
applies to that whole group. The blue completely covers the green inside the base shape, so the group is blue,
shown at 50%:

| point | want |
|---|---|
| (24,8) | (127,127,255), tol 3 |
| (17,8) | white (left of the base shape) |
| (30,8) | white |

Tree: both `paint` layers import with `clipping == true`, `base`/`base2`/`free` with false.

## Verdicts

Left cluster matched on the first render (shape from the base's alpha, yellow 50% over blue, cut at the shape
edge, `free` unclipped).

BUG 3 (fixed): right cluster showed full-strength blue (0,0,255) at (24,8), expected (127,127,255). The importer drew
the base at its opacity but the clipped layers at 100%. Photoshop applies the base layer's opacity (and blend mode)
to the whole clipping group. Fix: `children_to_nodes` now puts the base's opacity and blend on the outer clipping
group and draws the base and its shape plain inside it; clipped layers that blend with the backdrop isolate the
group. Re-rendered: the right cluster is a uniform (127,127,255) square, left cluster unchanged.
