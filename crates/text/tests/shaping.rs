//! Latin, Hebrew and Arabic shaping, and mixed-direction (bidi) visual order.

mod common;

use anim_text::{Alignment, Direction, Paragraph, RichText, Run, Script};
use common::{ARABIC, HEBREW, engine, engine_for, glyph_ids, one, para, run, visual_runs};

#[test]
fn latin_text_is_one_left_to_right_run_with_a_glyph_per_character() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("Hello World"), None);
    assert_eq!(layout.text(), "Hello World");
    assert_eq!(layout.lines.len(), 1);
    assert_eq!(layout.runs.len(), 1);
    let run = &layout.runs[0];
    assert!(!run.rtl);
    assert_eq!(run.glyphs.len(), 11);
    assert_eq!(run.range, 0..11);
    assert!(run.glyphs.windows(2).all(|w| w[0].x < w[1].x), "glyphs advance to the right");
    assert!(run.glyphs.iter().all(|g| g.id != 0), "no missing glyphs");
    assert!(layout.bounds.width() > 50.0 && layout.bounds.height() > 15.0);
    assert!((run.size - 20.0).abs() < f64::EPSILON);
}

#[test]
fn glyph_positions_scale_with_the_font_size() {
    let Some(mut engine) = engine() else { return };
    let width = |size: f64, engine: &mut anim_text::TextEngine| {
        let text = RichText::new(vec![Paragraph::new(vec![Run { size, ..Run::new("Typography") }])]);
        engine.layout(&text, None).bounds.width()
    };
    let (small, big) = (width(10.0, &mut engine), width(40.0, &mut engine));
    assert!((big / small - 4.0).abs() < 0.05, "{small} vs {big}");
}

#[test]
fn bold_and_italic_change_the_shaped_glyphs_or_widths() {
    let Some(mut engine) = engine() else { return };
    let plain = engine.layout(&one("Quick brown fox"), None);
    let bold = engine.layout(&RichText::new(vec![Paragraph::new(vec![Run { bold: true, ..run("Quick brown fox") }])]), None);
    let italic = engine.layout(&RichText::new(vec![Paragraph::new(vec![Run { italic: true, ..run("Quick brown fox") }])]), None);
    // Glyph ids can be shared between the weights of one family, so look at the font face, the synthesis flags and widths.
    // A variable system font (macOS) keeps one face and changes the variation coordinates instead.
    let differs = |a: &anim_text::GlyphRun, b: &anim_text::GlyphRun| a.font.id != b.font.id || a.font.coords != b.font.coords;
    assert!(bold.runs[0].embolden || differs(&bold.runs[0], &plain.runs[0]), "bold must come from a bold face or be synthesised");
    assert!(italic.runs[0].skew_degrees > 0.0 || differs(&italic.runs[0], &plain.runs[0]), "italic must come from an italic face or be synthesised");
    assert!(plain.runs[0].skew_degrees == 0.0 && !plain.runs[0].embolden);
    assert_eq!(glyph_ids(&plain).len(), glyph_ids(&bold).len());
}

#[test]
fn hebrew_runs_right_to_left_with_the_first_letter_rightmost() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one(HEBREW), None);
    assert_eq!(layout.runs.len(), 1);
    let run = &layout.runs[0];
    assert!(run.rtl);
    assert!(layout.lines[0].rtl);
    assert!(run.glyphs.iter().all(|g| g.id != 0), "Hebrew drawn with a font that has it");
    // Logical first character (shin) is at the right end of the line, the last one (mem) at the left end.
    let first = layout.caret_rect(layout.caret_at(0), 1.0).unwrap();
    let last = layout.caret_rect(layout.caret_at(HEBREW.len()), 1.0).unwrap();
    assert!(first.x0 > last.x0 + 40.0, "start of the text is on the right: {first:?} {last:?}");
}

#[test]
fn hebrew_space_is_between_the_two_words_in_visual_order() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one(HEBREW), None);
    let glyphs = &layout.runs[0].glyphs;
    // 9 characters, 9 glyphs: 4 letters, space, 4 letters. Visual order: second word, space, first word.
    assert_eq!(glyphs.len(), 9);
    let space = glyphs[4].id;
    assert_eq!(glyphs.iter().filter(|g| g.id == space).count(), 1);
    assert!(glyphs[0].id != space && glyphs[8].id != space);
}

