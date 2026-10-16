# svg-fillrules

Stage 240x160, white background, y down. Fixture is built in `tests/visual.rs::fillrules`. Colour used: blue (30,60,200).

Row 1 (y 0..80):
- Pentagram, centre (40,40), outer radius 30, vertices at angle -90 + 72k degrees, path order 0,2,4,1,3, EVEN-ODD.
- Same pentagram, centre (120,40), NON-ZERO.
- Bow-tie (self-crossing quad) (170,10) (230,70) (230,10) (170,70), NON-ZERO.

Row 2 (y 90..150): square 60x60 with a 30x30 square inside, outer at x0+10..x0+70, y 90..150, inner x0+25..x0+55, y 105..135,
for x0 = 0, 80, 160.
- x0=0: EVEN-ODD, inner square same winding as outer.
- x0=80: NON-ZERO, inner same winding.
- x0=160: NON-ZERO, inner OPPOSITE winding.

## Predictions (direct render, then the SVG re-rasterised in headless Chrome)

Pentagram geometry: the inner pentagon has circumradius 30 * cos72/cos36 = 11.5 and inradius 9.3.
1. (40,40) white (even-odd centre is a hole). (40,40) + 6px toward a vertex, e.g. pixel (40,34): white.
2. Arm tip pixel toward the top vertex, (40,16) (distance 24 from centre): blue.
3. Concave valley at angle -54 deg, distance 22: pixel (53,22) (40+22cos(-54)=52.9, 40+22 sin(-54)=22.2): white.
4. Non-zero pentagram: centre (120,40) blue (winding number 2). (120,34) blue. (120,16) blue. Valley (133,22) white.
5. Bow-tie: left lobe pixel (180,40) blue, right lobe (220,40) blue; top (200,15) white, bottom (200,65) white
   (both lobes filled whatever the winding sign, because the two lobes have opposite winding but non-zero).
6. Squares: x0=0 centre (40,120) white, ring (15,120) blue. x0=80 centre (120,120) blue. x0=160 centre (200,120) white
   (opposite winding cancels to zero), ring (175,120) blue.
7. Outside every shape (5,85) stays white.
8. The SVG must contain `fill-rule="evenodd"` exactly once (the EO pentagram and EO square: twice) and the re-rasterised SVG must
   differ from the direct render at no more than 1.5 percent of pixels by more than 40 per channel (edge antialiasing only), and
   every probe above gives the same class (blue/white) in both.

## Verdicts

Rendered and inspected (direct, Chrome re-raster, diff, 8x crop of the pentagram). All 18 probes hold in both renderers: even-odd pentagram has its
central pentagon hole, non-zero pentagram is solid, the bow-tie lobes are filled and the gaps white, the opposite-winding square under non-zero is a hole while
the same-winding one is solid. `fill-rule="evenodd"` appears exactly twice. The diff image shows only one pixel wide antialiasing lines along the edges (under
1.5 percent of pixels over 40 per channel). No bug.
