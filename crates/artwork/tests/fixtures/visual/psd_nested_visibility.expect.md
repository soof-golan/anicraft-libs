# psd_nested_visibility

Canvas 32x24, RGB 8-bit. Built by `tests/visual_psd.rs::nested_visibility` (bottom to top):

- `Paper` 0,0 32x24 white
- group `Body`
  - `torso` at (2,2) 10x10 red (200,30,30)
  - group `Arm`
    - `arm-fill` at (12,2) 6x10 green (30,160,30)
    - group `Ghost` (hidden)
      - `ghost-px` at (0,14) 8x8 blue (0,0,255), its own visible flag is on
    - `arm-hidden` at (20,2) 6x6 magenta, hidden flag
  - `empty` a 0x0 layer
  - `offcanvas` at (-20,-20) 10x10 yellow (entirely outside)
  - `straddle` at (28,20) 10x10 cyan (0,200,200): only 4x4 inside the canvas
- group `Deep` > `Deeper` > `Deepest` > `leaf` at (14,14) 4x4 black, plus a layer named `dup` at (20,14) 4x4 orange
  (255,128,0) in `Deepest` and another `dup` at (20,18) 4x4 purple (100,0,150) in `Deeper`

Also names: `名前`, a 60 character name, an emoji name and an empty name are layers in `Names` (all 1x1 at the
top-left, hidden so they do not paint).

## Layer tree

1. Root children, bottom first: `Paper`, `Body`, `Deep`, `Names`.
2. `Body` children: `torso`, `Arm`, `empty`, `offcanvas`, `straddle`.
3. `Arm` children: `arm-fill`, `Ghost`, `arm-hidden`. `Ghost` is hidden and keeps `ghost-px` (visible flag true) inside.
4. `arm-hidden` is hidden; `torso` is visible.
5. `empty` imports as a node (it exists, name kept) and paints nothing; it is not a raster with pixels.
6. `offcanvas` keeps offset (-20,-20); `straddle` keeps offset (28,20) and 10x10 pixels.
7. `Names` children keep every name exactly, including the 60 character one (the legacy Pascal name is cut at 31
   bytes) and the empty one.
8. Same tree when saved as PSB (large document).

## Composite pixels on white

| point | want |
|---|---|
| (5,5) | red (200,30,30) |
| (14,5) | green (30,160,30) |
| (3,16) | white: Ghost hides its child |
| (22,4) | white: arm-hidden is hidden |
| (0,0) | white: offcanvas does not reach the canvas |
| (30,22) | cyan (0,200,200): the straddling layer paints inside the canvas |
| (27,22) | white: left of the straddling layer |
| (15,15) | black (leaf) |
| (21,15) | orange (255,128,0) |
| (21,19) | purple (100,0,150) |
| (31,23) | cyan: last pixel of the canvas |

## Verdicts

Rendered and inspected at 12x (`/mnt/data/anim/scratch/visual/artwork/psd_nested_visibility@12x.png`): every
tree point and every pixel probe above matches on the first run, for PSD and PSB. The red torso meets the green arm
at x=12 with no gap, the cyan straddler shows only its 4x4 inside corner, orange sits above purple. No bug.
Locked in by `visual_psd.rs::nested_visibility` and `nested_visibility_as_psb`, golden `tests/golden/psd_nested_visibility.png`.
