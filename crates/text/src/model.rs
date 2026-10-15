//! The text model: plain serde data, no fonts involved.
//!
//! A [`RichText`] is a list of [`Paragraph`]s, each a list of styled [`Run`]s. The *plain text* of a `RichText`
//! (see [`RichText::plain_text`]) is every run's text in order with a single `'\n'` between paragraphs. All byte
//! offsets in this crate (hit tests, carets, selections, editing) index into that string.
//!
//! The editing helpers ([`RichText::insert`], [`RichText::delete`], [`RichText::replace`],
//! [`RichText::apply_style`]) keep runs well formed and never panic on bad offsets: offsets are clamped to the text
//! and snapped back to a character boundary.

use std::ops::Range;

use anim_core::Rgba8;
use serde::{Deserialize, Serialize};

/// Where a paragraph's lines sit between its margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Alignment {
    /// The start edge: left for left-to-right paragraphs, right for right-to-left ones.
    #[default]
    Start,
    /// The end edge: right for left-to-right paragraphs, left for right-to-left ones.
    End,
    /// The left edge, whatever the direction.
    Left,
    /// The right edge, whatever the direction.
    Right,
    /// Centred.
    Center,
    /// Both edges, except on the last line of the paragraph and on lines ended by a forced break.
    Justify,
}

/// A paragraph's base direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Direction {
    /// From the first strongly directional character (left-to-right when there is none).
    #[default]
    Auto,
    /// Left to right.
    Ltr,
    /// Right to left (Hebrew, Arabic, ...).
    Rtl,
}

/// A stretch of text in one style. Sizes and spacings are stage pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Run {
    /// The characters. A `'\n'` inside a run is a forced line break within the paragraph.
    pub text: String,
    /// A font family name, or a comma separated CSS list (`"Arial, Helvetica"`). Empty means the default sans serif.
    /// Characters the family lacks are drawn with a fallback font chosen per script.
    pub family: String,
    /// Font size in pixels (clamped to a sane range when laid out).
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub color: Rgba8,
    /// Extra space after every character, in pixels. Not applied to cursive scripts (Arabic) where it would break
    /// the joins.
    pub letter_spacing: f64,
    pub underline: bool,
}

impl Default for Run {
    fn default() -> Self {
        Self { text: String::new(), family: String::new(), size: 16.0, bold: false, italic: false, color: Rgba8::BLACK, letter_spacing: 0.0, underline: false }
    }
}

impl Run {
    /// A run of `text` in the default style.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }

    /// A run with the same style as `self` and different text.
    #[must_use]
    pub fn with_text(&self, text: impl Into<String>) -> Self {
        Self { text: text.into(), ..self.clone() }
    }

    /// Whether `other` has the same style (everything except the text).
    #[must_use]
    pub fn same_style(&self, other: &Self) -> bool {
        self.family == other.family
            && self.size.to_bits() == other.size.to_bits()
            && self.bold == other.bold
            && self.italic == other.italic
            && self.color == other.color
            && self.letter_spacing.to_bits() == other.letter_spacing.to_bits()
            && self.underline == other.underline
    }
}

/// A paragraph: runs plus the attributes that apply to all of its lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Paragraph {
    pub alignment: Alignment,
    pub direction: Direction,
    /// Extra space added after every line, in pixels (Animate's "line spacing"; may be negative).
    pub line_spacing: f64,
    /// Extra space before the first line, on the start side, in pixels (negative hangs the first line out).
    pub indent: f64,
    /// Left margin in pixels (physical: it is the left side for right-to-left paragraphs too).
    pub margin_left: f64,
    /// Right margin in pixels.
    pub margin_right: f64,
    pub runs: Vec<Run>,
}

impl Default for Paragraph {
    fn default() -> Self {
        Self { alignment: Alignment::Start, direction: Direction::Auto, line_spacing: 0.0, indent: 0.0, margin_left: 0.0, margin_right: 0.0, runs: Vec::new() }
    }
}

impl Paragraph {
    /// A paragraph of `runs` with default attributes.
    #[must_use]
    pub fn new(runs: Vec<Run>) -> Self {
        Self { runs, ..Self::default() }
    }

