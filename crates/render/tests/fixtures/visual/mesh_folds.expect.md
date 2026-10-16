# mesh_folds (280x70, transparent background)

Verdict: P1..P5 asserted in tests/visual/meshes.rs and passing; render and fold crops inspected: folded triangles
render without leaking, degenerate triangles are ignored (M1b identical to M1). No discrepancy.

Content (local space): four vertical stripes 10 px wide over x 0..40, y 0..40: red 255,0,0 / green 0,255,0 / blue
0,0,255 / yellow 255,255,0.

Source grid: vertices 0 (0,0), 1 (20,0), 2 (40,0), 3 (0,40), 4 (20,40), 5 (40,40). Left quad triangles [0,1,4], [0,4,3];
right quad triangles [1,2,5], [1,5,4].

Target vertices for the folded meshes: 0 (0,0), 1 (50,0), 2 (40,0), 3 (0,40), 4 (50,40), 5 (40,40). The middle column
moves right of the right column, so the right quad is turned inside out (mirrored, negative area) and lies over the end of
the left quad, x 40..50.

- M1 at translate(10,15): triangle order left quad first, then right quad.
- M2 at translate(100,15): the same mesh, right quad first, then left quad.
- M3 at translate(190,15): content = black -> white linear gradient over x 0..40; source quad (0,0),(40,0),(40,40),(0,40)
  in two triangles; target stretched 2x in x: (0,0),(80,0),(80,40),(0,40).
- M1b: M1's triangle list plus two bad triangles: [0,1,1] (zero area) and [0,1,99] (index out of range) at the end,
  rendered on its own canvas for comparison with M1.

## Predictions (mid height y = 35, i.e. mesh y 20; local x + mesh offset; tolerance 4)

- P1 M1: left quad maps source 0..20 onto target 0..50 (factor 2.5): red covers target x 0..25, green 25..50, but only
  x < 40 shows green because the mirrored right quad is painted after: it maps source 20..40 onto target 50..40:
  blue at target x 45..50, yellow at x 40..45. Pixels: x=10+10=20 red; x=10+35=45 green; x=10+42=52 yellow; x=10+48=58 blue;
  x=10+55=65 transparent (alpha 0).
- P2 M2: the left quad covers x 40..50 on top: x=10+42 and x=10+48 are green. x=20 red; x=45 green; x=65 transparent.
- P3 M3: target x = 2 * source x: gradient grey = source x / 40: target x 20 (pixel 190+20) -> source 10 -> 64; target x 60 -> 191;
  target x 40 -> 128. Beyond x=80: transparent. Tolerance 8.
- P4 M1b renders identically to M1 (max channel difference 0): bad triangles are ignored.
- P5 The mesh region is transparent above/below: (30, 5) and (30, 62) alpha 0; and the mirrored area does not leak left of
  x=10 or right of 10+50=60: pixel (62, 35) alpha 0 at M1.
