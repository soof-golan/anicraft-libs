//! The result of laying out a [`RichText`](crate::RichText): lines, glyph runs, bounds, and the geometry queries an
//! editor needs (hit testing, caret rectangles, selection rectangles).
//!
//! Coordinates are layout space: y down, the origin at the top-left corner of the text box. Byte offsets index into
//! [`TextLayout::text`], the plain text of the laid-out `RichText`.

use std::ops::Range;

use anim_core::num::{f64_to_f32, usize_to_f64};
use anim_core::{Point, Rect, Rgba8, Vec2};
use parley::FontData;
use parley::editing::{Cursor, Selection};
use parley::layout::{Affinity as ParleyAffinity, Cluster, Layout};

use crate::motion::{Affinity, Caret};

/// One positioned glyph. `x`/`y` is the glyph origin (on the baseline) in layout space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// Glyph id in the run's font.
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub advance: f64,
}

/// The font a run was shaped with: enough to read its outlines (`data()`, `index`, `coords`).
#[derive(Debug, Clone, PartialEq)]
pub struct FontFace {
    /// Identity of the font data (equal ids mean the same file contents).
    pub id: u64,
    /// Face index inside a collection file.
    pub index: u32,
    /// Normalised variation coordinates (empty for static fonts).
    pub coords: Vec<i16>,
    data: FontData,
}

impl FontFace {
    pub(crate) fn new(data: FontData, coords: Vec<i16>) -> Self {
        Self { id: data.data.id(), index: data.index, coords, data }
    }

    /// The font file's bytes.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        self.data.data.data()
    }
}

/// Glyphs of one font, size and colour on one line, in visual (left to right) order.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    pub font: FontFace,
    /// Font size in pixels.
    pub size: f64,
    pub color: Rgba8,
    /// Whether the characters of the run read right to left.
    pub rtl: bool,
    /// Index into [`TextLayout::lines`].
    pub line: usize,
    /// The characters the glyphs were shaped from, as a byte range of the plain text (logical order).
    pub range: Range<usize>,
    /// Left to right on screen, whatever the reading direction.
    pub glyphs: Vec<Glyph>,
    /// Where the run starts: left edge x and baseline y.
    pub origin: Point,
    pub advance: f64,
    /// The underline, if the run has one, as a rectangle.
    pub underline: Option<Rect>,
    /// The font has no bold face for the requested weight: draw the glyphs emboldened.
    pub embolden: bool,
    /// The font has no italic face: shear the glyphs by this many degrees (0 for none).
    pub skew_degrees: f64,
}

/// One line of text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Index into [`TextLayout::paragraphs`].
    pub paragraph: usize,
    /// The characters on the line (logical order). Soft-wrapped lines include their trailing space.
    pub range: Range<usize>,
    /// The line box: the extent of the visible text horizontally, the full line height vertically.
    pub rect: Rect,
    pub baseline: f64,
    pub ascent: f64,
    pub descent: f64,
    /// Whether the paragraph this line belongs to is right to left.
    pub rtl: bool,
}

/// One paragraph of the laid-out text.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphInfo {
    /// The paragraph's characters (without the `'\n'` after it).
    pub range: Range<usize>,
    /// The paragraph box: margins to margins horizontally, first to last line vertically.
    pub rect: Rect,
    pub rtl: bool,
    /// The paragraph's lines, as indices into [`TextLayout::lines`].
    pub lines: Range<usize>,
}

/// A laid-out text.
#[derive(Debug, Clone)]
pub struct TextLayout {
    pub lines: Vec<Line>,
    pub runs: Vec<GlyphRun>,
    pub paragraphs: Vec<ParagraphInfo>,
    /// The text box: the wrapping width (or the widest line when there is none) by the height of all lines.
    pub bounds: Rect,
    pub(crate) text: String,
    pub(crate) truncated: bool,
    pub(crate) paras: Vec<ParaLayout>,
}

/// A paragraph's parley layout with the bookkeeping to translate between its coordinates and layout space.
#[derive(Debug, Clone)]
pub(crate) struct ParaLayout {
    pub layout: Layout<Rgba8>,
    /// Plain-text offset of the paragraph's first character.
    pub start: usize,
    /// Bytes of paragraph text.
    pub len: usize,
    /// Bytes of invisible direction mark parley sees before the text.
    pub prefix: usize,
    /// Top-left corner of the paragraph box in layout space.
    pub origin: Vec2,
    /// Extra space added after every line (line N is shifted down by `N * spacing` against parley's own positions).
    pub spacing: f32,
    pub rtl: bool,
    /// Parley's top and bottom coordinate of every line.
    pub extents: Vec<(f32, f32)>,
}