    /// A paragraph of one default-style run.
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        Self::new(vec![Run::new(text)])
    }

    /// The paragraph's text (its runs concatenated).
    #[must_use]
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// The length of [`text`](Self::text) in bytes.
    #[must_use]
    pub fn text_len(&self) -> usize {
        self.runs.iter().fold(0usize, |n, r| n.saturating_add(r.text.len()))
    }

    /// Splits the runs at byte `local` (snapped back to a character boundary) into the runs before and after it.
    fn split_runs(&self, local: usize) -> (Vec<Run>, Vec<Run>) {
        let mut head = Vec::new();
        let mut tail = Vec::new();
        let mut pos = 0usize;
        for run in &self.runs {
            let end = pos.saturating_add(run.text.len());
            if end <= local {
                head.push(run.clone());
            } else if pos >= local {
                tail.push(run.clone());
            } else {
                let cut = snap_down(&run.text, local.saturating_sub(pos));
                head.push(run.with_text(run.text.get(..cut).unwrap_or_default()));
                tail.push(run.with_text(run.text.get(cut..).unwrap_or_default()));
            }
            pos = end;
        }
        (head, tail)
    }
}

/// The rich text of one text object.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RichText {
    pub paragraphs: Vec<Paragraph>,
}

/// Where a plain-text offset sits in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Location {
    paragraph: usize,
    /// Byte offset inside the paragraph's text.
    local: usize,
}

impl RichText {
    /// A text of `paragraphs`.
    #[must_use]
    pub const fn new(paragraphs: Vec<Paragraph>) -> Self {
        Self { paragraphs }
    }

    /// One paragraph per line of `text`, every run in `style` (its own `text` is ignored).
    #[must_use]
    pub fn from_plain(text: &str, style: &Run) -> Self {
        let paragraphs = text.split('\n').map(|line| Paragraph::new(vec![style.with_text(line)])).collect();
        Self { paragraphs }
    }

    /// One default-style paragraph per line of `text`.
    #[must_use]
    pub fn plain(text: &str) -> Self {
        Self::from_plain(text, &Run::default())
    }

