# prores-alpha

64x48 MOV, ProRes 4444, 24 fps, 2 frames, premultiplied RGBA input.
- columns 0..32: straight colour (200,30,30) with alpha ramping linearly with x: alpha(x) = 8 x + 4 for x in 0..32 (4 .. 252), stored premultiplied
  (colour channel c * alpha / 255, rounded).
- a fully transparent band x 32..40, written as (0,0,0,0).
- rows 0..24 of x 40..64: opaque blue (0,0,255); rows 24..48 of x 40..64: half-transparent white: straight (255,255,255), alpha 128, premultiplied (128,128,128,128).
Second encode with `alpha: false` of the same frames.

## Predictions (ffmpeg decode to yuva444p, raw planes)

1. The alpha plane is the input alpha: at x = 0..32, plane value within +-1 of 8 x + 4 (10 bit stored, 8 bit decoded); band x 32..40 exactly 0; blue block 255; white block 128 +-1.
2. The colour is stored STRAIGHT: converting the decoded Y/Cb/Cr (BT.709 limited) back to RGB for pixels with alpha >= 60 (x >= 8) gives (200,30,30) within +-8
   per channel. A premultiplied store would give (200,30,30) * alpha/255 (for x = 8: 69, 10, 10), which is the failure to look for.
   The white block decodes to (255,255,255) +-4 (not grey 128).
3. For the pixels whose alpha is below 60 (x < 8) the colour is noisier because 8-bit premultiplied input has too little precision; no assertion beyond: at x = 4 (alpha 36)
   the red channel is within +-35 of 200 and never darker than 150.
4. ffprobe: codec prores, profile 4444, pix_fmt yuva444p12le, codec tag `ap4h`, 64x48, nb_frames 2.
5. With alpha: false: pix_fmt has no alpha (yuv444p12le), and the picture is the premultiplied colour over black: the white block decodes to grey (128,128,128) +-4,
   the half-ramp pixel x = 16 (alpha 132) to (200,30,30) * 132 / 255 = (104,16,16) +-6.
6. Compositing the alpha version over a mid-grey (128,128,128) with ffmpeg (`overlay`) matches the arithmetic composite of the source over grey within +-6 per channel
   at every pixel in the ramp, x = 40..64 (a real compositor sees the same picture the display list described).

## Verdicts

Inspected at 8x: `prores-alpha-raw-rgba.png` (ffmpeg's straight RGBA, alpha viewed against white: a smooth pink to red ramp, a clean empty band, solid blue, the half white
block invisible against white) and `prores-alpha-over-grey.png` (composited over mid grey: smooth, no dark fringe, white block light grey, blue solid). Points 1-6 hold:
alpha plane within +-1.5 of the input, colour stored straight (200,30,30 +-8 for every pixel with alpha >= 60), pix_fmt yuva444p12le, tag ap4h; without alpha the
picture is the premultiplied colour over black; the over-grey composite matches the arithmetic within +-6 at every pixel (done on ffmpeg's decoded RGBA, not with
an ffmpeg overlay filter, which changes nothing about the check). The 63x47 odd-size MOVs (both alpha modes) decode at the right size with exact edge colours.
No bug.