impl ParaLayout {
    pub fn shift(&self, line: usize) -> f32 {
        self.spacing * f64_to_f32(usize_to_f64(line))
    }

    /// Plain-text offset of parley byte index `index`.
    pub fn global(&self, index: usize) -> usize {
        self.start.saturating_add(index.saturating_sub(self.prefix).min(self.len))
    }

    /// Parley byte index of plain-text offset `offset`.
    pub fn index(&self, offset: usize) -> usize {
        offset.saturating_sub(self.start).min(self.len).saturating_add(self.prefix)
    }

    pub fn cursor(&self, caret: Caret) -> Cursor {
        let affinity = match caret.affinity {
            Affinity::Downstream => ParleyAffinity::Downstream,
            Affinity::Upstream => ParleyAffinity::Upstream,
        };
        Cursor::from_byte_index(&self.layout, self.index(caret.offset), affinity)
    }

    pub fn caret(&self, cursor: Cursor) -> Caret {
        let affinity = match cursor.affinity() {
            ParleyAffinity::Downstream => Affinity::Downstream,
            ParleyAffinity::Upstream => Affinity::Upstream,
        };
        Caret { offset: self.global(cursor.index()), affinity }
    }

    /// The line containing parley y coordinate `y`, or the nearest one.
    pub fn line_at_parley_y(&self, y: f32) -> usize {
        nearest_line(self.extents.iter().copied().enumerate().map(|(i, e)| (i, e)), y)
    }

    /// The line whose (shifted) box contains local y coordinate `y`, or the nearest one.
    pub fn line_at_local_y(&self, y: f32) -> usize {
        nearest_line(self.extents.iter().enumerate().map(|(i, &(top, bottom))| (i, (top + self.shift(i), bottom + self.shift(i)))), y)
    }

    /// A parley y coordinate inside the line that local y coordinate `y` belongs to.
    pub fn to_parley_y(&self, y: f32) -> f32 {
        let line = self.line_at_local_y(y);
        let (top, bottom) = self.extents.get(line).copied().unwrap_or((0.0, 0.0));
        let y = y - self.shift(line);
        if bottom - top > 0.002 { y.clamp(top + 0.001, bottom - 0.001) } else { top }
    }

    /// Height of the paragraph box: all lines and the spacing between them.
    pub fn height(&self) -> f32 {
        let lines = self.extents.len().saturating_sub(1);
        self.layout.height() + self.shift(lines)
    }
}

/// Index of the interval containing `y`, or of the one closest to it (the first on ties).
fn nearest_line(lines: impl Iterator<Item = (usize, (f32, f32))>, y: f32) -> usize {
    let mut best = (0usize, f32::INFINITY);
    for (i, (top, bottom)) in lines {
        let distance = if y < top {
            top - y
        } else if y > bottom {
            y - bottom
        } else {
            0.0
        };
        if distance < best.1 {
            best = (i, distance);
        }
    }
    best.0
}

fn rect_of(para: &ParaLayout, x0: f64, y0: f64, x1: f64, y1: f64, line: usize) -> Rect {
    let dy = f64::from(para.shift(line));
    Rect::new(para.origin.x + x0, para.origin.y + y0 + dy, para.origin.x + x1, para.origin.y + y1 + dy)
}

