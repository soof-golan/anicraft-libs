# gradients_linear (160x128, white background)

Verdict: all points asserted in tests/visual/gradients.rs and passing; the 5x render and hard-stop crop inspected: pad,
reflect and repeat bands, out-of-order stops sorted, coincident stops give a hard edge, linearRGB lighter than sRGB.
No discrepancy.

Eight rows of 16 px, each a rectangle x 0..160. Every gradient spans x 40..120 (gradient space -819.2..819.2 mapped
by translate(80,0) * scale(80/1638.4, 1)), so t = (x - 40) / 80 at pixel centre x+0.5. Default stops black(0) ->
white(1) unless stated. Rows 3 to 5 go through anim_shape::EdgeShape (XFL fill styles), which sorts stops.

0. Pad. 1. Reflect. 2. Repeat.
3. Pad, stops given out of order: (1.0 white), (0.0 black), (0.5 red).
4. Pad, hard stop: black 0, red 0.5, blue 0.5, white 1.
5. Pad, stops only cover 0.25..0.75: black at 0.25, white at 0.75.
6. Pad, interpolation linearRGB, black -> white.
7. Reflect, with the cell x range shifted so the ends are visible (same as row 1 but sampled further out).

Grey value g(t) = 255 t for default stops.

## Predictions (pixel centre x+0.5; tolerance 3 unless stated)

- P0 Pad: x=10 -> 0,0,0; x=150 -> 255,255,255; x=80: t=0.50625 -> 129.
- P1 Reflect: x=0: t=-0.49375 -> reflects to 0.49375 -> 126; x=150: t=1.38125 -> 2-1.38125 = 0.61875 -> 158;
  x=130: t=1.13125 -> 0.86875 -> 221.
- P2 Repeat: x=0: t=-0.49375 -> frac 0.50625 -> 129; x=150: 0.38125 -> 97; x=130: 0.13125 -> 33.
- P3 Out of order stops are sorted: black(0) red(0.5) white(1): x=60 (t=0.25625) -> r = 255*0.25625/0.5 = 131,0,0;
  x=100 (t=0.75625): red->white at 0.5125: 255, 131, 131.
- P4 Hard stop: x=76 (t=0.45625) -> black->red at 0.9125: 233,0,0. x=84 (t=0.55625): blue->white at 0.1125: 29,29,255.
  The step itself: pixel x=79 (t=0.4938) is reddish (r > 240, b < 20) and x=80 (t=0.50625) is bluish (b > 240, r < 20).
- P5 Partial stops: x=50 (t=0.1) -> 0,0,0 (pad before 0.25); x=110 (t=0.9) -> 255,255,255; x=80: t=0.50625 ->
  (0.50625-0.25)/0.5 = 0.5125 -> 131.
- P6 linearRGB: x=80: t=0.50625 interpolates in linear light: lin=0.50625 -> sRGB 0.7406 -> 189 (tolerance 4).
  Lighter than the sRGB row (129) at the same x.
- P7 (same as P1 at other x): x=20 (t=-0.24375) -> 0.24375 -> 62.
