# pdf_ocg_layers

Hand-written PDF, one 100x100 pt page, five optional content groups (OCGs) used as layers. The viewer default
configuration `/D` has `/Order [Background Mid Hidden Top [Sub]]` and `/OFF [Hidden]`.

Layers (property name, layer name):

| key | name |
|---|---|
| L1 | `Background` |
| L2 | `Mid ✓ עברית` (a UTF-16BE string with byte order mark: check mark and Hebrew) |
| L3 | `Hidden` |
| L4 | `Top` |
| L5 | `Sub` |

Content stream, in paint order (PDF coordinates, origin bottom left; canvas y = 100 - pdf y):

1. `/OC /L1 BDC` cream (1,0.9,0.8) rectangle covering the page
2. `/OC /L2 BDC` blue (0,0,1) rectangle x 10..50, pdf y 10..50 (canvas y 50..90)
3. `/OC /L3 BDC` green (0,1,0) rectangle x 30..70, pdf y 30..70 (canvas y 30..70)
4. `/OC /L4 BDC` red (1,0,0) rectangle x 60..90, pdf y 60..90 (canvas y 10..40), and inside it
   - `/OC /L5 BDC` magenta (1,0,1) x 70..80, pdf y 70..80 (canvas y 20..30) `EMC`
   - `/Span <</MCID 0>> BDC` black x 62..66, pdf y 62..66 (canvas y 34..38) `EMC` (plain marked content, no layer)
   - `EMC`
5. `/OC /L2 BDC` yellow (1,1,0) x 10..20, pdf y 60..70 (canvas y 30..40) `EMC` (the same layer again, later in the stream)

## Predictions

Tree:

- every OCG is a layer with exactly the name above, including the check mark and the Hebrew word
- `Hidden` imports with `visible == false`, the others visible
- the layer `Mid ...` exists once, holding both of its rectangles (blue and yellow), not two layers
- `Top` holds the red rectangle; the black span rectangle is part of `Top`, not a layer of its own
- the canvas is 100x100

Pixels on white (display list leaves out hidden layers):

| point (canvas) | want |
|---|---|
| (5,5) | cream (255,230,204) |
| (20,80) | blue (0,0,255) |
| (40,60) | blue: the hidden green layer does not paint over it |
| (60,40) | cream: the hidden green square would cover (60,40) (x 30..70, y 30..70) |
| (85,20) | red |
| (75,25) | magenta |
| (63,36) | black |
| (15,35) | yellow (255,255,0) |
| (95,95) | cream |

Not predicted: how the importer nests `Sub` under `Top` (the nested array in `/Order`); the test only requires that
`Sub` exists and is findable by name.

## Verdicts

Inspected `pdf_ocg_layers@6x.png`: every pixel point matches (cream background, blue square without the hidden green
one over it, red with magenta inner square and the black span square inside, yellow square). Tree assertions
(names with check mark and Hebrew, `Hidden` invisible, `Mid` single layer with two rectangles, span merged into
`Top`) pass. No bug.
