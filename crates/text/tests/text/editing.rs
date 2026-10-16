//! Hit testing, caret and selection geometry, and visual caret movement in left-to-right, right-to-left and mixed text.

use crate::common;

use anim_core::Point;
use anim_text::{Affinity, Alignment, Caret, Direction, Motion, Paragraph, RichText, Script, Selection};
use common::{HEBREW, boundaries, close, engine, engine_for, one, para, run};

const MIXED: &str = "abc שלום def";
const SENTENCE: &str = "The quick brown fox jumps over the lazy dog and keeps running";

#[test]
fn carets_in_latin_text_advance_with_the_offset() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("Hello World"), None);
    let xs: Vec<f64> = boundaries(&layout).iter().map(|&o| layout.caret_rect(layout.caret_at(o), 1.0).unwrap().x0).collect();
    assert_eq!(xs.len(), 12);
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    assert!(close(xs[0], 0.0, 0.01));
    assert!(close(xs[11], layout.bounds.width(), 0.01));
    let rect = layout.caret_rect(layout.caret_at(3), 2.0).unwrap();
    assert!(close(rect.width(), 2.0, 0.01));
    assert!(close(rect.height(), layout.lines[0].rect.height(), 0.5), "caret as tall as the line");
}

#[test]
fn hit_testing_finds_the_nearest_character_boundary() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("Hello World"), None);
    let y = layout.lines[0].rect.center().y;
    for o in boundaries(&layout) {
        let x = layout.caret_rect(layout.caret_at(o), 1.0).unwrap().x0;
        assert_eq!(layout.hit_test(Point::new(x, y)), o, "offset {o} at x={x}");
        assert_eq!(layout.hit_test(Point::new(x + 0.4, y)), o);
    }
    assert_eq!(layout.hit_test(Point::new(-50.0, y)), 0, "left of the text");
    assert_eq!(layout.hit_test(Point::new(5000.0, y)), 11, "right of the text");
    assert_eq!(layout.hit_test(Point::new(2.0, -100.0)), 0, "above the first line");
    assert_eq!(layout.hit_test(Point::new(2.0, 5000.0)), 0, "below a single line: its column");
    // Closer to the left or the right half of a letter decides the side.
    let a = layout.caret_rect(layout.caret_at(1), 0.0).unwrap().x0;
    let b = layout.caret_rect(layout.caret_at(2), 0.0).unwrap().x0;
    assert_eq!(layout.hit_test(Point::new((b - a).mul_add(0.25, a), y)), 1);
    assert_eq!(layout.hit_test(Point::new((b - a).mul_add(0.75, a), y)), 2);
}

#[test]
fn hit_testing_in_hebrew_counts_from_the_right() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one(HEBREW), None);
    let y = layout.lines[0].rect.center().y;
    assert_eq!(layout.hit_test(Point::new(-10.0, y)), HEBREW.len(), "the left edge is the end of the text");
    assert_eq!(layout.hit_test(Point::new(layout.bounds.width() + 10.0, y)), 0, "the right edge is its start");
    for o in boundaries(&layout) {
        let x = layout.caret_rect(layout.caret_at(o), 1.0).unwrap().x0;
        assert_eq!(layout.hit_test(Point::new(x, y)), o, "offset {o}");
    }
}

#[test]
fn hit_testing_in_mixed_text_lands_on_the_nearest_caret_position() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one(MIXED), None);
    let y = layout.lines[0].rect.center().y;
    for step in 0..400u16 {
        let x = f64::from(step).mul_add(1.5, -5.0);
        if x > layout.bounds.width() + 5.0 {
            break;
        }
        let caret = layout.hit_test_caret(Point::new(x, y));
        let at = layout.caret_rect(caret, 0.0).unwrap().x0;
        // No other boundary is closer to the click than the one chosen.
        let best = boundaries(&layout)
            .iter()
            .flat_map(|&o| [Affinity::Downstream, Affinity::Upstream].map(|affinity| layout.caret_rect(Caret { offset: o, affinity }, 0.0).unwrap().x0))
            .map(|cx| (cx - x).abs())
            .fold(f64::INFINITY, f64::min);
        assert!((at - x).abs() <= best + 1e-6, "x={x}: chose {caret:?} at {at}, best distance {best}");
    }
}