impl TextLayout {
    /// The plain text the offsets of this layout index into.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the text was cut short because it exceeded the engine's size limits.
    #[must_use]
    pub const fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// The union of the line boxes (an empty rectangle at the origin when there is no line).
    #[must_use]
    pub fn content_bounds(&self) -> Rect {
        self.lines.iter().map(|l| l.rect).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO)
    }

    /// The index in [`lines`](Self::lines) of the line a caret at `offset` is on.
    #[must_use]
    pub fn line_of(&self, caret: Caret) -> Option<usize> {
        let rect = self.caret_rect(caret, 0.0)?;
        let y = rect.center().y;
        self.lines.iter().enumerate().min_by(|(_, a), (_, b)| distance_to_span(a.rect, y).total_cmp(&distance_to_span(b.rect, y))).map(|(i, _)| i)
    }

    pub(crate) fn para_for_offset(&self, offset: usize) -> Option<&ParaLayout> {
        self.paras.iter().rev().find(|p| p.start <= offset).or_else(|| self.paras.first())
    }

    pub(crate) fn para_index_for_offset(&self, offset: usize) -> Option<usize> {
        self.paras.iter().rposition(|p| p.start <= offset).or(if self.paras.is_empty() { None } else { Some(0) })
    }

    /// The caret at `offset` (clamped to the text, snapped to a character boundary), attached to the character after
    /// it.
    #[must_use]
    pub fn caret_at(&self, offset: usize) -> Caret {
        let mut offset = offset.min(self.text.len());
        while offset > 0 && !self.text.is_char_boundary(offset) {
            offset = offset.saturating_sub(1);
        }
        Caret { offset, affinity: Affinity::Downstream }
    }

    /// The caret nearest to `point`: the line is picked by y, the character by x, and the side of the character by
    /// which half of it `point` is in (visually, so it is right in right-to-left and mixed text too).
    #[must_use]
    pub fn hit_test_caret(&self, point: Point) -> Caret {
        let Some(pi) = self.paras.iter().rposition(|p| p.origin.y <= point.y).or(if self.paras.is_empty() { None } else { Some(0) }) else {
            return Caret::default();
        };
        let Some(para) = self.paras.get(pi) else { return Caret::default() };
        let local = point - para.origin;
        let y = para.to_parley_y(f64_to_f32(local.y));
        let cursor = Cursor::from_point(&para.layout, f64_to_f32(local.x), y);
        para.caret(cursor)
    }

    /// The byte offset nearest to `point` (see [`hit_test_caret`](Self::hit_test_caret)).
    #[must_use]
    pub fn hit_test(&self, point: Point) -> usize {
        self.hit_test_caret(point).offset
    }

    /// The rectangle of the caret `width` pixels wide. At a boundary between left-to-right and right-to-left text the
    /// same offset has two places; the caret's affinity picks one. `None` only when the layout has no paragraphs.
    #[must_use]
    pub fn caret_rect(&self, caret: Caret, width: f64) -> Option<Rect> {
        let para = self.para_for_offset(caret.offset)?;
        let bb = para.cursor(caret).geometry(&para.layout, f64_to_f32(width));
        let line = para.line_at_parley_y(f64_to_f32((bb.y0 + bb.y1) * 0.5));
        Some(rect_of(para, bb.x0, bb.y0, bb.x1, bb.y1, line))
    }

    /// The rectangles that highlight `range`: one per line and per visually separate stretch (a selection that crosses a
    /// left-to-right/right-to-left boundary gives several on one line). Paragraph breaks inside the range are not
    /// drawn.
    #[must_use]
    pub fn selection_rects(&self, range: Range<usize>) -> Vec<Rect> {
        let mut out = Vec::new();
        for para in &self.paras {
            let end = para.start.saturating_add(para.len);
            let (lo, hi) = (range.start.max(para.start), range.end.min(end));
            if lo >= hi {
                continue;
            }
            let anchor = Cursor::from_byte_index(&para.layout, para.index(lo), ParleyAffinity::Downstream);
            let focus = Cursor::from_byte_index(&para.layout, para.index(hi), ParleyAffinity::Upstream);
            for (bb, line) in Selection::new(anchor, focus).geometry(&para.layout) {
                out.push(rect_of(para, bb.x0, bb.y0, bb.x1, bb.y1, line));
            }
        }
        out
    }

    /// The word around `offset` (the word after it when `offset` sits between two), for double-click selection. Empty
    /// when the text is empty.
    #[must_use]
    pub fn word_range_at(&self, offset: usize) -> Range<usize> {
        let Some(para) = self.para_for_offset(offset) else { return 0..0 };
        if para.len == 0 {
            return para.start..para.start;
        }
        let mut local = offset.saturating_sub(para.start).min(para.len);
        if local >= para.len {
            local = para.len.saturating_sub(1);
        }
        let text = self.text.get(para.start..).unwrap_or_default();
        while local > 0 && !text.is_char_boundary(local) {
            local = local.saturating_sub(1);
        }
        let Some(cluster) = Cluster::from_byte_index(&para.layout, local.saturating_add(para.prefix)) else { return offset..offset };
        let r = cluster.text_range();
        let before = Cursor::from_byte_index(&para.layout, r.start, ParleyAffinity::Downstream).geometry(&para.layout, 0.0);
        let after = Cursor::from_byte_index(&para.layout, r.end, ParleyAffinity::Upstream).geometry(&para.layout, 0.0);
        let x = f64_to_f32((before.x0 + after.x0) * 0.5);
        let y = f64_to_f32((before.y0 + before.y1) * 0.5);
        let word = Selection::word_from_point(&para.layout, x, y).text_range();
        para.global(word.start)..para.global(word.end)
    }

    /// The paragraph containing `offset`, as a byte range.
    #[must_use]
    pub fn paragraph_range_at(&self, offset: usize) -> Range<usize> {
        self.para_for_offset(offset).map_or(0..0, |p| p.start..p.start.saturating_add(p.len))
    }
}

/// Vertical distance from `y` to the span of `rect` (zero inside it).
fn distance_to_span(rect: Rect, y: f64) -> f64 {
    if y < rect.y0 {
        rect.y0 - y
    } else if y > rect.y1 {
        y - rect.y1
    } else {
        0.0
    }
}
