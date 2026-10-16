//! Alignment, wrapping, margins, indents, line spacing and paragraph stacking.

use crate::common;

use anim_text::{Alignment, Direction, Paragraph, RichText, Script};
use common::{HEBREW, close, engine, engine_for, one, para, run};

const WIDTH: f64 = 300.0;
const SENTENCE: &str = "The quick brown fox jumps over the lazy dog and keeps running through the quiet forest";

#[test]
fn left_right_and_center_alignment_place_a_short_line() {
    let Some(mut engine) = engine() else { return };
    let line = |alignment, engine: &mut anim_text::TextEngine| {
        let layout = engine.layout(&para("Hello World", alignment, Direction::Ltr), Some(WIDTH));
        assert_eq!(layout.bounds.width(), WIDTH);
        layout.lines[0].rect
    };
    let left = line(Alignment::Left, &mut engine);
    let right = line(Alignment::Right, &mut engine);
    let center = line(Alignment::Center, &mut engine);
    assert!(close(left.x0, 0.0, 0.01), "{left:?}");
    assert!(close(right.x1, WIDTH, 0.01), "{right:?}");
    assert!(close(center.center().x, WIDTH / 2.0, 0.01), "{center:?}");
    assert!(close(left.width(), right.width(), 0.01) && close(left.width(), center.width(), 0.01));
}

#[test]
fn start_and_end_follow_the_paragraph_direction() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let line =
        |text: &str, alignment, direction, engine: &mut anim_text::TextEngine| engine.layout(&para(text, alignment, direction), Some(WIDTH)).lines[0].rect;
    let ltr_start = line("Hello", Alignment::Start, Direction::Ltr, &mut engine);
    let ltr_end = line("Hello", Alignment::End, Direction::Ltr, &mut engine);
    let rtl_start = line(HEBREW, Alignment::Start, Direction::Rtl, &mut engine);
    let rtl_end = line(HEBREW, Alignment::End, Direction::Rtl, &mut engine);
    assert!(close(ltr_start.x0, 0.0, 0.01));
    assert!(close(ltr_end.x1, WIDTH, 0.01));
    assert!(close(rtl_start.x1, WIDTH, 0.01), "RTL start is the right edge: {rtl_start:?}");
    assert!(close(rtl_end.x0, 0.0, 0.01), "RTL end is the left edge: {rtl_end:?}");
    // Physical alignment ignores the direction.
    let rtl_left = line(HEBREW, Alignment::Left, Direction::Rtl, &mut engine);
    assert!(close(rtl_left.x0, 0.0, 0.01));
}

#[test]
fn justify_fills_every_line_but_the_last() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&para(SENTENCE, Alignment::Justify, Direction::Ltr), Some(WIDTH));
    assert!(layout.lines.len() >= 3, "{} lines", layout.lines.len());
    let (last, others) = layout.lines.split_last().unwrap();
    for line in others {
        assert!(close(line.rect.x0, 0.0, 0.01) && close(line.rect.x1, WIDTH, 0.6), "{:?}", line.rect);
    }
    assert!(last.rect.x1 < WIDTH - 5.0, "the last line stays ragged: {:?}", last.rect);
}

#[test]
fn wrapping_breaks_at_word_boundaries_and_covers_every_character() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one(SENTENCE), Some(WIDTH));
    assert!(layout.lines.len() >= 3);
    let mut next = 0;
    for line in &layout.lines {
        assert_eq!(line.range.start, next, "line ranges tile the text");
        assert!(line.range.end > line.range.start);
        assert!(line.rect.x1 <= WIDTH + 0.01, "{:?}", line.rect);
        let text = SENTENCE.get(line.range.clone()).unwrap();
        assert!(!text.starts_with(' '), "{text:?}");
        next = line.range.end;
    }
    assert_eq!(next, SENTENCE.len());
    assert!(layout.lines.windows(2).all(|w| w[0].baseline < w[1].baseline));
    assert!(close(layout.bounds.height(), layout.lines.last().unwrap().rect.y1, 0.01));
}

#[test]
fn without_a_width_only_forced_breaks_make_new_lines() {
    let Some(mut engine) = engine() else { return };
    let long = engine.layout(&one(SENTENCE), None);
    assert_eq!(long.lines.len(), 1);
    assert!(long.bounds.width() > 400.0);
    let paragraphs = engine.layout(&RichText::plain("one\ntwo"), None);
    assert_eq!(paragraphs.paragraphs.len(), 2);
    let inside_run = RichText::new(vec![Paragraph::new(vec![run("one\ntwo")])]);
    let layout = engine.layout(&inside_run, None);
    assert_eq!(layout.paragraphs.len(), 1);
    assert_eq!(layout.lines.len(), 2, "a newline inside a run is a forced line break");
}