#[test]
fn selection_rectangles_cover_the_selected_characters() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("Hello World"), None);
    let rects = layout.selection_rects(2..7);
    assert_eq!(rects.len(), 1);
    let (a, b) = (layout.caret_rect(layout.caret_at(2), 0.0).unwrap().x0, layout.caret_rect(layout.caret_at(7), 0.0).unwrap().x0);
    assert!(close(rects[0].x0, a, 0.01) && close(rects[0].x1, b, 0.01), "{:?} vs {a}..{b}", rects[0]);
    assert!(close(rects[0].height(), layout.lines[0].rect.height(), 0.5));
    assert_eq!(layout.selection_rects(3..3).len(), 0);
    assert_eq!(layout.selection_rects(0..100).len(), 1, "ranges are clamped");
}

#[test]
fn a_selected_paragraph_break_is_a_narrow_rectangle_after_the_paragraph() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&RichText::plain("one\ntwo\n\nfour"), Some(300.0));
    // "e\nt": the end of "one", its break and the start of "two".
    let rects = layout.selection_rects(2..5);
    assert_eq!(rects.len(), 3, "{rects:?}");
    let end_of_one = layout.caret_rect(Caret { offset: 3, affinity: Affinity::Upstream }, 0.0).unwrap();
    let marker = rects.iter().find(|r| close(r.x0, end_of_one.x0, 0.01) && r.width() > 2.0 && r.y0 < layout.lines[1].rect.y0).expect("a marker after the text");
    assert!(close(marker.width(), end_of_one.height() * 0.25, 0.01));
    // The break of an empty paragraph is selectable too, and the last paragraph has none.
    assert_eq!(layout.selection_rects(8..9).len(), 1);
    assert_eq!(layout.selection_rects(9..13).len(), 1);
    assert_eq!(layout.selection_rects(0..layout.text().len()).len(), 6, "four texts' worth: three words, two breaks and one empty paragraph's break");
    // In a right-to-left paragraph the marker is on the left of the text.
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let rtl = engine
        .layout(&RichText::new(vec![Paragraph { direction: Direction::Rtl, ..Paragraph::new(vec![run(HEBREW)]) }, Paragraph::from_text("x")]), Some(300.0));
    let text_len = HEBREW.len();
    let marker = rtl.selection_rects(text_len..text_len + 1);
    assert_eq!(marker.len(), 1);
    assert!(marker[0].x1 <= rtl.lines[0].rect.x0 + 0.01, "{:?} vs {:?}", marker[0], rtl.lines[0].rect);
}

#[test]
fn selection_across_wrapped_lines_gives_a_rectangle_per_line() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one(SENTENCE), Some(150.0));
    assert!(layout.lines.len() >= 3);
    let rects = layout.selection_rects(2..SENTENCE.len() - 2);
    assert_eq!(rects.len(), layout.lines.len(), "{rects:?}");
    assert!(rects.windows(2).all(|w| w[0].y1 <= w[1].y0 + 0.01));
}

#[test]
fn selecting_across_a_direction_change_highlights_two_separate_stretches() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one(MIXED), None);
    // "c " then the first two Hebrew letters: logically contiguous, visually two pieces.
    let mut rects = layout.selection_rects(2..8);
    rects.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    assert_eq!(rects.len(), 2, "{rects:?}");
    let x = |o, affinity| layout.caret_rect(Caret { offset: o, affinity }, 0.0).unwrap().x0;
    assert!(close(rects[0].x0, x(2, Affinity::Downstream), 0.01));
    assert!(close(rects[0].x1, x(4, Affinity::Upstream), 0.01));
    assert!(rects[1].x0 > rects[0].x1 + 15.0, "the Hebrew part is at the far side of its run: {rects:?}");
    assert!(close(rects[1].x1, x(4, Affinity::Downstream), 0.01), "it starts at the right edge of the Hebrew run: {rects:?}");
    // The whole Hebrew word is one block.
    assert_eq!(layout.selection_rects(4..12).len(), 1);
}

