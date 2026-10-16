# psd_color_modes

Four small documents, each 6x2, one layer per row of probes, rendered on white.

## rgb16 (16-bit RGB with alpha), layer `Deep` at (0,0) 6x1

Samples (R,G,B,A as 16-bit): p0 (65535,0,0,65535), p1 (0,32768,0,65535), p2 (257,513,65278,65535)
(= 1, 2, 254 after rounding value/257), p3 (0,0,0,32768), p4 (65535,65535,65535,0), p5 (43690,43690,43690,65535) (= 170).

| point | want |
|---|---|
| (0,0) | (255,0,0) |
| (1,0) | (0,128,0), tol 1 (32768/257 = 127.5) |
| (2,0) | (1,2,254), tol 1 |
| (3,0) | black at alpha 0x8000 over white: 127 or 128, tol 1 |
| (4,0) | white (alpha 0) |
| (5,0) | (170,170,170), tol 1 |

## gray8 and gray16, layer at (0,0) 4x1

gray8: (0,255) (64,255) (200,255) (128,128): grey with alpha. 16-bit: (65535,65535) (16448,65535).

| point | want |
|---|---|
| gray8 (0,0) | (0,0,0) |
| gray8 (1,0) | (64,64,64) |
| gray8 (2,0) | (200,200,200) |
| gray8 (3,0) | half alpha grey 128 over white: 255-(255-128)*128/255 = 191, tol 2 |
| gray16 (0,0) | white (65535) |
| gray16 (1,0) | (64,64,64), tol 1 (16448/257 = 64) |

## cmyk8 and cmyk16, layer at (0,0) 6x1 (ink amounts, 0 = no ink)

p0 paper (0,0,0,0), p1 K only 255, p2 K only 128, p3 C only 255, p4 M only 255, p5 Y only 255.
Without an embedded profile the naive conversion R = 255*(1-C)*(1-K) etc. is what we can check against;
Photoshop's own profile conversion differs by up to ~60 levels on saturated inks. The properties that must hold:

| point | want |
|---|---|
| p0 | white (255,255,255) |
| p1 | very dark: every channel < 60 |
| p2 | mid grey: channels within 90..170 and all equal within 4 |
| p3 | cyan: R < 90, G > 150, B > 150 |
| p4 | magenta: G < 90, R > 150, B > 150 |
| p5 | yellow: B < 90, R > 150, G > 150 |

The 16-bit CMYK document (`Cmyka16`) gives the same classes of colour for the same relative inks (65535 = full ink).

## Verdicts

All matched (inspected the four `psd_rgb16`, `psd_gray8`, `psd_cmyk8`, `psd_cmyk16` PNGs at 24x). 16-bit values
convert with rounding (1,2,254 and 128 for 32768), grey alpha composites right, CMYK 8 and 16 bit give identical
looking swatches: paper white, K black, K half mid grey, saturated cyan, magenta and yellow. No bug.
