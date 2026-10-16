# psd_opacity

Canvas 24x16, no paper layer: everything is composited on white by the test renderer. Layers (bottom to top):

- `L50` at (0,0) 8x8 black, layer opacity 128
- `F50` at (8,0) 8x8 black, fill opacity 128
- `both` at (16,0) 8x8 black, opacity 128 and fill opacity 128 (they multiply: 0.25)
- group `G50` opacity 128 containing `a` red at (0,8) 8x8 and `b` blue at (4,8) 8x8 (overlap x 4..8)
- `zero` at (16,8) 8x8 black, opacity 0

## Predictions

An isolated group composites its content first and then applies its opacity once, so overlapping children do not
add up.

| point | want |
|---|---|
| (3,3) | black at 50%: 255*(1-128/255) = 127 grey, tol 2 |
| (11,3) | same 127 grey (fill opacity acts like opacity for plain pixels) |
| (19,3) | black at 0.25 coverage (0.502 * 0.502): 255 * (1 - 0.252) = 191 grey, tol 3 |
| (2,12) | `a` only, group 50%: (255,127,127) |
| (6,12) | overlap: `b` covers `a` before the group opacity: (127,127,255). Not a mix of a and b. |
| (10,12) | `b` only: (127,127,255) |
| (20,12) | white: opacity 0 |
| (14,12) | white: nothing there |

Tree: `G50.opacity == 128/255`, `L50.opacity == 128/255`, `F50` combined opacity 128/255 (fill folded in), `both`
0.25, `zero` 0.0.

## Verdicts

All points match (inspected `psd_opacity@16x.png`): the overlap cell is pure blue-at-50%, not a mixture with the
red below, so the group is composited in isolation before its opacity. Fill opacity is folded into the node
opacity (0.25 for `both`). No bug. Note: for a group, fill opacity is ignored (Photoshop only has it on layers
that draw pixels).
