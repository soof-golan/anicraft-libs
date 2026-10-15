//! The text engine: owns the font database and the layout scratch space, and turns a
//! [`RichText`] into a [`TextLayout`].
//!
//! Each paragraph is shaped, bidi-reordered and line-broken by parley as one layout of its own, so a paragraph can have
//! its own alignment, direction, margins and indent. Paragraphs are then stacked, and everything a consumer needs
//! (lines, glyph runs, bounds) is copied out in layout space; the parley layouts are kept inside the result for caret
//! and hit-test queries.

use std::borrow::Cow;
use std::ops::Range;

use anim_core::kurbo::Shape;
use anim_core::num::f64_to_f32;
use anim_core::{Rect, Rgba8, Vec2};
use anim_display::Node;
use fontique::Language;
use parley::layout::{Alignment as ParleyAlignment, AlignmentOptions, IndentOptions};
use parley::style::{FontFamily, FontFamilyName, FontStyle, FontWeight, GenericFamily, StyleProperty};
use parley::{Layout, LayoutContext};

use crate::extract::copy_lines;
use crate::fonts::{Fonts, ResolvedFont};
use crate::guard::guarded;
use crate::layout::{ParaLayout, ParagraphInfo, TextLayout};
use crate::model::{Alignment, Direction, Paragraph, RichText, Run};
use crate::outline::OutlineCache;
use crate::sanitize::Sanitizer;
use crate::script::{Script, is_cursive, scripts_in};

/// Plain text beyond this many bytes is not laid out.
pub const MAX_TEXT_BYTES: usize = 4 << 20;
/// Paragraphs beyond this many are not laid out.
pub const MAX_PARAGRAPHS: usize = 100_000;
const MIN_SIZE: f64 = 0.1;
const MAX_SIZE: f64 = 16_384.0;
/// Margins, indents, spacings and widths are clamped to this magnitude.
const MAX_EXTENT: f64 = 1.0e7;
/// Font families listed on one run (the author's list plus script fallbacks).
const MAX_FAMILIES: usize = 24;
/// Smallest width lines are broken at, so a zero or negative box still makes progress.
const MIN_LINE_WIDTH: f64 = 1.0;

const LRM: &str = "\u{200E}";
const RLM: &str = "\u{200F}";

/// Lays out rich text. Create one per app or per layout actor and reuse it: it owns the font database (system fonts are
/// discovered lazily on first use) and caches shaping data and glyph outlines.
pub struct TextEngine {
    fonts: Fonts,
    lcx: LayoutContext<Rgba8>,
    outlines: OutlineCache,
    language: Option<Language>,
    /// Tests make the next shaping attempts panic, to exercise the fallbacks.
    #[cfg(test)]
    inject_panics: usize,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TextEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextEngine").finish_non_exhaustive()
    }
}

/// A paragraph shaped but not yet broken into lines.
struct Shaped {
    layout: Layout<Rgba8>,
    start: usize,
    len: usize,
    /// The paragraph's text as the author wrote it.
    text: String,
    prefix: usize,
    alignment: Alignment,
    margin_left: f64,
    margin_right: f64,
    spacing: f64,
}

impl TextEngine {
    /// An engine with the fonts installed on this machine.
    #[must_use]
    pub fn new() -> Self {
        Self::with_fonts(Fonts::new(true))
    }

    /// An engine that knows no fonts but the ones registered with [`register_font`](Self::register_font): for tests
    /// and for apps that ship their own fonts.
    #[must_use]
    pub fn without_system_fonts() -> Self {
        Self::with_fonts(Fonts::new(false))
    }

    fn with_fonts(fonts: Fonts) -> Self {
        Self {
            fonts,
            lcx: LayoutContext::new(),
            outlines: OutlineCache::default(),
            language: None,
            #[cfg(test)]
            inject_panics: 0,
        }
    }

    /// Registers font data (TTF, OTF, or a collection) and returns the family names it provides. Registered fonts
    /// are found by name like installed ones, and when the machine has no generic sans serif the first one stands in.
    pub fn register_font(&mut self, bytes: Vec<u8>) -> Vec<String> {
        self.fonts.register(bytes)
    }

    /// All font families available, sorted.
    pub fn families(&mut self) -> Vec<String> {
        self.fonts.families()
    }

    /// Whether a family called `name` is installed or registered.
    pub fn has_family(&mut self, name: &str) -> bool {
        self.fonts.has_family(name)
    }

