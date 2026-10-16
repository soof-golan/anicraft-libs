//! Shared helpers for the integration tests.
//!
//! Shaping must not depend on the machine's fonts (Windows, macOS and Linux pick different fallbacks), so [`engine`]
//! and [`engine_for`] give an engine without system fonts and with only the bundled OFL test fonts (a regular file with
//! Latin, Hebrew and Arabic, merged from subsets of Noto Sans, Noto Sans Hebrew and Noto Sans Arabic, and a Latin bold:
//! `tests/fonts`, see `ATTRIBUTION.md`). One regular file, like most system fonts, so runs never split at a font
//! change. Tests about the machine's own
//! fonts (discovery, other scripts such as Han) ask [`system_engine`] or [`system_engine_for`] instead, which return
//! `None` (and the test returns early, passing) when no installed font fits.
#![allow(dead_code, reason = "each test file uses a different subset")]

use std::ops::Range;

use anim_core::Rect;
use anim_text::{Alignment, Direction, Paragraph, RichText, Run, Script, TextEngine, TextLayout};

pub const HEBREW: &str = "שלום עולם";
pub const ARABIC: &str = "مرحبا بالعالم";

/// The bundled test fonts: regular (Latin, Hebrew, Arabic with the lam-alef ligature) and a Latin bold weight.
const FONTS: [&[u8]; 2] = [include_bytes!("../fonts/AnicraftTestSans-Regular.ttf"), include_bytes!("../fonts/AnicraftTestSans-Bold.ttf")];

/// An engine that knows only the bundled test fonts (the first stands in for the generic sans serif): the same on
/// every machine.
#[allow(clippy::unnecessary_wraps, reason = "always Some, but tests call it like `system_engine`, which can be None")]
pub fn engine() -> Option<TextEngine> {
    let mut engine = TextEngine::without_system_fonts();
    for font in FONTS {
        assert!(!engine.register_font(font.to_vec()).is_empty(), "a bundled test font failed to load");
    }
    Some(engine)
}

/// The bundled engine if it covers every script in `scripts`, or `None` (with a note) when one is missing.
pub fn engine_for(scripts: &[Script]) -> Option<TextEngine> {
    let mut engine = engine()?;
    for script in scripts {
        if !engine.covers_script(*script) {
            eprintln!("skipping: the bundled test fonts do not cover {script:?}");
            return None;
        }
    }
    Some(engine)
}

/// An engine with the machine's fonts, or `None` when there is no font at all.
pub fn system_engine() -> Option<TextEngine> {
    let mut engine = TextEngine::new();
    if engine.families().is_empty() {
        eprintln!("skipping: no fonts installed");
        return None;
    }
    Some(engine)
}

/// A system engine that can draw every script in `scripts`, or `None` (with a note) when one is missing.
pub fn system_engine_for(scripts: &[Script]) -> Option<TextEngine> {
    let mut engine = system_engine()?;
    for script in scripts {
        if !engine.covers_script(*script) {
            eprintln!("skipping: no installed font covers {script:?}");
            return None;
        }
    }
    Some(engine)
}

pub fn run(text: &str) -> Run {
    Run { size: 20.0, ..Run::new(text) }
}

/// One paragraph of one 20 px run.
pub fn one(text: &str) -> RichText {
    RichText::new(vec![Paragraph::new(vec![run(text)])])
}

/// One paragraph with the given attributes.
pub fn para(text: &str, alignment: Alignment, direction: Direction) -> RichText {
    RichText::new(vec![Paragraph { alignment, direction, ..Paragraph::new(vec![run(text)]) }])
}

/// The glyph runs of the layout from left to right on screen, as (text range, right-to-left).
pub fn visual_runs(layout: &TextLayout) -> Vec<(Range<usize>, bool)> {
    let mut runs: Vec<_> = layout.runs.iter().collect();
    runs.sort_by(|a, b| a.origin.x.total_cmp(&b.origin.x));
    runs.into_iter().map(|r| (r.range.clone(), r.rtl)).collect()
}

/// Every character boundary of the layout's text, including 0 and the length.
pub fn boundaries(layout: &TextLayout) -> Vec<usize> {
    let text = layout.text();
    (0..=text.len()).filter(|&i| text.is_char_boundary(i)).collect()
}

pub fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

pub fn rect_close(a: Rect, b: Rect, tolerance: f64) -> bool {
    close(a.x0, b.x0, tolerance) && close(a.y0, b.y0, tolerance) && close(a.x1, b.x1, tolerance) && close(a.y1, b.y1, tolerance)
}

/// Glyph ids of the whole layout in left-to-right order.
pub fn glyph_ids(layout: &TextLayout) -> Vec<u32> {
    let mut runs: Vec<_> = layout.runs.iter().collect();
    runs.sort_by(|a, b| a.origin.x.total_cmp(&b.origin.x));
    runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.id)).collect()
}
