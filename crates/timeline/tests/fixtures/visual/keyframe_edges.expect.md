# keyframe_edges: keyframes on frame 0 and on the last frame, gaps, tweens that cannot run

Written before rendering. Stage 100x100, a red 10x10 box symbol (`box`, graphic). Four layers in one 10-frame
scene (frame_count = 10 from the longest layer).

- `late`: kf0 (dur 9, classic tween) with instance at x = 0, kf9 (dur 1, the LAST frame of the scene) with the
  instance at x = 90, y = 10 for all layers below as noted. Linear: x(f) = 10 f for f = 0..9.
- `gap`: first keyframe at frame 3 (dur 4) at y = 30, a blank keyframe 7..9.
- `dangling`: kf0 dur 5 with a classic tween type and an instance at (0, 50), followed by a BLANK keyframe at 5
  (dur 5): the tween has no partner, so it holds.
- `short`: kf0 dur 1 classic tween then kf1 dur 1 (tween over a single frame), y = 70, x 0 -> 40, then nothing.

Frame count: `late` ends at 10, `gap` at 10, `dangling` at 10, `short` at 2.

- P1: `late` at f = 0..9 has translation x = 10 f exactly (to 1e-9), y = 10 flat; f = 9 is exactly the end keyframe.
- P2: `late` at f = 10, 11, 1000 and u32::MAX draws nothing (a layer does not extend past its last keyframe span),
  no panic.
- P3: `gap`: frames 0..2 blank (pixel (5, 35) WHITE), frames 3..6 box at x = 0 (pixel (5, 35) RED), frames 7..9
  blank.
- P4: `dangling`: frames 0..4 the box at (0, 50) held (not moved, not vanished): pixel (5, 55) RED at f = 2; frames
  5..9 blank.
- P5: `short`: f = 0 box at x = 0, f = 1 at x = 40 (the end keyframe), f = 2 nothing.
- P6: frame 9 pixel probes: `late` box at (90..100, 10..20): (95, 15) RED, (85, 15) WHITE.
- P7: scene_frames(0) = 10; scene_frames(5) = 0 (unknown scene), and `frame(5, 0)` is an empty stage.

## Verdicts

Rendered (keyframe_edges.png) and asserted. P1-P7 hold: a keyframe on frame 0 and one on the last frame tween
exactly, nothing is drawn past the last span (including u32::MAX), blank gaps and dangling tweens hold or vanish as
predicted. No disagreement.
