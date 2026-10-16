//! Hostile input: arbitrary strings, numbers and offsets must never panic, and the answers must stay sane.

#![expect(clippy::disallowed_types, reason = "the proptest macros and strategies are built on Arc")]

use crate::common;

use anim_core::Point;
use anim_text::{Affinity, Alignment, Caret, Direction, Motion, Paragraph, RichText, Run};
use common::engine;
use proptest::prelude::*;

/// Characters that stress shaping and bidi: Latin, spaces, newlines, Hebrew, Arabic (with marks and ligature pairs),
/// CJK, combining marks, bidi controls, emoji, zero-width joiners, controls.
const ALPHABET: &[&str] = &[
    "a", "b", "Z", "é", " ", " ", "\n", "\t", "\r", "1", "9", ".", ",", "(", ")", "ש", "ל", "ו", "ם", "ב", "ا", "ل", "ٌ", "ـ", "ﻻ", "中", "文", "あ", "한",
    "\u{0301}", "\u{200E}", "\u{200F}", "\u{202A}", "\u{202B}", "\u{202C}", "\u{202D}", "\u{202E}", "\u{2066}", "\u{2067}", "\u{2069}", "\u{200D}", "\u{200C}",
    "😀", "\u{0}", "\u{FEFF}", "\u{FFFD}", "\u{2028}", "\u{0085}",
];

/// Progress marks for hunting a hang: `ANIM_TEXT_TRACE=1 cargo test ... -- --nocapture`.
fn trace(what: &str) {
    if std::env::var_os("ANIM_TEXT_TRACE").is_some() {
        eprintln!("  .. {what}");
    }
}

/// Cases per property: 48 by default (a few seconds), more with `ANIM_TEXT_CASES=5000` for an overnight soak.
fn cases() -> u32 {
    std::env::var("ANIM_TEXT_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(48)
}

/// Ordinary text: letters, digits, spaces and punctuation in Latin, Hebrew, Arabic and CJK, with no controls or joiners.
const TILING_ALPHABET: &[&str] = &["a", "b", "word", " ", " ", "1", "9", ".", ",", "(", ")", "ש", "ל", "ו", "ם", "ב", "ا", "ل", "中", "文", "あ", "한", "é"];

fn arb_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::sample::select(ALPHABET), 0..60).prop_map(|parts| parts.concat())
}

fn arb_float() -> impl Strategy<Value = f64> {
    prop_oneof![
        3 => -50.0..500.0f64,
        1 => Just(0.0),
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
        1 => Just(f64::NEG_INFINITY),
        1 => Just(1e300),
        1 => Just(-1e300),
    ]
}

fn arb_alignment() -> impl Strategy<Value = Alignment> {
    proptest::sample::select(&[Alignment::Start, Alignment::End, Alignment::Left, Alignment::Right, Alignment::Center, Alignment::Justify][..])
}

fn arb_direction() -> impl Strategy<Value = Direction> {
    proptest::sample::select(&[Direction::Auto, Direction::Ltr, Direction::Rtl][..])
}

fn arb_run() -> impl Strategy<Value = Run> {
    (arb_text(), arb_float(), any::<bool>(), any::<bool>(), any::<bool>(), arb_float()).prop_map(|(text, size, bold, italic, underline, spacing)| Run {
        text,
        size,
        bold,
        italic,
        underline,
        letter_spacing: spacing,
        ..Run::default()
    })
}

fn arb_paragraph() -> impl Strategy<Value = Paragraph> {
    (proptest::collection::vec(arb_run(), 0..4), arb_alignment(), arb_direction(), arb_float(), arb_float(), arb_float(), arb_float()).prop_map(
        |(runs, alignment, direction, line_spacing, indent, margin_left, margin_right)| Paragraph {
            alignment,
            direction,
            line_spacing,
            indent,
            margin_left,
            margin_right,
            runs,
        },
    )
}

fn arb_rich() -> impl Strategy<Value = RichText> {
    proptest::collection::vec(arb_paragraph(), 0..4).prop_map(RichText::new)
}

fn arb_width() -> impl Strategy<Value = Option<f64>> {
    prop_oneof![Just(None), arb_float().prop_map(Some)]
}