    /// The plain text: all runs, paragraphs separated by `'\n'`. Offsets elsewhere in this crate index into it.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let mut out = String::with_capacity(self.text_len());
        for (i, p) in self.paragraphs.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            for r in &p.runs {
                out.push_str(&r.text);
            }
        }
        out
    }

    /// The length of [`plain_text`](Self::plain_text) in bytes.
    #[must_use]
    pub fn text_len(&self) -> usize {
        let chars = self.paragraphs.iter().fold(0usize, |n, p| n.saturating_add(p.text_len()));
        chars.saturating_add(self.paragraphs.len().saturating_sub(1))
    }

    /// Whether there is no text at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paragraphs.iter().all(|p| p.runs.iter().all(|r| r.text.is_empty()))
    }

    /// Each paragraph's byte range in the plain text (without the `'\n'` that follows it).
    #[must_use]
    pub fn paragraph_ranges(&self) -> Vec<Range<usize>> {
        let mut start = 0usize;
        self.paragraphs
            .iter()
            .map(|p| {
                let end = start.saturating_add(p.text_len());
                let range = start..end;
                start = end.saturating_add(1);
                range
            })
            .collect()
    }

    /// The style of the character after `offset` (or the one before it at the end of the text), if there is any run.
    #[must_use]
    pub fn style_at(&self, offset: usize) -> Option<&Run> {
        let loc = self.locate(offset)?;
        let para = self.paragraphs.get(loc.paragraph)?;
        let mut pos = 0usize;
        let mut last = None;
        for run in &para.runs {
            let end = pos.saturating_add(run.text.len());
            if loc.local < end {
                return Some(run);
            }
            if !run.text.is_empty() {
                last = Some(run);
            }
            pos = end;
        }
        last.or_else(|| para.runs.last())
    }

    /// Inserts `text` at `offset`. A `'\n'` in `text` starts a new paragraph with the attributes of the one it
    /// splits. The text takes the style of the character before `offset` (or after it at the start of a paragraph).
    pub fn insert(&mut self, offset: usize, text: &str) {
        self.replace(offset..offset, text);
    }

    /// Deletes `range`. Deleting across a paragraph break joins the paragraphs; the first one's attributes win.
    pub fn delete(&mut self, range: Range<usize>) {
        self.replace(range, "");
    }

    /// Replaces `range` with `text`, see [`insert`](Self::insert) and [`delete`](Self::delete).
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        if self.paragraphs.is_empty() {
            self.paragraphs.push(Paragraph::default());
        }
        let total = self.text_len();
        let start = range.start.min(total);
        let end = range.end.clamp(start, total);
        let (Some(a), Some(b)) = (self.locate(start), self.locate(end)) else { return };
        let Some(first) = self.paragraphs.get(a.paragraph).cloned() else { return };
        let Some(last) = self.paragraphs.get(b.paragraph) else { return };
        let (head, _) = first.split_runs(a.local);
        let (_, tail) = last.split_runs(b.local);
        // The style typed text inherits.
        let style = head.last().or_else(|| tail.first()).cloned().unwrap_or_default();

        let mut segments = text.split('\n');
        let mut current = Paragraph { runs: head, ..first };
        if let Some(seg) = segments.next() {
            current.runs.push(style.with_text(seg));
        }
        let mut rebuilt = Vec::new();
        for seg in segments {
            let next = Paragraph { runs: vec![style.with_text(seg)], ..current.clone() };
            rebuilt.push(std::mem::replace(&mut current, next));
        }
        current.runs.extend(tail);
        rebuilt.push(current);
        let Some(upto) = b.paragraph.checked_add(1) else { return };
        self.paragraphs.splice(a.paragraph..upto.min(self.paragraphs.len()), rebuilt);
        self.normalize();
    }

    /// Applies `f` to the style of every run piece inside `range`, splitting runs at its ends.
    pub fn apply_style(&mut self, range: Range<usize>, mut f: impl FnMut(&mut Run)) {
        let ranges = self.paragraph_ranges();
        for (para, prange) in self.paragraphs.iter_mut().zip(ranges) {
            let lo = range.start.max(prange.start);
            let hi = range.end.min(prange.end);
            if lo >= hi {
                continue;
            }
            let (lo, hi) = (lo.saturating_sub(prange.start), hi.saturating_sub(prange.start));
            let mut out = Vec::with_capacity(para.runs.len().saturating_add(2));
            let mut pos = 0usize;
            for run in std::mem::take(&mut para.runs) {
                let end = pos.saturating_add(run.text.len());
                let (a, b) = (lo.max(pos), hi.min(end));
                if a >= b {
                    out.push(run);
                } else {
                    let ca = snap_down(&run.text, a.saturating_sub(pos));
                    let cb = snap_down(&run.text, b.saturating_sub(pos));
                    let piece = |r: Range<usize>| run.with_text(run.text.get(r).unwrap_or_default());
                    if ca > 0 {
                        out.push(piece(0..ca));
                    }
                    let mut mid = piece(ca..cb);
                    f(&mut mid);
                    out.push(mid);
                    if cb < run.text.len() {
                        out.push(piece(cb..run.text.len()));
                    }
                }
                pos = end;
            }
            para.runs = out;
        }
        self.normalize();
    }

    /// Drops empty runs (keeping one in an otherwise empty paragraph, which carries the paragraph's style) and merges
    /// neighbouring runs that have the same style.
    pub fn normalize(&mut self) {
        for para in &mut self.paragraphs {
            let carrier = para.runs.iter().find(|r| r.text.is_empty()).cloned();
            let mut merged: Vec<Run> = Vec::with_capacity(para.runs.len());
            for run in std::mem::take(&mut para.runs) {
                if run.text.is_empty() {
                    continue;
                }
                match merged.last_mut() {
                    Some(prev) if prev.same_style(&run) => prev.text.push_str(&run.text),
                    _ => merged.push(run),
                }
            }
            if merged.is_empty() {
                merged.extend(carrier);
            }
            para.runs = merged;
        }
    }

    fn locate(&self, offset: usize) -> Option<Location> {
        let mut start = 0usize;
        for (i, p) in self.paragraphs.iter().enumerate() {
            let len = p.text_len();
            let end = start.saturating_add(len);
            if offset <= end || i.saturating_add(1) == self.paragraphs.len() {
                let text = p.text();
                let local = snap_down(&text, offset.saturating_sub(start).min(len));
                return Some(Location { paragraph: i, local });
            }
            start = end.saturating_add(1);
        }
        None
    }
}

