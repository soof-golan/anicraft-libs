# bidi-parens-mirror

Four paragraphs, DejaVu Sans 28 px, unwrapped.

1. `(שלום) [עולם] {אבג}` (auto -> RTL). Brackets pair with the Hebrew inside them and sit at level 1, so each is drawn
   mirrored: the logical `(` (at offset 0) is the rightmost glyph and its glyph id equals the id of `)` in an LTR
   layout (its picture is `)`, opening towards the left). The leftmost glyph of the line is the mirror of logical `}`,
   i.e. glyph id of `{`.
   - P1: rightmost glyph id == glyph id of ")" ; leftmost glyph id == glyph id of "{".
2. `שלום (hello) עולם`: `hello` stays LTR. The parens around it resolve to level 1 (neutrals between L and R take the
   embedding direction) and are mirrored, so visually the text reads `(hello)`: the glyph left of `hello` is the glyph
   for `(` (= mirror of logical `)`), the glyph right of `hello` is the glyph for `)`.
   - P2: glyph immediately left of the `hello` run has the id of "(" ; immediately right has the id of ")".
3. `(abc)` forced LTR: no mirroring; glyph ids are those of "(" "a" "b" "c" ")" in order.
   - P3.
4. `((א))` RTL: four parens, all mirrored. Left to right on screen the glyph ids are: `(` `(` א `)` `)`
   (logical `))` at the end shows as `((` on the left).
   - P4: ids in x order = [id("("), id("("), id(א), id(")"), id(")")].

Verdicts (bidi-parens-mirror.png): P1 to P4 confirmed. Pictures read `(שלום) [עולם] {אבג}`, `שלום (hello) עולם`,
`(abc)` unmirrored and `((א))`; glyph ids match the predictions (mirroring by glyph substitution, only in RTL runs).
