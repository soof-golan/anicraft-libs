# mp4-edges

H.264 at a size that is NOT a multiple of the 16 px macroblock: 66x50 (needs a crop rectangle: coded 80x64), crf 0, 3 frames.
Content: background blue (0,0,255); the two right-most columns (x 64..66) green (0,255,0); the two bottom rows (y 48..50) magenta (255,0,255) (the bottom right
2x2 corner is magenta); a vertical 1 px white line at x = 15 from y 4..40 and a horizontal 1 px white line at y = 21 from x 20..60 (odd x and y, so each
sits in only one half of a chroma sample pair); a single white pixel at (40,30).

## Predictions

1. ffprobe reports `width=66 height=50` (coded 80x64 must be cropped away by the SPS cropping rectangle); decoding yields 66x50 pictures.
2. Decoded Y plane: pixel (65,5) is green's luma 173 (+-3), pixel (65,49) magenta's 78 (+-3), (30,49) 78, (30,30) is blue 32. The right-most column and bottom row are real
   picture content, not padding (padding replication would hide a missing crop: values at x=66..79 must not be visible at all since the width is 66).
3. The white line keeps its luma: (15,20) Y >= 215 and its horizontal neighbours (14,20), (16,20) Y <= 50 (a 1 px line is preserved by luma, which is not subsampled),
   the white pixel (40,30) Y >= 200.
4. Chroma is 4:2:0: the chroma of the line is shared with its neighbour column, so Cb at (14..16, 20) is the average of white (128) and blue (240), i.e. in 150..230,
   and strictly between the pure-blue and the white values. The picture 4 px away from any line is pure blue chroma (240, 118) +-3.
5. Frames 1 and 2 are identical to frame 0 (same content sent): decoded luma planes equal within +-2 everywhere, no drift.
6. RGB decode with ffmpeg (conversion with the BT.709 tag): the bottom right 2x2 is magenta within +-12, the corner (65,49) in particular.

## Verdicts

Inspected at 8x (`mp4-edges.png`). The picture is exactly 66x50 (crop rectangle honoured), the green right edge and magenta bottom edge are real content up to the last
pixel row and column including the corner, the 1 px lines and the single pixel keep full luma, and the pixel next to each line darkens to navy because its chroma
is shared with the white pixel of the 2x2 block: exactly the 4:2:0 behaviour predicted in point 4, appearing only on the even side of each odd-positioned line. Frames
do not drift. No bug.
