# mp4-colour-bars

64x48 H.264 MP4, 24 fps, 2 frames, `Quality { crf: 0 }` (as close to lossless as the encoder offers) and a second encode at the default crf 20.
Frame content (opaque, written as premultiplied RGBA with alpha 255), every colour block aligned to even pixels so 4:2:0 chroma is exact:
- rows 0..24: eight vertical bars, 8 px wide each: white, yellow, cyan, green, magenta, red, blue, black (the classic order).
- rows 24..48: a grey ramp in eight 8 px patches with values 0, 36, 73, 109, 146, 182, 219, 255.

## Predictions (BT.709, limited range: Y = 16 + 219 (0.2126 R + 0.7152 G + 0.0722 B) / 255, Cb/Cr = 128 + 224 (...) / 255)

Decoding to raw yuv420p with ffmpeg (no colour conversion, so the decoder cannot hide a wrong matrix):
1. Bar centre samples (Y, Cb, Cr) within +-2 at crf 0: white (235,128,128), yellow (219,16,138), cyan (188,154,16), green (173,42,26), magenta (78,214,230),
   red (63,102,240), blue (32,240,118), black (16,128,128).
2. Grey ramp Y = 16 + 219 v / 255: 16, 47, 80, 112, 144, 175, 205, 235 (+-2) and Cb = Cr = 128 (+-1).
3. The stream is TAGGED as BT.709 limited range: ffprobe reports `color_space=bt709` and `color_range=tv`. A 64x48 picture without tags is decoded by players
   with the BT.601 matrix (they guess by height), which turns saturated colours visibly wrong; the file must carry the tag because the encoder used the 709 matrix.
   Also `color_primaries=bt709` and `color_transfer=bt709`.
4. Decoding to RGB with ffmpeg's own conversion (honouring the tags) gives, per bar, the source colour within +-6 per channel at crf 0 (red (255,0,0), etc.)
   and white is (255,255,255) within +-3 (not 235: the limited range is expanded).
5. ffprobe: codec h264, profile High, pix_fmt yuv420p, 64x48, nb_frames 2, no audio stream.
6. At crf 20 the bars stay within +-8 per channel in RGB at the bar centres.
7. Footprint: both files play (no ffmpeg decode errors on stderr at `-v error`).

## Verdicts

Encoded and decoded with ffmpeg/ffprobe; picture inspected at 6x (`mp4-colour-bars-crf0.png`: bars in the right order, ramp monotone, white and black at the
extremes). Points 1-7 all hold at crf 0 and crf 20: raw yuv420p samples match the BT.709 limited-range values within +-2, the stream is tagged bt709/tv for
matrix, primaries and transfer (so players do not fall back to BT.601 for a small picture), and the RGB conversion gives pure colours and full-range white.
No bug.
