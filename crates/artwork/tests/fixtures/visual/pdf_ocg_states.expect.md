# pdf_ocg_states

Visibility rules of optional content, on a 100x100 page with three squares in three OCGs `A` (L1), `B` (L2),
`C` (L3) at x 0..30, 35..65, 70..100 (pdf y 35..65), colours red, green, blue, and a fourth square (pink, x 35..65,
pdf y 0..30) marked with an optional content *membership dictionary* `/M1` (`/OCGs [B C] /P /AllOn`).

Configurations (each is its own file):

1. `/D << /BaseState /OFF /ON [A] >>`: only A on. Predicted: A visible, B hidden, C hidden. M1 (AllOn needs B and C)
   hidden.
2. `/D << /OFF [A B] >>` (BaseState default ON): A hidden, B hidden, C visible. M1 hidden (B off).
3. `/D << >>`: all on: A, B, C visible, M1 visible.

Pixels at the squares' centres (canvas y of the A/B/C squares is 35..65, so y = 50; the pink square is at canvas
y 70..100, so y = 85):

| config | A (15,50) | B (50,50) | C (85,50) | M1 (50,85) |
|---|---|---|---|---|
| 1 | red | white | white | white |
| 2 | white | white | blue | white |
| 3 | red | green | blue | pink (255,192,203) |

(Display list: hidden layers are left out.)

The importer need not model M1 (membership dictionaries) exactly, but it must not make hidden content visible: if
M1 is ignored, it is the pink square that decides, and the verdict records what the library does.

## Verdicts

Inspected `pdf_ocg_states_{base_off,two_off,all_on}@5x.png`: all three configurations match the table. The
membership dictionary M1 is honoured (AllOn of B and C): pink shows only in the all-on configuration and never
leaks when B is off. No bug.