#[test]
fn a_tiny_or_invalid_width_still_terminates_with_one_character_per_line_at_most() {
    let Some(mut engine) = engine() else { return };
    for width in [0.0, -5.0, f64::NAN, f64::INFINITY, 1.0, 0.001] {
        let layout = engine.layout(&one("abcdef ghij"), Some(width));
        assert_ne!(layout.lines.len(), 0);
        assert!(layout.lines.len() <= 11, "width {width}: {} lines", layout.lines.len());
        assert!(layout.bounds.width().is_finite() && layout.bounds.height().is_finite());
    }
}

#[test]
fn margins_shrink_the_box_the_text_aligns_in() {
    let Some(mut engine) = engine() else { return };
    let text = RichText::new(vec![Paragraph {
        alignment: Alignment::Right,
        direction: Direction::Ltr,
        margin_left: 20.0,
        margin_right: 30.0,
        ..Paragraph::new(vec![run("Hello World")])
    }]);
    let layout = engine.layout(&text, Some(WIDTH));
    assert!(close(layout.lines[0].rect.x1, WIDTH - 30.0, 0.01), "{:?}", layout.lines[0].rect);
    let wrapped = RichText::new(vec![Paragraph { margin_left: 20.0, margin_right: 30.0, ..Paragraph::new(vec![run(SENTENCE)]) }]);
    let layout = engine.layout(&wrapped, Some(WIDTH));
    for line in &layout.lines {
        assert!(line.rect.x0 >= 20.0 - 0.01 && line.rect.x1 <= WIDTH - 30.0 + 0.01, "{:?}", line.rect);
    }
    assert!(layout.lines.len() > 3, "less room, more lines");
}

#[test]
fn first_line_indent_applies_to_the_start_edge_only() {
    let Some(mut engine) = engine() else { return };
    let indented = |indent: f64| RichText::new(vec![Paragraph { indent, direction: Direction::Ltr, ..Paragraph::new(vec![run(SENTENCE)]) }]);
    let layout = engine.layout(&indented(40.0), Some(WIDTH));
    assert!(close(layout.lines[0].rect.x0, 40.0, 0.01), "{:?}", layout.lines[0].rect);
    assert!(close(layout.lines[1].rect.x0, 0.0, 0.01));
    // Hanging indent: the first line sticks out to the left of the others.
    let hanging = RichText::new(vec![Paragraph { indent: -15.0, margin_left: 40.0, direction: Direction::Ltr, ..Paragraph::new(vec![run(SENTENCE)]) }]);
    let layout = engine.layout(&hanging, Some(WIDTH));
    assert!(close(layout.lines[1].rect.x0 - layout.lines[0].rect.x0, 15.0, 0.01), "{:?}", layout.lines);
}

#[test]
fn indent_of_a_right_to_left_paragraph_is_on_the_right() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let text = RichText::new(vec![Paragraph { indent: 40.0, direction: Direction::Rtl, ..Paragraph::new(vec![run(HEBREW)]) }]);
    let layout = engine.layout(&text, Some(WIDTH));
    assert!(close(layout.lines[0].rect.x1, WIDTH - 40.0, 0.01), "{:?}", layout.lines[0].rect);
}

#[test]
fn line_spacing_adds_space_between_lines_and_after_the_paragraph() {
    let Some(mut engine) = engine() else { return };
    let spaced =
        |spacing: f64| RichText::new(vec![Paragraph { line_spacing: spacing, ..Paragraph::new(vec![run(SENTENCE)]) }, Paragraph::new(vec![run("next")])]);
    let base = engine.layout(&spaced(0.0), Some(WIDTH));
    let wide = engine.layout(&spaced(10.0), Some(WIDTH));
    assert_eq!(base.lines.len(), wide.lines.len());
    for (i, (a, b)) in base.lines.iter().zip(&wide.lines).enumerate() {
        let shift = 10.0 * f64::from(u8::try_from(i).unwrap());
        assert!(close(b.baseline - a.baseline, shift, 0.01), "line {i}: {} vs {}", a.baseline, b.baseline);
    }
    // The second paragraph starts after the last line plus its spacing.
    let n = f64::from(u8::try_from(base.paragraphs[0].lines.len()).unwrap());
    let (first, second) = (&base.paragraphs[1], &wide.paragraphs[1]);
    assert!(close(second.rect.y0 - first.rect.y0, 10.0 * n, 0.01));
    // A hit test lands on the shifted lines.
    let mid = wide.lines[1].rect.center();
    let hit = wide.hit_test(mid);
    assert!(wide.lines[1].range.contains(&hit), "{hit} not in {:?}", wide.lines[1].range);
}