#[test]
fn moving_right_through_mixed_text_never_moves_the_caret_left() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one(MIXED), None);
    let mut caret = layout.caret_at(0);
    let mut xs = vec![layout.caret_x(caret).unwrap()];
    let mut seen = vec![caret.offset];
    for _ in 0..30 {
        let next = layout.move_caret(caret, Motion::Right, None);
        if next == caret {
            break;
        }
        caret = next;
        xs.push(layout.caret_x(caret).unwrap());
        seen.push(caret.offset);
    }
    assert!(xs.windows(2).all(|w| w[0] <= w[1] + 1e-6), "visual movement is monotonic: {xs:?} at offsets {seen:?}");
    assert_eq!(caret.offset, MIXED.len(), "ends at the end of the text: {seen:?}");
    assert!(xs.windows(2).all(|w| w[0] < w[1] - 1e-6), "every press makes progress: {xs:?}");
    // Every place a caret can be on screen is visited. (A boundary between left-to-right and right-to-left text has two
    // offsets sharing one place, and one offset with two places: all of them are covered.)
    for o in boundaries(&layout) {
        let places = [Affinity::Downstream, Affinity::Upstream].map(|affinity| layout.caret_x(Caret { offset: o, affinity }).unwrap());
        assert!(places.iter().any(|p| xs.iter().any(|x| close(*x, *p, 0.01))), "offset {o} skipped: {seen:?} {xs:?}");
    }
    // And back again with Left.
    let mut xs_back = vec![layout.caret_x(caret).unwrap()];
    for _ in 0..30 {
        let next = layout.move_caret(caret, Motion::Left, None);
        if next == caret {
            break;
        }
        caret = next;
        xs_back.push(layout.caret_x(caret).unwrap());
    }
    assert!(xs_back.windows(2).all(|w| w[0] >= w[1] - 1e-6), "{xs_back:?}");
    assert_eq!(caret.offset, 0);
}

#[test]
fn in_right_to_left_text_right_moves_towards_the_start() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one(HEBREW), None);
    // Start at the left end of the line (the end of the text) and press Right.
    let mut caret = layout.caret_at(HEBREW.len());
    let mut offsets = vec![caret.offset];
    for _ in 0..20 {
        let next = layout.move_caret(caret, Motion::Right, None);
        if next == caret {
            break;
        }
        caret = next;
        offsets.push(caret.offset);
    }
    assert!(offsets.windows(2).all(|w| w[0] > w[1]), "offsets fall as the caret moves right: {offsets:?}");
    assert_eq!(offsets.len(), 10, "nine characters, ten positions: {offsets:?}");
    assert_eq!(caret.offset, 0);
    let one_left = layout.move_caret(layout.caret_at(0), Motion::Left, None);
    assert!(one_left.offset > 0, "Left from the start moves visually left, which is further into the text");
}

#[test]
fn word_motion_jumps_between_words_visually() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("alpha beta gamma"), None);
    let mut caret = layout.caret_at(0);
    let mut stops = vec![];
    for _ in 0..4 {
        caret = layout.move_caret(caret, Motion::WordRight, None);
        stops.push(caret.offset);
    }
    // Word motion stops at the end of each word, and stays at the end of the text.
    assert_eq!(stops, vec![5, 10, 16, 16]);
    let back = layout.move_caret(caret, Motion::WordLeft, None);
    assert_eq!(back.offset, 11, "back to the start of the last word");
    let back2 = layout.move_caret(back, Motion::WordLeft, None);
    assert_eq!(back2.offset, 6);
}