fn arb_motion() -> impl Strategy<Value = Motion> {
    proptest::sample::select(
        &[
            Motion::Left,
            Motion::Right,
            Motion::WordLeft,
            Motion::WordRight,
            Motion::Up,
            Motion::Down,
            Motion::LineStart,
            Motion::LineEnd,
            Motion::ParagraphStart,
            Motion::ParagraphEnd,
            Motion::TextStart,
            Motion::TextEnd,
        ][..],
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn layout_and_every_query_survive_anything(
        text in arb_rich(),
        width in arb_width(),
        offsets in proptest::collection::vec(0usize..400, 6),
        points in proptest::collection::vec((arb_float(), arb_float()), 6),
        motions in proptest::collection::vec(arb_motion(), 8),
    ) {
        let Some(mut engine) = engine() else { return Ok(()) };
        if std::env::var_os("ANIM_TEXT_TRACE").is_some() {
            eprintln!("CASE {text:?} {width:?}");
        }
        let layout = engine.layout(&text, width);
        trace("laid out");
        let len = layout.text().len();

        prop_assert!(layout.bounds.x1.is_finite() && layout.bounds.y1.is_finite());
        prop_assert!(!layout.paragraphs.is_empty());
        for line in &layout.lines {
            prop_assert!(line.range.start <= line.range.end && line.range.end <= len);
            prop_assert!(layout.text().is_char_boundary(line.range.start) && layout.text().is_char_boundary(line.range.end));
            prop_assert!(line.rect.x0.is_finite() && line.rect.y0.is_finite() && line.rect.x1.is_finite() && line.rect.y1.is_finite());
        }
        for run in &layout.runs {
            prop_assert!(run.range.end <= len);
            prop_assert!(run.glyphs.iter().all(|g| g.x.is_finite() && g.y.is_finite()));
        }
        // Display conversion never panics and produces finite geometry.
        let nodes = engine.to_display(&layout);
        trace("displayed");
        for node in &nodes {
            if let anim_display::Node::Fill(fill) = node {
                let bb = anim_core::kurbo::Shape::bounding_box(&fill.path);
                prop_assert!(bb.x0.is_finite() || fill.path.elements().is_empty());
            }
        }
        trace("display checked");

        for &offset in &offsets {
            for affinity in [Affinity::Downstream, Affinity::Upstream] {
                let caret = Caret { offset, affinity };
                trace(&format!("caret_rect {caret:?}"));
                let rect = layout.caret_rect(caret, 1.0);
                prop_assert!(rect.is_some_and(|r| r.x0.is_finite() && r.y0.is_finite() && r.height() >= 0.0), "caret {caret:?} in {len} bytes: {rect:?}");
                for motion in &motions {
                    trace(&format!("move {motion:?} from {caret:?}"));
                    let moved = layout.move_caret(caret, *motion, None);
                    prop_assert!(moved.offset <= len && layout.text().is_char_boundary(moved.offset), "{motion:?} from {caret:?} gave {moved:?} (len {len})");
                }
            }
            trace(&format!("word_range_at {offset}"));
            let word = layout.word_range_at(offset);
            prop_assert!(word.start <= word.end && word.end <= len, "{word:?}");
            trace(&format!("selection_rects {offset}"));
            let rects = layout.selection_rects(offset..offset.saturating_add(17));
            prop_assert!(rects.iter().all(|r| r.x0.is_finite() && r.x1.is_finite() && r.y0.is_finite()));
        }
        for &(x, y) in &points {
            let hit = layout.hit_test_caret(Point::new(x, y));
            prop_assert!(hit.offset <= len && layout.text().is_char_boundary(hit.offset), "hit {hit:?} (len {len}) at ({x}, {y})");
        }
        // A chain of motions from the start keeps working.
        let mut caret = layout.caret_at(0);
        let mut goal = None;
        for motion in &motions {
            caret = layout.move_caret(caret, *motion, goal);
            goal = if matches!(motion, Motion::Up | Motion::Down) { goal.or_else(|| layout.caret_x(caret)) } else { None };
            prop_assert!(caret.offset <= len);
        }
    }

    #[test]
    fn wrapped_single_paragraph_lines_tile_the_text(
        text in proptest::collection::vec(proptest::sample::select(TILING_ALPHABET), 0..60).prop_map(|parts| parts.concat()),
        width in 5.0..300.0f64,
    ) {
        let Some(mut engine) = engine() else { return Ok(()) };
        let rich = RichText::new(vec![Paragraph::new(vec![Run { size: 14.0, ..Run::new(text) }])]);
        let layout = engine.layout(&rich, Some(width));
        let mut next = 0;
        for line in &layout.lines {
            prop_assert_eq!(line.range.start, next);
            next = line.range.end;
        }
        prop_assert_eq!(next, layout.text().len());
    }

    #[test]
    fn model_edits_match_a_plain_string(
        initial in arb_text(),
        edits in proptest::collection::vec((0usize..80, 0usize..80, arb_text()), 0..8),
    ) {
        let mut rich = RichText::plain(&initial);
        let mut reference = rich.plain_text();
        for (a, b, insert) in edits {
            let snap = |s: &str, mut i: usize| {
                i = i.min(s.len());
                while !s.is_char_boundary(i) { i = i.saturating_sub(1); }
                i
            };
            let (lo, hi) = (a.min(b), a.max(b));
            let (lo, hi) = (snap(&reference, lo), snap(&reference, hi));
            reference.replace_range(lo..hi, &insert);
            rich.replace(a.min(b)..a.max(b), &insert);
            prop_assert_eq!(rich.plain_text(), reference.clone());
            prop_assert_eq!(rich.text_len(), reference.len());
            prop_assert!(!rich.paragraphs.is_empty());
        }
    }
}

/// Parley asserts (a panic in debug builds) when a combining mark, joiner or variation selector follows a line break;
/// the engine rewrites such characters into invisible ones of the same length, so the rest of the text is still shaped
/// normally (and not replaced by the last-resort placeholders).
#[test]
fn marks_and_joiners_after_a_line_break_do_not_derail_shaping() {
    let Some(mut engine) = common::engine_for(&[anim_text::Script::Hebrew]) else { return };
    for (text, mark_len) in
        [("\n\u{301}", 2), ("\n\u{64C}", 2), ("\n\u{200D}", 3), ("\r\u{FE0F}", 3), ("\u{2028}\u{301}", 2), ("\n\u{903}", 3), ("\n\u{E0100}", 4)]
    {
        let full = format!("a{text}שלום b");
        let layout = engine.layout(&RichText::new(vec![Paragraph::from_text(full.clone())]), Some(300.0));
        assert_eq!(layout.text(), full, "offsets are the author's offsets");
        assert!(layout.runs.iter().any(|r| r.rtl && r.glyphs.iter().all(|g| g.id != 0)), "the Hebrew after {text:?} is shaped for real");
        assert!(layout.lines.len() >= 2, "{text:?} still breaks the line");
        let hebrew_start = full.find('ש').unwrap();
        assert_eq!(hebrew_start, 1 + text.len(), "{mark_len}");
        let caret = layout.caret_at(hebrew_start);
        assert!(layout.caret_rect(caret, 1.0).is_some());
    }
}

/// Found by the soak test: parley splits the text range of a multi-character cluster (a letter followed by a
/// zero-width joiner) evenly by bytes, so a line break between its components landed inside a character. Every offset
/// the layout reports is snapped back to a character boundary.
#[test]
fn offsets_stay_on_character_boundaries_when_parley_splits_a_cluster() {
    // The Han characters get the last-resort glyph with the bundled fonts, which is as good a cluster for this test.
    let Some(mut engine) = common::engine_for(&[anim_text::Script::Hebrew]) else { return };
    let run = |text: &str, letter_spacing: f64| Run { text: text.into(), letter_spacing, ..Run::default() };
    let text = RichText::new(vec![Paragraph {
        direction: Direction::Ltr,
        runs: vec![
            run("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0.0),
            run("a中aaaaaaaaaaaaaaaa\u{2028}aaaaa\u{200d}ש中aaaaaaaaaaaaaaaaaaaaaaaa", 0.0),
            run("aaaaaaaaaaשaaaaaaaaaaaaaaaaaaaaaaaaaaa中\u{200d}\u{301}", 163.7),
        ],
        ..Paragraph::default()
    }]);
    for width in [1.0, 5.0, 40.0, 1e300] {
        let layout = engine.layout(&text, Some(width));
        let on_boundary = |o: usize| layout.text().is_char_boundary(o);
        for line in &layout.lines {
            assert!(on_boundary(line.range.start) && on_boundary(line.range.end), "width {width}: line {:?}", line.range);
        }
        for run in &layout.runs {
            assert!(on_boundary(run.range.start) && on_boundary(run.range.end), "width {width}: run {:?}", run.range);
        }
        for offset in (0..=layout.text().len()).filter(|&o| on_boundary(o)) {
            for motion in [Motion::Left, Motion::Right, Motion::LineStart, Motion::LineEnd, Motion::WordRight] {
                let moved = layout.move_caret(Caret::new(offset), motion, None);
                assert!(on_boundary(moved.offset), "width {width}: {motion:?} from {offset} gave {moved:?}");
            }
            let word = layout.word_range_at(offset);
            assert!(on_boundary(word.start) && on_boundary(word.end));
        }
    }
}

#[test]
fn word_motion_always_terminates() {
    let Some(mut engine) = engine() else { return };
    let text = RichText::new(vec![Paragraph::from_text("a\u{200F}b \u{202E}c d\u{202C} \u{2067}e f\u{2069} שלום עולם \u{200D}x\ny z".repeat(8))]);
    let layout = engine.layout(&text, Some(120.0));
    let len = layout.text().len();
    let started = std::time::Instant::now();
    for offset in (0..=len).filter(|&o| layout.text().is_char_boundary(o)) {
        for motion in [Motion::WordLeft, Motion::WordRight] {
            for affinity in [Affinity::Downstream, Affinity::Upstream] {
                let moved = layout.move_caret(Caret { offset, affinity }, motion, None);
                assert!(moved.offset <= len);
            }
        }
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
}

#[test]
fn enormous_inputs_are_truncated_not_fatal() {
    let Some(mut engine) = engine() else { return };
    // Too many paragraphs for the cap would take long to shape; a single paragraph over the byte cap is rejected quickly.
    let huge = "x".repeat(anim_text::MAX_TEXT_BYTES + 1);
    let layout = engine.layout(&RichText::new(vec![Paragraph::from_text("kept"), Paragraph::from_text(huge)]), Some(100.0));
    assert!(layout.is_truncated());
    assert_eq!(layout.text(), "kept");
    assert!(layout.caret_rect(layout.caret_at(0), 1.0).is_some());
}

/// Found when the tests moved from system fonts to the bundled one: for a text with no cluster at the offset, the word
/// range was the offset itself, past the end of the text.
#[test]
fn word_ranges_stay_inside_the_text_for_glyphless_and_far_offsets() {
    let Some(mut engine) = engine() else { return };
    // The second engine has no font at all, so no text has glyphs.
    let mut fontless = anim_text::TextEngine::without_system_fonts();
    for text in ["\u{feff}", "\u{200b}", "\n", "a\u{feff}", "abc"] {
        for engine in [&mut engine, &mut fontless] {
            let layout = engine.layout(&RichText::plain(text), None);
            let len = layout.text().len();
            for offset in 0..12 {
                let word = layout.word_range_at(offset);
                assert!(word.start <= word.end && word.end <= len, "{text:?} at {offset}: {word:?} (len {len})");
            }
        }
    }
}

#[test]
fn a_long_paragraph_lays_out_in_reasonable_time() {
    let Some(mut engine) = engine() else { return };
    let text = "The quick brown fox jumps over the lazy dog. ".repeat(400);
    let start = std::time::Instant::now();
    let layout = engine.layout(&RichText::plain(&text), Some(400.0));
    assert!(layout.lines.len() > 40);
    assert!(start.elapsed() < std::time::Duration::from_secs(20), "{:?}", start.elapsed());
}
