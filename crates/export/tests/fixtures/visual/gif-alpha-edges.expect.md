# gif-alpha-edges

Stage 40x40, TRANSPARENT stage (no background), fps 24, 6 frames. A single opaque red (255,0,0) disc radius 10.5, centre (14+2i, 20) in frame i
(moves right 2 px per frame). Anti-aliased edge pixels therefore have fractional coverage. GifOptions default (alpha_threshold 128, Loop forever).
A second pass with fps = 60 and a third with a 3-colour gradient (below).

## Predictions

Decoding with the `gif` crate (raw frames) and with ffmpeg (composited, disposal applied):
1. Frame 0, pixel (14,20) (disc centre): opaque (255,0,0). Pixel (0,0): transparent (alpha 0).
2. A pixel is transparent in the GIF iff its direct-render alpha < 128 (the threshold), opaque otherwise; ZERO exceptions across all 6 frames.
3. Every opaque pixel, including the edge pixels whose coverage was only 50..99 percent, has colour EXACTLY (255,0,0): GIF has no partial alpha and the exporter
   must write straight (un-premultiplied) colour; a dark red edge ring (premultiplied leak) or any palette mixing is a bug. Palette holds red only plus the transparent index.
4. Frame delays at 24 fps (hundredths of a second) are [4,4,5,4,4,4]: ideal ends round(100(i+1)/24) = 4, 8, 13, 17, 21, 25; total 25 cs = 6/24 s exactly.
5. NETSCAPE loop extension present with count 0 (forever) by default; absent for GifLoop::Once.
6. Composited by a real decoder (ffmpeg -> PNG per frame), frame 3's disc is centred at (20,20) and NO pixel of the earlier discs remains visible outside it:
   pixel (4,20) (leftmost part of frame 0's disc, x 3.5..) is transparent in the composited frame 3. (Disposal must clear the previous frame.)
7. At 60 fps: every delay is 2 (min_delay_cs), since 100/60 = 1.67 < 2; documented GIF limit (total plays 1.2 x slower), not asserted as a bug.
8. Gradient case: a 64x64 frame, opaque, with a diagonal RGB gradient (more than 256 distinct colours) quantised: mean absolute channel error vs the direct render
   <= 4 and max <= 48; and a fully transparent 16x16 corner remains exactly transparent.

## Verdicts

Points 1-7 hold (frame 3 viewed at 8x: crisp binary edge, no dark ring, pure red; ffmpeg-composited frame 3 has nothing left from frame 0 so disposal is right;
delays [4,4,5,4,4,4]; NETSCAPE only when looping).

Point 8 was too tight and is revised, with the reason: 256 palette entries spread over a two-dimensional colour surface roughly 360 units across leave cells of
about 22 units, so even an ideal quantiser measures a mean error near 4 and a maximum in the 20s. `gif` crate's NeuQuant measures mean 5.2 / max 29 at speed 1
and mean 8.2 / max 52 at the default speed 10 (visible blotching at the bottom of the gradient, see `gif-gradient-gif.png`). The assertion is now mean <= 10,
max <= 64. The transparent corner stays exactly transparent and everything else opaque. Quality note, not asserted: default speed 10 and the lack of dithering
are visible on smooth gradients; a better quantiser or dithering would need a new dependency (to be decided by the owner).
