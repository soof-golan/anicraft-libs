# psd_group_masks

Canvas 32x32 on white. Group masks are created by editing a built file (the builder has no group mask support).

- group `G1`, mask rect (0,0) 16x16, columns 0..8 are 255, columns 8..16 are 0, default 0. Contains `a` red
  (255,0,0) at (0,0) 16x16 and `b` blue (0,0,255) at (4,4) 12x12.
- group `G2`, mask rect (20,4) 4x4 all 0, default 255. Contains `c` green (0,160,0) at (16,0) 16x16.
- group `Outer` (mask: rect (0,16) 16x16, columns 0..8 are 255, rest 0, default 0) containing group `Inner`
  (mask: rect (0,16) 16x16, rows 0..8 are 255, rest 0, default 0) containing `d` black at (0,16) 16x16.
- group `EmptyMask`, mask with an empty rectangle (0 x 0) and default colour 0, containing `e` orange
  (255,128,0) at (16,16) 16x16. A mask that is 0 everywhere hides the whole group.
- group `FullMask`, mask with empty rectangle and default 255 containing `f` teal (0,128,128) at (0,0) 2x2
  (stays fully visible).

## Predictions

| point | want |
|---|---|
| (2,2) | red: a, inside the mask |
| (6,6) | blue: b over a, inside the mask |
| (9,2) | white: mask is 0 there |
| (12,12) | white: b is also cut by the mask |
| (17,1) | green |
| (21,5) | white: the hole of G2 |
| (24,5) | green again (hole is x 20..24, y 4..8) |
| (27,1) | green |
| (3,18) | black: both masks 255 |
| (10,18) | white: Outer cuts the columns |
| (3,26) | white: Inner cuts the rows |
| (20,20) | white: EmptyMask hides e |
| (0,0), (1,1) | teal (0,128,128): FullMask |

Tree: `Outer` and `Inner` each report a mask.

## Verdicts

First render: everything matched except `EmptyMask`.

BUG 4 (fixed): `e` (orange) was fully visible at (20,20); expected white. A group mask with an empty rectangle and
background 0 is zero everywhere, the importer logged "group mask too large, ignored" (wrong message too) and
dropped the mask. Fix: such a mask imports as a 1x1 transparent mask, which hides the whole group. Re-rendered: the
lower right quadrant is white, nested Outer/Inner masks intersect to the 8x8 black block, FullMask's teal 2x2 stays.