#[test]
fn negative_line_spacing_tightens_the_lines() {
    let Some(mut engine) = engine() else { return };
    let tight = RichText::new(vec![Paragraph { line_spacing: -4.0, ..Paragraph::new(vec![run(SENTENCE)]) }]);
    let normal = RichText::new(vec![Paragraph::new(vec![run(SENTENCE)])]);
    let a = engine.layout(&tight, Some(WIDTH));
    let b = engine.layout(&normal, Some(WIDTH));
    assert!(a.bounds.height() < b.bounds.height() - 4.0);
}

#[test]
fn paragraphs_stack_top_to_bottom_with_their_own_attributes() {
    let Some(mut engine) = engine() else { return };
    let text = RichText::new(vec![
        Paragraph { alignment: Alignment::Left, direction: Direction::Ltr, ..Paragraph::new(vec![run("First")]) },
        Paragraph { alignment: Alignment::Right, direction: Direction::Ltr, ..Paragraph::new(vec![run("Second")]) },
        Paragraph { alignment: Alignment::Center, direction: Direction::Ltr, ..Paragraph::new(vec![run("Third")]) },
    ]);
    let layout = engine.layout(&text, Some(WIDTH));
    assert_eq!(layout.text(), "First\nSecond\nThird");
    assert_eq!(layout.paragraphs.len(), 3);
    assert_eq!(layout.paragraphs[1].range, 6..12);
    assert!(layout.lines.windows(2).all(|w| w[0].rect.y1 <= w[1].rect.y0 + 0.01));
    assert!(close(layout.lines[0].rect.x0, 0.0, 0.01));
    assert!(close(layout.lines[1].rect.x1, WIDTH, 0.01));
    assert!(close(layout.lines[2].rect.center().x, WIDTH / 2.0, 0.01));
    assert_eq!(layout.lines.iter().map(|l| l.paragraph).collect::<Vec<_>>(), vec![0, 1, 2]);
}

#[test]
fn without_a_width_paragraphs_align_inside_the_widest_one() {
    let Some(mut engine) = engine() else { return };
    let text = RichText::new(vec![
        Paragraph { direction: Direction::Ltr, ..Paragraph::new(vec![run("a much longer first paragraph")]) },
        Paragraph { alignment: Alignment::Right, direction: Direction::Ltr, ..Paragraph::new(vec![run("short")]) },
        Paragraph { alignment: Alignment::Center, direction: Direction::Ltr, ..Paragraph::new(vec![run("mid")]) },
    ]);
    let layout = engine.layout(&text, None);
    let widest = layout.lines[0].rect.x1;
    assert!(close(layout.bounds.width(), widest, 0.05));
    assert!(close(layout.lines[1].rect.x1, widest, 0.05), "right-aligned to the widest line: {:?}", layout.lines[1].rect);
    assert!(close(layout.lines[2].rect.center().x, widest / 2.0, 0.05));
}

#[test]
fn empty_texts_still_have_a_line_for_the_caret() {
    let Some(mut engine) = engine() else { return };
    for text in [RichText::default(), RichText::new(vec![Paragraph::default()]), one(""), RichText::plain("\n\n")] {
        let layout = engine.layout(&text, Some(100.0));
        assert!(!layout.lines.is_empty(), "{text:?}");
        let caret = layout.caret_at(0);
        let rect = layout.caret_rect(caret, 1.0).unwrap();
        assert!(rect.height() > 5.0, "{rect:?}");
    }
    let three = engine.layout(&RichText::plain("\n\n"), Some(100.0));
    assert_eq!(three.paragraphs.len(), 3);
    assert_eq!(three.lines.len(), 3);
}

#[test]
fn tabs_and_carriage_returns_do_not_shift_offsets() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&RichText::new(vec![Paragraph::new(vec![run("a\tb\r\nc")])]), None);
    assert_eq!(layout.text().len(), 6);
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[1].range.start, 5);
}