    /// The families that fall back for `script`, best first (empty when no installed font can draw it).
    pub fn fallback_chain(&mut self, script: Script) -> Vec<String> {
        self.fonts.chain(script)
    }

    /// Whether some available font can draw `script`.
    pub fn covers_script(&mut self, script: Script) -> bool {
        !self.fonts.chain(script).is_empty()
    }

    /// Sets the BCP 47 language (`"ja"`, `"zh-Hans"`, `"he"`) used to choose between regional forms of the same
    /// characters. `None`, or an invalid tag, leaves the choice to the fonts.
    pub fn set_language(&mut self, tag: Option<&str>) {
        self.language = tag.and_then(|t| Language::parse(t).ok());
    }

    /// Turns a laid-out text into display nodes (filled glyph outlines, underlines), reusing cached outlines.
    pub fn to_display(&mut self, layout: &TextLayout) -> Vec<Node> {
        self.outlines.display(layout)
    }

    /// The tight bounding box of everything [`to_display`](Self::to_display) draws for `layout`: the glyph outlines and
    /// underlines themselves, not the line boxes. `None` when nothing is drawn (empty text, spaces, transparent colour).
    /// Useful for auto-sized text and for selection frames that hug the letters.
    pub fn ink_bounds(&mut self, layout: &TextLayout) -> Option<Rect> {
        let mut total: Option<Rect> = None;
        for node in self.outlines.display(layout) {
            let (path, grow) = match &node {
                Node::Fill(fill) => (&fill.path, 0.0),
                Node::Stroke(stroke) => (&stroke.path, stroke.style.width / 2.0),
                _ => continue,
            };
            if path.elements().is_empty() {
                continue;
            }
            let bounds = path.bounding_box().inflate(grow, grow);
            total = Some(total.map_or(bounds, |t| t.union(bounds)));
        }
        total
    }

    /// Shapes and lays out `text`. With `max_width` lines wrap at that width (a single word wider than it overflows);
    /// without, only forced breaks start a new line and alignment is relative to the widest line.
    ///
    /// Never fails: missing fonts, empty text, absurd sizes and invalid numbers all lay out to something sensible. Text
    /// beyond [`MAX_TEXT_BYTES`] or [`MAX_PARAGRAPHS`] is dropped (see [`TextLayout::is_truncated`]).
    pub fn layout(&mut self, text: &RichText, max_width: Option<f64>) -> TextLayout {
        let max_width = max_width.filter(|w| w.is_finite() && *w > 0.0).map(|w| w.min(MAX_EXTENT));

        let mut shaped: Vec<Shaped> = Vec::new();
        let mut end = 0usize;
        let mut truncated = false;
        for (i, paragraph) in text.paragraphs.iter().enumerate() {
            let start = if i == 0 { 0 } else { end.saturating_add(1) };
            if shaped.len() >= MAX_PARAGRAPHS || start.saturating_add(paragraph.text_len()) > MAX_TEXT_BYTES {
                truncated = true;
                break;
            }
            let item = self.shape_paragraph(paragraph, start);
            end = start.saturating_add(item.len);
            shaped.push(item);
        }
        if shaped.is_empty() {
            shaped.push(self.shape_paragraph(&Paragraph::default(), 0));
        }
        let mut plain = text.plain_text();
        plain.truncate(crate::model::snap_down(&plain, end));

        // Break lines. Without a width the box is as wide as the widest paragraph, and every paragraph is aligned in it.
        let margins = |s: &Shaped| s.margin_left + s.margin_right;
        let available = |s: &Shaped, container: f64| (container - margins(s)).max(MIN_LINE_WIDTH);
        let mut container = max_width.unwrap_or(0.0);
        for s in &mut shaped {
            break_lines(s, max_width.map(|w| f64_to_f32(available(s, w))));
            if max_width.is_none() {
                container = container.max(f64::from(s.layout.width()) + margins(s));
            }
        }
        if max_width.is_none() && shaped.len() > 1 {
            for s in &mut shaped {
                let width = available(s, container).mul_add(1.0001, 0.01);
                break_lines(s, Some(f64_to_f32(width)));
            }
        }
        for s in &mut shaped {
            if guarded(|| s.layout.align(parley_alignment(s.alignment), AlignmentOptions::default())).is_none() {
                s.layout = Layout::new();
            }
        }

        // Stack the paragraphs and copy the results out.
        let mut out = TextLayout {
            lines: Vec::new(),
            runs: Vec::new(),
            paragraphs: Vec::new(),
            bounds: Rect::ZERO,
            text: plain,
            truncated,
            paras: Vec::with_capacity(shaped.len()),
        };
        let mut y = 0.0f64;
        let mut last_spacing = 0.0f64;
        for s in shaped {
            let extents: Vec<(f32, f32)> = s.layout.lines().map(|l| (l.metrics().block_min_coord, l.metrics().block_max_coord)).collect();
            let para = ParaLayout {
                rtl: s.layout.is_rtl(),
                layout: s.layout,
                start: s.start,
                len: s.len,
                text: s.text,
                prefix: s.prefix,
                origin: Vec2::new(s.margin_left, y),
                spacing: f64_to_f32(s.spacing),
                extents,
            };
            let first_line = out.lines.len();
            let runs_before = out.runs.len();
            if guarded(|| copy_lines(&para, out.paragraphs.len(), &mut out)).is_none() {
                out.lines.truncate(first_line);
                out.runs.truncate(runs_before);
            }
            let height = f64::from(para.height());
            out.paragraphs.push(ParagraphInfo {
                range: s.start..s.start.saturating_add(s.len),
                rect: Rect::new(s.margin_left, y, (container - s.margin_right).max(s.margin_left), y + height),
                rtl: para.rtl,
                lines: first_line..out.lines.len(),
            });
            y += height + s.spacing;
            last_spacing = s.spacing;
            out.paras.push(para);
        }
        out.bounds = Rect::new(0.0, 0.0, container, (y - last_spacing).max(0.0));
        out
    }