#[test]
fn vertical_motion_keeps_the_horizontal_position() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one(SENTENCE), Some(150.0));
    assert!(layout.lines.len() >= 3);
    let start = layout.caret_at(layout.lines[0].range.start + 4);
    let goal = layout.caret_x(start).unwrap();
    let down = layout.move_caret(start, Motion::Down, Some(goal));
    assert!(layout.lines[1].range.contains(&down.offset), "{down:?} not on line 1 {:?}", layout.lines[1].range);
    assert!(close(layout.caret_x(down).unwrap(), goal, 12.0), "stays near x={goal}");
    let down2 = layout.move_caret(down, Motion::Down, Some(goal));
    assert!(layout.lines[2].range.contains(&down2.offset));
    let up = layout.move_caret(down2, Motion::Up, Some(goal));
    assert!(layout.lines[1].range.contains(&up.offset));
    // At the ends of the text vertical motion goes to the start or the end.
    assert_eq!(layout.move_caret(layout.caret_at(2), Motion::Up, None).offset, 0);
    let last = layout.caret_at(SENTENCE.len() - 1);
    assert_eq!(layout.move_caret(last, Motion::Down, None).offset, SENTENCE.len());
}

#[test]
fn line_and_text_boundaries() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one(SENTENCE), Some(150.0));
    let inside = layout.caret_at(layout.lines[1].range.start + 3);
    let start = layout.move_caret(inside, Motion::LineStart, None);
    assert_eq!(start.offset, layout.lines[1].range.start);
    let end = layout.move_caret(inside, Motion::LineEnd, None);
    let line_text = SENTENCE.get(layout.lines[1].range.clone()).unwrap();
    assert_eq!(end.offset, layout.lines[1].range.start + line_text.trim_end().len(), "End stops before the wrapped space");
    assert_eq!(end.affinity, Affinity::Upstream, "and stays on that line");
    assert!(layout.lines[1].range.contains(&end.offset.saturating_sub(1)));
    let end_rect = layout.caret_rect(end, 1.0).unwrap();
    assert!(close(end_rect.center().y, layout.lines[1].rect.center().y, 1.0), "{end_rect:?}");
    assert_eq!(layout.move_caret(inside, Motion::TextStart, None).offset, 0);
    assert_eq!(layout.move_caret(inside, Motion::TextEnd, None).offset, SENTENCE.len());
    assert_eq!(layout.move_caret(inside, Motion::ParagraphEnd, None).offset, SENTENCE.len());
    assert_eq!(layout.move_caret(inside, Motion::ParagraphStart, None).offset, 0);
}

#[test]
fn the_caret_moves_across_paragraph_breaks() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&RichText::plain("one\ntwo\n\nfour"), Some(200.0));
    assert_eq!(layout.text(), "one\ntwo\n\nfour");
    let mut caret = layout.caret_at(2);
    let mut offsets = vec![];
    for _ in 0..12 {
        caret = layout.move_caret(caret, Motion::Right, None);
        offsets.push(caret.offset);
    }
    // 3 = end of "one", 4 = start of "two" (the newline is skipped as one step), ... the empty paragraph is a stop.
    assert_eq!(&offsets[..3], &[3, 4, 5], "{offsets:?}");
    assert!(offsets.contains(&8) && offsets.contains(&9), "{offsets:?}");
    assert_eq!(offsets.last().copied(), Some(13), "{offsets:?}");
    for _ in 0..20 {
        caret = layout.move_caret(caret, Motion::Left, None);
    }
    assert_eq!(caret.offset, 0);
    let from_two = layout.move_caret(layout.caret_at(4), Motion::Left, None);
    assert_eq!(from_two.offset, 3, "Left at the start of a paragraph is the end of the previous one");
    // Vertical motion crosses paragraphs too.
    let down = layout.move_caret(layout.caret_at(1), Motion::Down, None);
    assert!((4..=7).contains(&down.offset), "{down:?}");
}

