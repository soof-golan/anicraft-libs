# gradients_radial (300x200, white background)

Verdict: all points asserted in tests/visual/gradients.rs and passing; the 3x render and focal crop inspected: pad,
reflect rings, repeat rings, focal offset, and elliptical axes (wide, tall) all as predicted. No discrepancy.

Six cells of 100x100, each a rectangle filling the cell, radial gradient red(0) -> blue(1), centre (50,50) in the cell.
Gradient space radius 819.2 is mapped to R px by scale(R/819.2). Colour at t: (255(1-t), 0, 255 t).

1. Pad, R=25, focal 0. 2. Reflect, R=25. 3. Repeat, R=25.
4. Pad, R=40, focal 0.5 (focal point at x = 50 + 0.5*40 = 70).
5. Pad, ellipse: matrix scale(40/819.2, 20/819.2): Rx=40, Ry=20.
6. Pad, ellipse rotated by 90 degrees: Rx=20 (along y... ) the matrix rotate(90deg) * scale(40/819.2, 20/819.2), so the
   long axis (40) lies along y and the short (20) along x.

## Predictions (pixel centre = x+0.5; tolerance 8)

- P1 Cell 1: pixel (62,50) centre (62.5,50.5): d=12.52 -> t=0.50 -> 127,0,128. Pixel (90,50): d=40.5>25 -> pad -> 0,0,255.
  Pixel (50,50): t=0.02 -> 250,0,5.
- P2 Cell 2 (x offset 100): pixel (190,50) = local (90,50): d=40.5 -> t=1.62 -> reflect 0.38 -> 158,0,97.
- P3 Cell 3 (x offset 200): pixel (290,50): t=1.62 -> repeat 0.62 -> 97,0,158.
- P4 Cell 4 (x offset 0, y offset 100): focal point (70,150) is red (t=0): pixel (70,150) r > 240. Along y=150 the colour at
  fraction of the way from focal point to the circle: (80,150): ray to the right edge (90) length 20, d=10.5 -> t=0.525 ->
  121,0,134. (40,150): left of focal by 29.5, ray hits edge at x=10: length 60 -> t=0.49 -> 130,0,125. The two points
  are equally "half way" though 9.5 px apart in distance from the centre: asymmetry check: red channel of (80,150) and
  (40,150) differ by < 15 while the distance from the centre to them are 30.5 and 9.5.
- P5 Cell 5 (x offset 100, y 100): ellipse Rx=40, Ry=20: (150,160) is d_y=10.5 -> t=0.525 -> 121,0,134; (170,150)
  is d_x=20.5 -> t=0.51 -> 125,0,130; both near the midpoint; (150,175) beyond Ry (d_y=25.5>20) -> 0,0,255.
- P6 Cell 6 (x offset 200, y 100): long axis along y: (260,150) is d_x=10.5 of Rx=20 -> t=0.525; (250,170) is d_y=20.5 of
  Ry=40 -> t=0.51; (250,195) beyond (d_y=45.5>40) -> 0,0,255; (275,150): d_x=25.5>20 -> 0,0,255.