    /// Shapes one paragraph (no line breaking yet). If parley panics on the text, the paragraph is shaped again with every
    /// non-ASCII character replaced by a placeholder, and failing that it is left empty: never a panic, never a lost
    /// offset.
    fn shape_paragraph(&mut self, paragraph: &Paragraph, start: usize) -> Shaped {
        for scrub in [false, true] {
            if let Some(shaped) = guarded(|| self.try_shape(paragraph, start, scrub)) {
                return shaped;
            }
            // A panic inside parley leaves its scratch buffers taken: continue with fresh ones.
            self.lcx = LayoutContext::new();
        }
        Shaped {
            layout: Layout::new(),
            start,
            len: paragraph.text_len(),
            text: paragraph.text(),
            prefix: 0,
            alignment: paragraph.alignment,
            margin_left: 0.0,
            margin_right: 0.0,
            spacing: 0.0,
        }
    }

    fn try_shape(&mut self, paragraph: &Paragraph, start: usize, scrub: bool) -> Shaped {
        #[cfg(test)]
        if self.inject_panics > 0 {
            self.inject_panics = self.inject_panics.saturating_sub(1);
            std::panic::panic_any("injected shaping failure");
        }
        let prefix = match paragraph.direction {
            Direction::Auto => "",
            Direction::Ltr => LRM,
            Direction::Rtl => RLM,
        };
        let mut ptext = String::from(prefix);
        let mut spans: Vec<Range<usize>> = Vec::with_capacity(paragraph.runs.len());
        let mut sanitizer = Sanitizer::new(scrub);
        for run in &paragraph.runs {
            let from = ptext.len();
            sanitizer.push(&mut ptext, &run.text);
            spans.push(from..ptext.len());
        }
        let fonts: Vec<RunFont> = paragraph.runs.iter().map(|r| self.run_font(r)).collect();
        let default_run = paragraph.runs.first().cloned().unwrap_or_default();
        let default_font = fonts.first().cloned().unwrap_or_else(|| RunFont::new(&default_run));

        let language = self.language;
        let mut builder = self.lcx.ranged_builder(&mut self.fonts.cx, &ptext, 1.0, false);
        for property in style_properties(&default_run, &default_font, language) {
            builder.push_default(property);
        }
        for ((run, font), span) in paragraph.runs.iter().zip(&fonts).zip(&spans) {
            if span.is_empty() {
                continue;
            }
            for property in style_properties(run, font, language) {
                builder.push(property, span.clone());
            }
            let spacing = f64_to_f32(finite(run.letter_spacing, 0.0).clamp(-MAX_EXTENT, MAX_EXTENT));
            if spacing != 0.0 {
                for part in spacing_ranges(ptext.get(span.clone()).unwrap_or_default(), span.start) {
                    builder.push(StyleProperty::LetterSpacing(spacing), part);
                }
            }
        }
        let mut layout = builder.build(&ptext);
        let indent = finite(paragraph.indent, 0.0).clamp(-MAX_EXTENT, MAX_EXTENT);
        if indent != 0.0 {
            layout.set_text_indent(f64_to_f32(indent), IndentOptions::default());
        }
        Shaped {
            layout,
            start,
            len: paragraph.text_len(),
            text: paragraph.text(),
            prefix: prefix.len(),
            alignment: paragraph.alignment,
            margin_left: finite(paragraph.margin_left, 0.0).clamp(0.0, MAX_EXTENT),
            margin_right: finite(paragraph.margin_right, 0.0).clamp(0.0, MAX_EXTENT),
            spacing: finite(paragraph.line_spacing, 0.0).clamp(-MAX_EXTENT, MAX_EXTENT),
        }
    }

