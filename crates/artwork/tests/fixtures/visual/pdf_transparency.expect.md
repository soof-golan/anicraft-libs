# pdf_transparency

One 100x50 pt page. Layers (OCGs) `Back`, `Alpha`, `Mul`; graphics states `GS1 << /ca 0.5 >>` and
`GS2 << /BM /Multiply >>`.

- `Back`: cream (1,0.9,0.8) full page, plus a cyan (0,1,1) rectangle x 50..100, pdf y 0..50 (the right half)
- `Alpha`: `/GS1 gs 1 0 0 rg 10 10 30 30 re f`: red at 50% fill alpha over the cream (canvas x 10..40, y 10..40)
- `Mul`: `/GS2 gs 1 1 0 rg 60 10 30 30 re f`: yellow Multiply over the cyan (canvas x 60..90, y 10..40)

## Predictions

| point (canvas) | want |
|---|---|
| (5,5) | cream (255,230,204) |
| (20,20) | red 50% over cream: (255,115,102), tol 3 |
| (75,25) | yellow x cyan = (0,255,0), tol 2 |
| (95,45) | cyan (0,255,255) |
| (55,45) | cyan |
| (45,25) | cream |

The multiply is a blend of the object against what is below it, so the green only shows where the yellow square
meets the cyan.

## Verdicts

Inspected `pdf_transparency@8x.png`: salmon (255,115,102) for 50% red over cream, pure green where Multiply
yellow meets cyan, cream and cyan untouched elsewhere. All points match. No bug.
