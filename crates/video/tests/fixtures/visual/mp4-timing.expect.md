# mp4-timing

H.264 MP4 at 30000/1001 fps (29.97), 31 frames, 32x32. Frame i is a SOLID grey of value 8 * i + 8 (8 .. 248), so the decoded order identifies every frame.
Also: a one-frame MP4 at 24 fps, a 25 fps 50 frame MP4 with an I-frame request every 2 s, and a MOV (ProRes 4444) at 29.97 with 10 frames.

## Predictions

1. ffprobe: `nb_frames=31`, `r_frame_rate=30000/1001`, `avg_frame_rate=30000/1001`, video stream duration 31 * 1001 / 30000 = 1.03437 s (+-0.001).
2. `ffprobe -show_frames`: frame i has `pts_time` = i * 1001 / 30000 (+-0.0005), strictly increasing, no gaps, no duplicates.
3. Decoding all frames, frame i has luma 16 + 219 * (8 i + 8) / 255 (+-3 at crf 0): the order is the order sent, with exactly 31 frames decoded (not 30, not 32).
4. The one-frame file: nb_frames 1, duration 1/24 = 0.0417 s, decodes to one picture.
5. 25 fps file: nb_frames 50, duration 2.0 s, at least two sync frames (frames 0 and 50 cannot both be, so exactly one key at frame 0 for a 50 frame clip with
   keyint 50: `keyint = 2 s` means one at frame 0 only; with 100 frames there must be keys at 0 and 50). Asserted on the 100 frame variant.
6. MOV 29.97 ProRes: nb_frames 10, duration 10 * 1001 / 30000 = 0.33367 s, every frame is a key frame (`key_frame=1`), `codec_name=prores`, codec tag `ap4h`.
7. `Summary::duration` equals the ffprobe duration to within one tick of rounding, and `Summary::frames` equals nb_frames.

## Verdicts

1-4, 6 and 7 held at once: 31 frames at 30000/1001 give duration 1.03437 s, packet pts exactly i * 1001 / 30000, decoded luma identifies each frame in send order, the
single-frame file works, ProRes at 29.97 reports 10 frames and 0.33367 s with tag `ap4h`.

Point 5 was wrong as written: the 100 frame 25 fps clip has key frames at 0, 50 and also 64. The encoder adds a scene-cut key (the fixture is a +2 grey step per
frame, which its detector treats as a cut at frame 64); I had derived "keys only at multiples of 2 s" from the `keyint` setting, but `keyint` is a maximum, not a
schedule. The assertion is now: keys at 0 and 50, and never more than 50 frames without one. Not a bug.