#[test]
fn arabic_letters_take_their_joining_forms() {
    let Some(mut engine) = engine_for(&[Script::Arabic]) else { return };
    // Three BEH: initial, medial and final forms, none of which is the isolated form.
    let isolated = glyph_ids(&engine.layout(&one("ب"), None));
    let joined = glyph_ids(&engine.layout(&one("ببب"), None));
    assert_eq!(isolated.len(), 1);
    assert_eq!(joined.len(), 3);
    assert!(joined.iter().all(|id| *id != isolated[0]), "joined forms differ from the isolated form: {isolated:?} {joined:?}");
    assert!(joined.iter().all(|id| *id != 0));
    // Same letter, so same glyphs for the same position: initial != final.
    assert_ne!(joined[0], joined[2]);
}

#[test]
fn arabic_lam_alef_forms_a_ligature() {
    let Some(mut engine) = engine_for(&[Script::Arabic]) else { return };
    let layout = engine.layout(&one("لا"), None);
    assert_eq!(layout.runs.len(), 1);
    // Most fonts have a lam-alef ligature glyph; some system fonts (Windows) draw two joined glyphs instead. Either
    // way the pair is one cluster of one run, never more than two glyphs.
    let glyphs = layout.runs[0].glyphs.len();
    assert!((1..=2).contains(&glyphs), "two letters shape to at most two glyphs, got {glyphs}");
    assert!(layout.runs[0].rtl);
    assert_eq!(layout.runs[0].range, 0..4);
}

#[test]
fn letter_spacing_leaves_arabic_joined_but_spaces_latin() {
    let Some(mut engine) = engine_for(&[Script::Arabic]) else { return };
    let spaced = |text: &str| RichText::new(vec![Paragraph::new(vec![Run { letter_spacing: 6.0, ..run(text) }])]);
    let word = ARABIC.split(' ').next().unwrap();
    let arabic_plain = engine.layout(&one(word), None);
    let arabic_spaced = engine.layout(&spaced(word), None);
    assert_eq!(glyph_ids(&arabic_plain), glyph_ids(&arabic_spaced));
    assert!((arabic_plain.bounds.width() - arabic_spaced.bounds.width()).abs() < 0.5, "cursive text is not letter-spaced");
    // The space between two Arabic words is not cursive, so it does get the spacing.
    let two_plain = engine.layout(&one(ARABIC), None);
    let two_spaced = engine.layout(&spaced(ARABIC), None);
    assert!((two_spaced.bounds.width() - two_plain.bounds.width() - 6.0).abs() < 0.5, "{} vs {}", two_spaced.bounds.width(), two_plain.bounds.width());
    let latin_plain = engine.layout(&one("spacing"), None);
    let latin_spaced = engine.layout(&spaced("spacing"), None);
    assert!(latin_spaced.bounds.width() > latin_plain.bounds.width() + 30.0, "7 letters x 6 px");
}

#[test]
fn mixed_paragraph_reorders_the_hebrew_run_only() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one("abc שלום def"), None);
    let visual = common::visual_spans(&layout);
    assert_eq!(visual, vec![(0..4, false), (4..12, true), (12..16, false)], "{visual:?}");
    // Glyph x positions strictly increase across the whole line.
    let xs: Vec<f64> = {
        let mut runs: Vec<_> = layout.runs.iter().collect();
        runs.sort_by(|a, b| a.origin.x.total_cmp(&b.origin.x));
        runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.x)).collect()
    };
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
}

#[test]
fn right_to_left_paragraph_puts_latin_words_in_reading_order_from_the_right() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    // Logical: Hebrew word, "abc", Hebrew word. In an RTL paragraph the first Hebrew word is rightmost.
    let layout = engine.layout(&para("שלום abc גדול", Alignment::Start, Direction::Rtl), None);
    let visual = visual_runs(&layout);
    // Left to right on screen: the second Hebrew word (bytes 13..21), "abc" in the middle, the first word (byte 0) at the
    // right edge.
    assert!(visual.first().is_some_and(|(r, rtl)| *rtl && r.contains(&13)), "{visual:?}");
    assert!(visual.last().is_some_and(|(r, rtl)| *rtl && r.contains(&0)), "{visual:?}");
    assert!(layout.lines[0].rtl);
    assert!(layout.runs.iter().any(|r| !r.rtl && r.range.contains(&9)), "abc is a left-to-right run inside");
}

