# eases: custom bezier chains, named presets, classic acceleration, per-property targets

Written before rendering. A red 10x10 box (graphic symbol) tweens x from 0 to 100 over frames 0..10 on one layer
(end keyframe at 10); progress p = f / 10. Position x(f) = 100 * ease(p).

## ease_custom_chain

Custom ease with 7 points (two cubic segments): anchors (0,0), (0.5,1.2), (1,1); controls (0.1,0), (0.2,1.2) for
the first segment and (0.6,1.2), (0.9,1.0) for the second. Points are `[time, value]`.

- P1: segment boundary is an anchor: at p = 0.5 the eased value is exactly 1.2, so x(5) = 120.0 (tolerance 0.05).
- P2: the ease overshoots: some frame has x > 100, and x(10) = 100.0 exactly, x(0) = 0.
- P3: for every frame the value matches an independent solve (bisection on the cubic's time coordinate, then its
  value coordinate) of the right segment to 0.05 px.
- P4: the ease is continuous across the anchor: x(5) is between x(4) and x(6) inclusive of overshoot only if the
  curve is monotone there; with these controls x(4) < x(5).

## ease_named

Standard (easings.net) formulas, evaluated at p = 0.3, 0.5, 0.7 (frames 3, 5, 7):

- P5: quartInOut: 0.0648 at 0.3, 0.5 at 0.5, 0.9352 at 0.7.
- P6: backOut at 0.5: 1 + 2.70158 * (-0.5)^3 + 1.70158 * (-0.5)^2 = 1.0876975.
- P7: bounceOut at 0.5: 7.5625 * (0.5 - 1.5/2.75)^2 + 0.75 = 0.765625.
- P8: elasticOut at 0.5: 2^-5 * sin((5 - 0.75) * 2 pi / 3) + 1 = 1.015625.
- P9: sineInOut at 0.5 is 0.5; cubicIn at 0.5 is 0.125; expoOut at 0.5 is 1 - 2^-5 = 0.96875.
- P10: at p = 0 and p = 1 every preset is exactly 0 and 1 (no overshoot residue at the ends).

## ease_classic_acceleration

- P11: acceleration +50 at p = 0.5: p + 0.5 p (1 - p) = 0.625 -> x = 62.5.
- P12: acceleration -50 at p = 0.5: 0.375 -> x = 37.5; acceleration 0: 50.
- P13: a custom ease on the keyframe overrides acceleration.

## ease_targets

Box also rotates auto 0 -> 90 degrees; `All` = cubicIn, `Position` = linear.

- P14: at f = 5 x = 50 (Position linear wins over All) while the rotation is 90 * 0.125 = 11.25 degrees (All
  applies to rotation).
- P15: a `Scale` target with no entry and no `All` falls back to the classic acceleration.

## Verdicts

All P1-P15 hold and are asserted numerically (no pixels needed). No disagreement.