/// The largest character boundary of `s` that is at most `index`.
pub(crate) fn snap_down(s: &str, index: usize) -> usize {
    let mut i = index.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i = i.saturating_sub(1);
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold(text: &str) -> Run {
        Run { bold: true, ..Run::new(text) }
    }

    #[test]
    fn plain_text_joins_paragraphs_with_newlines() {
        let t = RichText::new(vec![Paragraph::new(vec![Run::new("ab"), bold("cd")]), Paragraph::from_text("é")]);
        assert_eq!(t.plain_text(), "abcd\né");
        assert_eq!(t.text_len(), 7);
        assert_eq!(t.paragraph_ranges(), vec![0..4, 5..7]);
        assert!(!t.is_empty());
        assert!(RichText::default().is_empty());
    }

    #[test]
    fn insert_inherits_the_style_before_the_caret() {
        let mut t = RichText::new(vec![Paragraph::new(vec![Run::new("ab"), bold("cd")])]);
        t.insert(4, "ef");
        assert_eq!(t.plain_text(), "abcdef");
        assert_eq!(t.paragraphs[0].runs.len(), 2);
        assert!(t.paragraphs[0].runs[1].bold);
        t.insert(0, ">");
        assert!(!t.paragraphs[0].runs[0].bold, "at the start the style comes from the following text");
        assert_eq!(t.plain_text(), ">abcdef");
    }

    #[test]
    fn newline_splits_a_paragraph_and_keeps_its_attributes() {
        let mut t = RichText::new(vec![Paragraph { alignment: Alignment::Center, ..Paragraph::from_text("abcd") }]);
        t.insert(2, "\n");
        assert_eq!(t.paragraphs.len(), 2);
        assert_eq!(t.plain_text(), "ab\ncd");
        assert!(t.paragraphs.iter().all(|p| p.alignment == Alignment::Center));
    }

    #[test]
    fn delete_across_paragraphs_joins_them() {
        let mut t = RichText::plain("one\ntwo\nthree");
        t.delete(2..9);
        assert_eq!(t.plain_text(), "onhree");
        assert_eq!(t.paragraphs.len(), 1);
        t.delete(0..100);
        assert!(t.is_empty());
        assert_eq!(t.paragraphs.len(), 1);
    }

    #[test]
    fn offsets_inside_a_character_snap_back() {
        let mut t = RichText::plain("aé");
        t.insert(2, "x");
        assert_eq!(t.plain_text(), "axé");
        t.delete(2..3);
        assert!(t.plain_text().is_char_boundary(1));
    }

    #[test]
    fn apply_style_splits_and_merges_runs() {
        let mut t = RichText::plain("hello world");
        t.apply_style(6..11, |r| r.bold = true);
        assert_eq!(t.paragraphs[0].runs.len(), 2);
        assert_eq!(t.paragraphs[0].runs[0].text, "hello ");
        assert!(t.paragraphs[0].runs[1].bold);
        t.apply_style(6..11, |r| r.bold = false);
        assert_eq!(t.paragraphs[0].runs.len(), 1);
    }

    #[test]
    fn style_at_prefers_the_following_character() {
        let t = RichText::new(vec![Paragraph::new(vec![Run::new("ab"), bold("cd")])]);
        assert!(!t.style_at(1).unwrap().bold);
        assert!(t.style_at(2).unwrap().bold);
        assert!(t.style_at(4).unwrap().bold, "at the end the last run's style");
    }

    #[test]
    fn serde_round_trip_and_defaults() {
        let t = RichText::new(vec![Paragraph { direction: Direction::Rtl, ..Paragraph::new(vec![bold("שלום")]) }]);
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<RichText>(&json).unwrap(), t);
        let sparse: RichText = serde_json::from_str(r#"{"paragraphs":[{"runs":[{"text":"x"}]}]}"#).unwrap();
        assert_eq!(sparse.paragraphs[0].runs[0].size, 16.0);
    }
}