#[test]
fn explicit_direction_overrides_the_first_strong_character() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let auto = engine.layout(&para("123 שלום", Alignment::Start, Direction::Auto), Some(200.0));
    let ltr = engine.layout(&para("שלום 123", Alignment::Start, Direction::Ltr), Some(200.0));
    let rtl = engine.layout(&para("abc def", Alignment::Start, Direction::Rtl), Some(200.0));
    let plain = engine.layout(&para("abc def", Alignment::Start, Direction::Auto), Some(200.0));
    assert!(auto.lines[0].rtl, "digits are weak: the first strong character, Hebrew, decides");
    assert!(!ltr.lines[0].rtl, "Hebrew first, but the paragraph is forced left to right");
    assert!(rtl.lines[0].rtl, "Latin text in a paragraph forced right to left");
    assert!(!plain.lines[0].rtl);
    // A forced-RTL paragraph is start-aligned to the right, a forced-LTR one to the left.
    assert!(rtl.lines[0].rect.x1 > 199.0, "{:?}", rtl.lines[0].rect);
    assert!(ltr.lines[0].rect.x0 < 1.0, "{:?}", ltr.lines[0].rect);
    // The direction mark must not leak into the offsets: caret 0 is at the start of the visible text.
    assert_eq!(rtl.text(), "abc def");
    assert_eq!(rtl.caret_at(0).offset, 0);
    // The Latin words keep their own order inside the right-aligned line: "a" is at the line's left end.
    let first_letter = rtl.caret_rect(rtl.caret_at(0), 1.0).unwrap();
    assert!(common::close(first_letter.x0, rtl.lines[0].rect.x0, 0.5), "{first_letter:?} vs {:?}", rtl.lines[0].rect);
}

#[test]
fn brackets_are_mirrored_in_right_to_left_text() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    // Logical "(ש)" in a right-to-left line: the opening bracket is at the right and is drawn with the closing glyph,
    // so on screen the line looks exactly like the left-to-right line "(ש)" (same text pieces, hence the same fonts on
    // any system) and unlike ")ש(".
    let rtl = engine.layout(&para("(ש)", Alignment::Start, Direction::Rtl), None);
    let ltr = engine.layout(&para("(ש)", Alignment::Left, Direction::Ltr), None);
    let swapped = engine.layout(&para(")ש(", Alignment::Left, Direction::Ltr), None);
    let visual = glyph_ids(&rtl);
    assert_eq!(visual.len(), 3);
    assert_eq!(visual, glyph_ids(&ltr), "mirrored brackets");
    assert_ne!(visual, glyph_ids(&swapped), "the bracket glyphs are swapped, not kept");
}

#[test]
fn numbers_inside_hebrew_stay_left_to_right() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one("שלום 2024 עולם"), None);
    let digits = layout.runs.iter().find(|r| !r.rtl && r.range.start >= 9 && r.range.end <= 14);
    assert!(digits.is_some(), "digits form a left-to-right run: {:?}", visual_runs(&layout));
}

#[test]
fn colours_and_sizes_split_runs_without_changing_the_text() {
    let Some(mut engine) = engine() else { return };
    let red = anim_core::Rgba8::opaque(255, 0, 0);
    let text = RichText::new(vec![Paragraph::new(vec![Run { color: red, ..run("red ") }, Run { size: 40.0, ..run("big") }])]);
    let layout = engine.layout(&text, None);
    assert_eq!(layout.text(), "red big");
    assert!(layout.runs.iter().any(|r| r.color == red && r.range.start == 0));
    assert!(layout.runs.iter().any(|r| (r.size - 40.0).abs() < 1e-6 && r.range.start >= 4));
    let line = &layout.lines[0];
    // Font metrics differ per system (a 40 px line can be a bit under 40 px tall), so compare with the small run alone.
    let small = engine.layout(&one("red "), None);
    assert!(line.rect.height() > small.lines[0].rect.height() * 1.4, "the line is as tall as its biggest run");
}
