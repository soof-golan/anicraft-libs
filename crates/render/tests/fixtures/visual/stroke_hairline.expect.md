# stroke_hairline (160x100, white background, black strokes, cap none)

Eight horizontal strokes from x=10 to 150 (device). Measure the coverage (sum of alpha/255) of a column at x=80 over a
window of +-3 rows around each line.

1. y=10.5, width 1: pixel-aligned.
2. y=20.0, width 1: straddles two rows.
3. y=30.5, width 0 (hairline).
4. y=40.0, width 0 (hairline) straddling two rows.
5. Group scale 4: local line y=13.125 (device 52.5), width 0: still one device pixel.
6. Group scale 4: local y=16.125 (device 64.5), width 1, scale mode None: one device pixel.
7. Group scale 4: local y=20 (device 80), width 1, scale mode Normal: 4 device pixels.
8. Group scale 4: local y=23.125 (device 92.5), width 0, scale mode Normal: still one device pixel.

## Predictions

- P1 coverage 1.0 (+-0.05); the pixel at (80,10) is 0,0,0 and (80,9), (80,11) are white.
- P2 coverage 1.0 (+-0.05); rows 19 and 20 are each about 50 per cent (alpha 128 +-6).
- P3 coverage 1.0 (+-0.15) in the single row 30; rows 29 and 31 below alpha 20.
- P4 coverage 1.0 (+-0.15) split over rows 39 and 40.
- P5 coverage 1.0 (+-0.15), row 52 dominant.
- P6 coverage 1.0 (+-0.05), row 64 fully black.
- P7 coverage 4.0 (+-0.05), rows 78..81 fully black, rows 77 and 82 white.
- P8 coverage 1.0 (+-0.15).
- P9 The line ends are flat (cap none): (9,10) white and (150,10) white.

## Verdicts

All points asserted in tests/visual/strokes.rs and passing; the 5x render and left-end crop inspected: hairlines stay
about 1 px at every position, sub-pixel offsets give grey two-row lines of equal total ink, the 4 px bar is solid.
No discrepancy.
