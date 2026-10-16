# svg-animated

8 frames, 80x40 stage, fps 10, background white. Frame i content (by design of the fixture, `tests/visual.rs::animated_source`):
- a red 10x10 square at x = X[i], y = 5, with X = [0, 8, 16, 24, 24, 40, 16, 56];
- frames 3 and 4 are IDENTICAL (a hold: both show the square at x=24); frame 6 repeats frame 2's picture (non-adjacent repeat, square at x=16);
- a blue 6x6 square at (70, 30) that exists only in frames 0 and 7 (appears, disappears, reappears).

Exports: SMIL (`SvgAnimation::Smil`) and CSS (`SvgAnimation::Css`), `merge_holds = true`, repeat on.

## Predictions

1. Document is valid XML (roxmltree parses it), `viewBox="0 0 80 40"`.
2. With merge_holds the hold frames 3+4 share one `<g>`: the number of `id="frameN"` groups is 7 (not 8). Frame 6 (repeat of frame 2) is NOT adjacent
   so it gets its own group (7 groups: frames 0,1,2,3-4,5,6,7).
3. Chrome at animation time t = (i + 0.5) / 10 s shows frame i: red square covers x in [8i, 8i+10), y in [5,15):
   probe pixel (X[i]+5, 10) red; for every other frame j whose square does not overlap (|X[j]-X[i]| >= 10), pixel (X[j]+5, 10) is white.
   Blue square pixel (72,32) is blue in frames 0 and 7 and white in frames 1..6.
4. Both SMIL and CSS variants show the same frame at the same time. (Chrome headless `--virtual-time-budget` drives the clock.)
5. With `repeat = false`, after the end (t = 1.2 s) the final frame stays (frame 7: red at x=56..66, blue visible).
6. Total duration attribute is 0.8s.

## Verdicts

Rendered with headless Chrome at the middle of each frame for SMIL and CSS (contact sheets `svg-animated-{smil,css}-contact.png`, one tile per frame).
Square and blue marker positions match the table for all 8 frames in both variants, the hold merges into one group (7 groups), the non-adjacent repeat of frame 2
keeps its own group, `repeat = false` leaves frame 7 visible after the end, and duration is 0.8s. No bug.