#[test]
fn right_to_left_paragraph_breaks_continue_in_reading_order() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let text = RichText::new(vec![
        Paragraph { direction: Direction::Rtl, ..Paragraph::new(vec![run("שלום")]) },
        Paragraph { direction: Direction::Rtl, ..Paragraph::new(vec![run("עולם")]) },
    ]);
    let layout = engine.layout(&text, Some(200.0));
    assert_eq!(layout.text().len(), 17);
    // Reading order is right to left, so the caret walks leftwards through "שלום" (offset 0 at the right edge) to the
    // end of the paragraph: visually pressing Left moves forward in the text.
    let mut caret = layout.caret_at(0);
    let mut offsets = vec![0];
    for _ in 0..9 {
        caret = layout.move_caret(caret, Motion::Left, None);
        offsets.push(caret.offset);
    }
    assert_eq!(offsets, vec![0, 2, 4, 6, 8, 9, 11, 13, 15, 17], "forward through both paragraphs: {offsets:?}");
}

#[test]
fn extending_a_selection_keeps_the_anchor() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("Hello World"), None);
    let mut selection = Selection::collapsed(layout.caret_at(2));
    selection = layout.move_selection(selection, Motion::Right, true, None);
    selection = layout.move_selection(selection, Motion::Right, true, None);
    assert_eq!(selection.range(), 2..4);
    assert_eq!(selection.anchor.offset, 2);
    let collapsed = layout.move_selection(selection, Motion::Left, false, None);
    assert!(collapsed.is_collapsed());
    assert_eq!(collapsed.focus.offset, 2, "Left collapses to the left edge of the selection");
    let collapsed = layout.move_selection(selection, Motion::Right, false, None);
    assert_eq!(collapsed.focus.offset, 4);
    let word = layout.word_range_at(8);
    assert_eq!(word, 6..11, "double click in 'World'");
    assert_eq!(layout.word_range_at(2), 0..5);
}

#[test]
fn word_ranges_work_in_right_to_left_text() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one(HEBREW), None);
    let second = layout.word_range_at(12);
    assert_eq!(second, 9..17, "{second:?}");
    let first = layout.word_range_at(2);
    assert_eq!(first.start, 0);
    assert!(first.end >= 8, "{first:?}");
}

#[test]
fn carets_survive_line_spacing_and_alignment() {
    let Some(mut engine) = engine() else { return };
    let text = RichText::new(vec![Paragraph {
        alignment: Alignment::Center,
        line_spacing: 6.0,
        direction: Direction::Ltr,
        margin_left: 10.0,
        ..Paragraph::new(vec![run(SENTENCE)])
    }]);
    let layout = engine.layout(&text, Some(180.0));
    for (i, line) in layout.lines.iter().enumerate() {
        let caret = layout.caret_at(line.range.start);
        let rect = layout.caret_rect(caret, 1.0).unwrap();
        assert!(close(rect.center().y, line.rect.center().y, 1.0), "line {i}: caret {rect:?} line {:?}", line.rect);
        assert!(close(rect.x0, line.rect.x0, 0.6), "line {i}: caret {rect:?} line {:?}", line.rect);
        let hit = layout.hit_test(Point::new(line.rect.x0 + 0.2, line.rect.center().y));
        assert_eq!(hit, line.range.start, "line {i}");
    }
    let rects = layout.selection_rects(0..SENTENCE.len());
    for (rect, line) in rects.iter().zip(&layout.lines) {
        assert!(close(rect.center().y, line.rect.center().y, 1.0), "{rect:?} vs {:?}", line.rect);
    }
}

#[test]
fn offsets_inside_characters_and_beyond_the_text_are_clamped() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let layout = engine.layout(&one("aשb"), None);
    assert_eq!(layout.caret_at(2).offset, 1, "inside the two-byte letter snaps back");
    assert_eq!(layout.caret_at(99).offset, 4);
    assert!(layout.caret_rect(layout.caret_at(99), 1.0).is_some());
    assert!(layout.caret_rect(Caret::new(2), 1.0).is_some());
    let moved = layout.move_caret(Caret::new(99), Motion::Left, None);
    assert!(moved.offset <= 4);
    let para = para("x", Alignment::Left, Direction::Auto);
    let one_char = engine.layout(&para, None);
    assert_eq!(one_char.move_caret(Caret::new(0), Motion::Left, None).offset, 0);
    assert_eq!(one_char.move_caret(Caret::new(1), Motion::Right, None).offset, 1);
}