    /// The font of a run: the author's families (a name that is not an installed family is resolved like
    /// [`resolve_font_name`](Self::resolve_font_name) does, and a style in the name adds to the run's), then the fallback
    /// chains of the scripts in its text, then the generic sans serif.
    fn run_font(&mut self, run: &Run) -> RunFont {
        let (mut bold, mut italic) = (run.bold, run.italic);
        let mut list: Vec<FontFamilyName<'static>> = Vec::new();
        for entry in FontFamilyName::parse_css_list(&run.family).flatten().take(MAX_FAMILIES) {
            let entry = entry.into_owned();
            if let FontFamilyName::Named(name) = &entry
                && !self.fonts.has_family(name)
            {
                let resolved = self.fonts.resolve(name);
                if self.fonts.has_family(&resolved.family) {
                    bold |= resolved.bold;
                    italic |= resolved.italic;
                    list.push(FontFamilyName::Named(Cow::Owned(resolved.family)));
                    continue;
                }
            }
            list.push(entry);
        }
        let kana = run.text.chars().any(|c| matches!(u32::from(c), 0x3040..=0x30FF));
        for script in scripts_in(&run.text, 4) {
            let mut chain = self.fonts.chain(script);
            if kana && script == Script::Han {
                // Kana means Japanese: take the Japanese forms of the shared ideographs.
                chain.sort_by_key(|name| !name.ends_with(" JP"));
            }
            for name in chain {
                let entry = FontFamilyName::Named(Cow::Owned(name));
                if list.len() < MAX_FAMILIES && !list.contains(&entry) {
                    list.push(entry);
                }
            }
        }
        list.push(FontFamilyName::Generic(GenericFamily::SansSerif));
        RunFont { families: list, bold, italic }
    }

    /// Works out which installed family and style a font name from a document stands for: a family name, a PostScript
    /// name (`Arial-BoldMT`, `MyriadPro-Regular`), a full name (`Arial Bold`), or the name of a font this machine
    /// lacks (for the well-known ones a metric-compatible substitute is used when installed, `Arial` becoming
    /// `Liberation Sans`). Importers call this to fill [`Run::family`], [`Run::bold`] and [`Run::italic`]. Layout also
    /// applies it to family names that are not installed, so passing the raw name as the family works too.
    pub fn resolve_font_name(&mut self, name: &str) -> ResolvedFont {
        self.fonts.resolve(name)
    }
}

/// The families and style flags a run is shaped with.
#[derive(Debug, Clone)]
struct RunFont {
    families: Vec<FontFamilyName<'static>>,
    bold: bool,
    italic: bool,
}

impl RunFont {
    fn new(run: &Run) -> Self {
        Self { families: vec![FontFamilyName::Generic(GenericFamily::SansSerif)], bold: run.bold, italic: run.italic }
    }
}

/// Breaks `s` into lines at `width`; a layout parley panics on is replaced by an empty one.
fn break_lines(s: &mut Shaped, width: Option<f32>) {
    if guarded(|| s.layout.break_all_lines(width)).is_none() {
        s.layout = Layout::new();
    }
}

const fn finite(v: f64, default: f64) -> f64 {
    if v.is_finite() { v } else { default }
}

