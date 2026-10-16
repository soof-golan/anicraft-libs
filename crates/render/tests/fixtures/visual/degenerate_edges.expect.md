# degenerate_edges (200x140, white background) - XFL edges through EdgeShape::to_display

Verdict: P1..P7 asserted in tests/visual/fills.rs and passing; render and the dot / G2 crops inspected: round-cap dot
is a disc, square-cap dot a square, zero-area and lone-point shapes draw nothing, no spikes from coincident segments.
Found by this fixture: a boundary listed twice (or once per side) became two coincident contours that even-odd
cancelled into a hole; fixed in anim-shape (drop_duplicates in assemble.rs) with a unit test.

Row 1 (y 0..70): three zero-length stroke edges "!x 50|x 50", width 20, black, caps round (x=30), square (x=90),
none (x=150).

Row 2 (y 80..120):
- G1 (10,80)-(50,120): red square whose single edge (fill1=1, clockwise) is listed twice in the file (an exact
  duplicate).
- G2 (70,80)-(110,120): green square from one fill-only edge, plus a second stroke-only edge with the very same
  segments (black, width 4, square joins): coincident geometry with different roles.
- G3 (130,80)-(170,120): blue square whose edge text repeats vertices, giving zero-length segments at every corner.
- G4 (180..190, 80): a fill region whose three points are collinear (zero area), fill1=1 red, no stroke; and a lone
  "!185 100" edge (move only) with a stroke.

## Predictions

- P1 Round cap dot: (30,50) black, (37,50) black, (22,42) white (corner of the box, 11.3 px away, outside radius 10),
  (42,50) white.
- P2 Square cap dot is a 20x20 square (80..100, 40..60): (90,50) black, (82,42) black, (97,57) black, (78,50) white.
- P3 Butt cap: nothing is drawn: (150,50) white.
- P4 G1: duplicates must not cancel under even-odd: (30,100) red, (12,82) red.
- P5 G2: fill (90,100) green 0,160,0; stroke centred on the boundary: (70,100) and (71,100) black (the stroke covers
  x 68..72), (74,100) green; the stroke ring is closed at the corners: (69,79) black (round joins by default
  style, so the outer corner pixel is covered at least 50 per cent). A fill-only edge and a stroke-only edge on the same segments do not cancel.
- P6 G3: (150,100) blue, corners sharp: (130,80) and (169,119) pure blue 0,0,255.
- P7 G4: nothing drawn for the zero-area region or the lone point: pixel (185,80) and (185,100) white.
