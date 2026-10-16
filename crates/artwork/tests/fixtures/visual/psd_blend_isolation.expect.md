# psd_blend_isolation

Canvas 32x16. `Base` is a full-canvas layer (200,100,50). Source colour for every blended layer is S=(100,150,200).

## Top half (y 0..8): eight 4 px wide strips, each a layer of 4x8 with a different blend mode, x = 4*i

Photoshop formulas on 8-bit values (b = base, s = source):

| i | mode | want (R,G,B) |
|---|---|---|
| 0 | Multiply: b*s/255 | (78,59,39) |
| 1 | Screen: 255-(255-b)(255-s)/255 | (222,191,211) |
| 2 | Difference: abs(b-s) | (100,50,150) |
| 3 | Linear Dodge (add), clamped | (255,250,250) |
| 4 | Overlay (driven by the base) | (188,118,78) |
| 5 | Darken | (100,100,50) |
| 6 | Lighten | (200,150,200) |
| 7 | Exclusion: b+s-2bs/255 | (143,132,172) |

Tolerance 3 per channel. Probe at the middle of each strip, y = 4.

## Bottom half (y 8..16): group isolation, 8 px wide cells

Every cell holds a Multiply layer of colour S over `Base`.

| cell | structure | want |
|---|---|---|
| x 0..8 | group with blend Normal containing the Multiply layer. A Normal group is isolated in Photoshop: the Multiply layer blends against the group's own (empty) backdrop, so it shows plain S | (100,150,200) |
| x 8..16 | Pass Through group with the same child. Pass Through lets the child see the canvas below: Multiply against Base | (78,59,39) |
| x 16..24 | group with blend Multiply containing a Normal layer of colour S: the group result is multiplied | (78,59,39) |
| x 24..32 | Pass Through group at opacity 128 containing the Multiply layer. The group's effect is scaled by its opacity: lerp(Base, Multiply result, 0.5) | (139,79,45), tol 4 |

Tree: Normal group imports with `blend == Normal`, the pass through group with `PassThrough`.

## Verdicts

Top half: all eight blend formulas matched on the first render. Bottom half, first render: cells x 0..8 and
x 24..32 were wrong.

- BUG 1 (fixed): the Normal group was not isolated. Cell 0 showed (78,59,39) (Multiply against the base) instead
  of (100,150,200). Cause: the renderer draws a Normal group straight onto its parent, and the importer mapped a
  PSD Normal group to `BlendMode::Normal`. Fix: a PSD Normal group that holds backdrop-blending content imports with
  `BlendMode::Layer` (isolated), cell 0 now shows (100,150,200).
- Cell 8..16 (pass through), 16..24 (Multiply group): matched.
- LIMITATION 2 (not fixed): a Pass Through group at opacity 128 holding a Multiply layer. Photoshop yields
  lerp(base, multiply, 0.5) = (139,79,45). Our display model has no "blend inside against the backdrop, then fade"
  group, so the group is drawn isolated: (150,125,125). The importer now says so in `warnings`. The test pins the
  approximation so a future renderer change shows up.