/// The parts of `text` (which starts at byte `base` of the paragraph text) that letter spacing applies to: everything
/// except cursive scripts.
fn spacing_ranges(text: &str, base: usize) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut open: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if is_cursive(c) {
            if let Some(from) = open.take() {
                ranges.push(base.saturating_add(from)..base.saturating_add(i));
            }
        } else if open.is_none() {
            open = Some(i);
        }
    }
    if let Some(from) = open {
        ranges.push(base.saturating_add(from)..base.saturating_add(text.len()));
    }
    ranges
}

fn style_properties(run: &Run, font: &RunFont, language: Option<Language>) -> Vec<StyleProperty<'static, Rgba8>> {
    let size = finite(run.size, 16.0).clamp(MIN_SIZE, MAX_SIZE);
    vec![
        StyleProperty::FontFamily(FontFamily::List(Cow::Owned(font.families.clone()))),
        StyleProperty::FontSize(f64_to_f32(size)),
        StyleProperty::FontWeight(FontWeight::new(if font.bold { 700.0 } else { 400.0 })),
        StyleProperty::FontStyle(if font.italic { FontStyle::Italic } else { FontStyle::Normal }),
        StyleProperty::Brush(run.color),
        StyleProperty::Underline(run.underline),
        StyleProperty::Locale(language),
    ]
}

const fn parley_alignment(alignment: Alignment) -> ParleyAlignment {
    match alignment {
        Alignment::Start => ParleyAlignment::Start,
        Alignment::End => ParleyAlignment::End,
        Alignment::Left => ParleyAlignment::Left,
        Alignment::Right => ParleyAlignment::Right,
        Alignment::Center => ParleyAlignment::Center,
        Alignment::Justify => ParleyAlignment::Justify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_skips_cursive_text() {
        assert_eq!(spacing_ranges("ab", 3), vec![3..5]);
        assert_eq!(spacing_ranges("aبb", 0), vec![0..1, 3..4]);
        assert_eq!(spacing_ranges("بب", 0), Vec::<Range<usize>>::new());
    }

    #[test]
    fn a_shaping_panic_falls_back_to_a_scrubbed_paragraph_with_the_same_offsets() {
        let mut engine = TextEngine::new();
        let text = RichText::plain("ab שלום\nsecond");
        let normal = engine.layout(&text, Some(300.0));
        engine.inject_panics = 1;
        let scrubbed = engine.layout(&text, Some(300.0));
        assert_eq!(scrubbed.text(), normal.text());
        assert_eq!(scrubbed.paragraphs.len(), 2);
        assert_eq!(scrubbed.paragraphs[0].range, normal.paragraphs[0].range);
        assert_eq!(scrubbed.lines.len(), normal.lines.len());
        // The engine keeps working afterwards.
        let after = engine.layout(&text, Some(300.0));
        assert_eq!(after.lines.len(), normal.lines.len());
    }

    #[test]
    fn when_shaping_keeps_failing_the_paragraph_is_empty_but_queries_still_work() {
        let mut engine = TextEngine::new();
        engine.inject_panics = 2;
        let layout = engine.layout(&RichText::plain("lost\nkept"), Some(300.0));
        assert_eq!(layout.text(), "lost\nkept");
        assert_eq!(layout.paragraphs.len(), 2);
        assert_eq!(layout.paragraphs[0].range, 0..4);
        assert_eq!(layout.paragraphs[0].lines, 0..1, "the failed paragraph is one empty line");
        assert_eq!(layout.lines.len(), 2);
        assert!(layout.runs.iter().all(|r| r.line == 1), "its glyphs are in the other paragraph, which is intact");
        for offset in 0..=layout.text().len() {
            let caret = layout.caret_at(offset);
            let _ = layout.caret_rect(caret, 1.0);
            let _ = layout.hit_test_caret(anim_core::Point::new(5.0, 5.0));
            let _ = layout.selection_rects(0..offset);
            for motion in [crate::Motion::Left, crate::Motion::Right, crate::Motion::Up, crate::Motion::Down, crate::Motion::LineEnd, crate::Motion::WordRight]
            {
                let _ = layout.move_caret(caret, motion, None);
            }
            let _ = layout.word_range_at(offset);
        }
        assert_eq!(engine.to_display(&layout).len(), 1);
    }

    #[test]
    fn numbers_are_made_finite() {
        assert!((finite(f64::NAN, 3.0) - 3.0).abs() < f64::EPSILON);
        assert!((finite(f64::INFINITY, 3.0) - 3.0).abs() < f64::EPSILON);
        assert!((finite(2.0, 3.0) - 2.0).abs() < f64::EPSILON);
    }
}
