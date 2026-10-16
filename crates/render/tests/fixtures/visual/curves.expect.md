# curves (300x100, white background)

Verdict: P1, P2, P4, P5 hold as written; P3 and P6 had too tight bounds (corrected below, with reasons). Render and the
circle-edge and S-curve crops were inspected: both discs round, S lobes meet at (250,50), cubic survives assembly.

Cell A (centre 50,50): circle R=40 made of four exact cubic segments (kappa 0.5522847498), fill1=1 black.
Cell B (centre 150,50): circle R=40 made of eight quadratic segments (control points at R/cos 22.5 deg).
Cell C (x 200..300): an S-shaped cubic (10,90) c1 (90,90) c2 (10,10) to (90,10) (offset +200 in x) closed by the
straight chord back to the start, fill black: two lobes on opposite sides of the chord, which meet at (250,50).
The same shape is also round-tripped through XFL attributes (to_xfl_edges, from_xfl) and rendered again.

## Predictions

- P1 A: every pixel whose centre is more than 1.0 px inside the circle (R=40) is black (ink >= 0.98), every pixel whose
  centre is more than 1.0 px outside is white (ink <= 0.02); the total ink equals the disc area pi R^2 = 5026.5 within
  0.25 per cent. The exact cubic circle is within 0.02 px of the true circle.
- P2 B: the same with a 1.2 px margin and 1 per cent on the area (the 8-quad circle bulges by R (1.003135-1) = 0.125 px
  at most).
- P3 A and B agree up to that bulge: their pixel-wise difference after shifting B by -100 px stays below 0.25 * 255 + 8 = 72
  per channel. (Original bound was 0.125 * 255 + 8 = 40; it was wrong. tiny-skia anti-aliases with 4 sub-scanlines, so
  coverage on a near-horizontal edge is quantised to steps of 0.25 and an edge that moved by only 0.125 px can flip one
  step. Observed: pixel (45,10) coverage 0.75 vs 0.50, exactly one step. Not a renderer bug: both circles are
  geometrically correct per P1 and P2.)
- P4 C: the filled lobes lie between the S curve and its chord. B(0.25) = (245, 77.5) (hand computed from
  (1-t)^3 P0 + 3(1-t)^2 t C1 + 3(1-t) t^2 C2 + t^3 P3), tangent (60,-90) so the unit normal is (0.832, 0.555).
  The point 2.5 px toward the chord, (242.9, 76.1), is black; 2.5 px away, (247.1, 78.9), is white. By the symmetry
  through (250,50): B(0.75) = (255, 22.5); (257.1, 23.9) black and (252.9, 21.1) white.
- P5 The assembled path of the S shape contains a CurveTo (the cubic survives; it is not flattened to lines).
- P6 After the XFL round trip (to_xfl_edges then from_xfl) the render is identical (max channel difference <= 2); with
  the `cubics` attributes removed (quadratic approximation only) it differs by <= 10 (anti-aliasing noise).
  Correction: the bound of 10 ignored the 0.25 coverage quantisation of the 4-sub-scanline anti-aliaser (see P3). The
  corrected prediction: max channel difference <= 66 (one coverage step), and fewer than 60 pixels differ by more than
  10, all on the S edge.
