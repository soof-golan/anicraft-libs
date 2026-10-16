//! Visual verification of the text engine on hard synthetic fixtures (`tests/fixtures/visual/*.json`). Each fixture has
//! a prediction written before it was rendered in the matching `.expect.md`; the assertions here check those predictions
//! (with the verdicts and the corrections recorded there).
//!
//! The fixtures name DejaVu Sans, so the tests skip when it is not installed. With `ANICRAFT_VISUAL_OUT` set every
//! fixture is also rendered to a PNG and dumped to a text file there, for inspection; the committed
//! `tests/fixtures/visual/*.png` are the goldens the renders are compared with.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::while_float,
    clippy::suboptimal_flops,
    clippy::needless_collect,
    clippy::explicit_iter_loop,
    clippy::redundant_closure_for_method_calls,
    clippy::doc_markdown,
    reason = "assertions on known fixtures: a failure is a failed test, and the picture maths is on small numbers"
)]

use crate::common;

use std::fmt::Write as _;
use std::ops::Range;

use anim_text::{Affinity, Alignment, Caret, Direction, GlyphRun, Motion, Paragraph, RichText, Run, TextEngine, TextLayout};
use common::close;
use common::render::{Canvas, Overlay, fixture, out_dir, render, render_to_file};

const DEJAVU: &str = "DejaVu Sans";

const FIXTURES: &[(&str, Option<f64>)] = &[
    ("bidi-he-numbers", None),
    ("bidi-parens-mirror", None),
    ("arabic-joining", None),
    ("rtl-wrap", Some(330.0)),
    ("rtl-justify", Some(340.0)),
    ("niqqud-carets", None),
    ("emoji-fallback", None),
    ("selection-mixed", None),
    ("empty-lines-spaces", Some(400.0)),
    ("narrow-width", Some(1.0)),
    ("rtl-base-latin", Some(600.0)),
    ("rtl-number-alignment", Some(400.0)),
];

fn engine() -> Option<TextEngine> {
    let mut engine = TextEngine::new();
    if engine.has_family(DEJAVU) {
        Some(engine)
    } else {
        eprintln!("skipping: {DEJAVU} is not installed");
        None
    }
}

fn width_of(name: &str) -> Option<f64> {
    FIXTURES.iter().find(|(n, _)| *n == name).and_then(|(_, w)| *w)
}

/// A readable description of a layout: lines, glyph runs in screen order, caret x at every boundary.
fn dump(layout: &TextLayout) -> String {
    let mut out = String::new();
    let text = layout.text();
    for (i, l) in layout.lines.iter().enumerate() {
        let _ = writeln!(
            out,
            "line {i}: range {:?} rect x {:.2}..{:.2} y {:.2}..{:.2} baseline {:.2} rtl {}",
            l.range, l.rect.x0, l.rect.x1, l.rect.y0, l.rect.y1, l.baseline, l.rtl
        );
    }
    let mut runs: Vec<_> = layout.runs.iter().collect();
    runs.sort_by(|a, b| a.line.cmp(&b.line).then(a.origin.x.total_cmp(&b.origin.x)));
    for r in runs {
        let ids: Vec<_> = r.glyphs.iter().map(|g| g.id).collect();
        let xs: Vec<_> = r.glyphs.iter().map(|g| format!("{:.1}", g.x)).collect();
        let _ = writeln!(
            out,
            "run line {} {:?} {:?} rtl {} font {:x} x {:.2}..{:.2} adv {:.2} ids {ids:?} xs {xs:?}",
            r.line,
            r.range,
            text.get(r.range.clone()).unwrap_or("?"),
            r.rtl,
            r.font.id & 0xffff,
            r.origin.x,
            r.origin.x + r.advance,
            r.advance
        );
    }
    for offset in (0..=text.len()).filter(|&o| text.is_char_boundary(o)) {
        let x = |affinity| layout.caret_rect(Caret { offset, affinity }, 0.0).map(|r| format!("{:.2}@{:.1}", r.x0, r.y0));
        let _ = writeln!(out, "caret {offset}: down {:?} up {:?}", x(Affinity::Downstream), x(Affinity::Upstream));
    }
    out
}

/// Compares `canvas` with the committed golden `tests/fixtures/visual/<name>.png` (a few pixels may differ by a little).
fn assert_golden(name: &str, canvas: &Canvas) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/visual").join(format!("{name}.png"));
    let Ok(file) = std::fs::File::open(&path) else {
        eprintln!("no golden for {name}");
        return;
    };
    drop(file);
    let golden = anim_testkit::read_png(&path).expect("golden pixels");
    assert_eq!((golden.width, golden.height), (canvas.width, canvas.height), "{name}: golden size");
    let bytes = &golden.rgba;
    let differing = bytes.chunks(4).zip(canvas.rgba.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24)).count();
    let pixels = (canvas.width as usize) * (canvas.height as usize);
    assert!(differing * 500 <= pixels, "{name}: {differing} of {pixels} pixels differ from the golden");
}

#[test]
fn render_dump_and_compare_every_fixture() {
    let Some(mut engine) = engine() else { return };
    let dir = out_dir();
    for &(name, width) in FIXTURES {
        let layout = engine.layout(&fixture(name), width);
        let canvas = render_to_file(&mut engine, &layout, &Overlay { selections: Vec::new(), all_carets: true }, name);
        if let Some(dir) = &dir {
            std::fs::write(dir.join(format!("{name}.txt")), dump(&layout)).expect("write dump");
        }
        assert_golden(name, &canvas);
    }
}

// ---- helpers ---------------------------------------------------------------------------------------------------

fn text_of<'a>(layout: &'a TextLayout, run: &GlyphRun) -> &'a str {
    layout.text().get(run.range.clone()).unwrap_or("")
}

fn run_with_text<'a>(layout: &'a TextLayout, text: &str) -> &'a GlyphRun {
    layout.runs.iter().find(|r| text_of(layout, r) == text).unwrap_or_else(|| panic!("no glyph run with exactly {text:?}"))
}

fn caret_x(layout: &TextLayout, offset: usize, affinity: Affinity) -> f64 {
    layout.caret_rect(Caret { offset, affinity }, 0.0).expect("caret").x0
}

fn caret_y(layout: &TextLayout, offset: usize, affinity: Affinity) -> f64 {
    layout.caret_rect(Caret { offset, affinity }, 0.0).expect("caret").y0
}

/// (x, id, advance) of every glyph of the runs inside `range`, left to right on screen.
fn glyphs_in(layout: &TextLayout, range: &Range<usize>) -> Vec<(f64, u32, f64)> {
    let mut out: Vec<_> = layout
        .runs
        .iter()
        .filter(|r| r.range.start >= range.start && r.range.end <= range.end)
        .flat_map(|r| r.glyphs.iter().map(|g| (g.x, g.id, g.advance)))
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Glyph ids (left to right) of the glyphs that advance, i.e. without zero-width joiners.
fn ids_in(layout: &TextLayout, range: &Range<usize>) -> Vec<u32> {
    glyphs_in(layout, range).into_iter().filter(|g| g.2 > 0.01).map(|g| g.1).collect()
}

fn para(text: &str, direction: Direction, size: f64) -> RichText {
    RichText::new(vec![Paragraph { direction, runs: vec![Run { family: DEJAVU.into(), size, ..Run::new(text) }], ..Paragraph::default() }])
}

/// The width of `text` laid out alone, unwrapped.
fn natural_width(engine: &mut TextEngine, text: &str, size: f64) -> f64 {
    engine.layout(&para(text, Direction::Ltr, size), None).runs.iter().map(|r| r.advance).sum()
}

/// The glyph id of a single character in the font (laid out left to right).
fn glyph_id(engine: &mut TextEngine, text: &str, size: f64) -> u32 {
    engine.layout(&para(text, Direction::Ltr, size), None).runs.first().and_then(|r| r.glyphs.first()).map(|g| g.id).expect("a glyph")
}

/// The offsets the caret visits pressing `motion` repeatedly from the start of `para`, up to the paragraph's end (the
/// next press would leave for the next paragraph).
fn stops(layout: &TextLayout, para: &Range<usize>, motion: Motion) -> Vec<usize> {
    let mut caret = layout.caret_at(para.start);
    let mut out = Vec::new();
    for _ in 0..200 {
        let next = layout.move_caret(caret, motion, None);
        if next == caret || !para.contains(&next.offset) && next.offset != para.end {
            break;
        }
        out.push(next.offset);
        if next.offset == para.end {
            break;
        }
        caret = next;
    }
    out
}

fn strictly_increasing(xs: impl Iterator<Item = f64>) -> bool {
    let v: Vec<f64> = xs.collect();
    v.windows(2).all(|w| w[1] > w[0])
}

// ---- bidi-he-numbers --------------------------------------------------------------------------------------------

#[test]
fn hebrew_sentence_keeps_numbers_whole_and_in_visual_order() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("bidi-he-numbers"), width_of("bidi-he-numbers"));
    let text = layout.text().to_owned();
    assert_eq!(layout.lines.len(), 1);
    let line = &layout.lines[0];
    assert!(line.rtl);
    // P1, P7: the text starts at the right edge and ends at the left.
    assert!(close(caret_x(&layout, 0, Affinity::Downstream), line.rect.x1, 0.5));
    assert!(close(caret_x(&layout, text.len(), Affinity::Downstream), line.rect.x0, 0.5));
    // P2, P3: each number is one left-to-right run with its characters in increasing x.
    for number in ["054-1234567", "3.14", "2024"] {
        let run = run_with_text(&layout, number);
        assert!(!run.rtl, "{number}");
        assert!(strictly_increasing(run.glyphs.iter().map(|g| g.x)), "{number}");
    }
    // P4: the hyphen of `ב-2024` is not part of the number: it sits in the right-to-left run before it.
    let dash = text.find("ב-").unwrap() + "ב".len();
    let owner = layout.runs.iter().find(|r| r.range.contains(&dash)).unwrap();
    assert!(owner.rtl && owner.range.end == dash + 1, "{:?}", owner.range);
    // P5: reverse logical order on screen.
    let (a, b, c) = (run_with_text(&layout, "2024"), run_with_text(&layout, "3.14"), run_with_text(&layout, "054-1234567"));
    assert!(a.origin.x + a.advance <= b.origin.x + 0.01 && b.origin.x + b.advance <= c.origin.x + 0.01);
    // P6: one offset, two places, at the boundary between the Hebrew space and the number.
    let at = text.find("054").unwrap();
    assert!(close(caret_x(&layout, at, Affinity::Downstream), c.origin.x, 0.05));
    assert!(close(caret_x(&layout, at, Affinity::Upstream), c.origin.x + c.advance, 0.05));
    // P8: no extra spacing, and the text box holds all of the ink (nothing at negative x).
    assert!(close(c.advance, natural_width(&mut engine, "054-1234567", 28.0), 0.05));
    for r in &layout.runs {
        assert!(r.origin.x >= layout.bounds.x0 - 0.01 && r.origin.x + r.advance <= layout.bounds.x1 + 0.01, "{:?} outside {:?}", r.range, layout.bounds);
    }
}

// ---- rtl-number-alignment ---------------------------------------------------------------------------------------

#[test]
fn rtl_line_with_a_number_aligns_every_way() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("rtl-number-alignment"), Some(400.0));
    let ink = |i: usize| layout.lines[i].rect;
    let start = ink(0);
    let width = start.width();
    assert!(close(start.x1, 400.0, 0.3), "Start: {start:?}");
    assert!(close(ink(1).x0, 0.0, 0.3), "End: {:?}", ink(1));
    assert!(close(ink(2).x0, 0.0, 0.3), "Left: {:?}", ink(2));
    assert!(close(ink(3).x1, 400.0, 0.3), "Right: {:?}", ink(3));
    assert!(close(ink(4).center().x, 200.0, 0.3), "Center: {:?}", ink(4));
    assert!(close(ink(5).x1, 400.0, 0.3) && close(ink(5).width(), width, 0.3), "Justify on the last line: {:?}", ink(5));
    assert!(close(ink(6).x1, 400.0, 0.3) && close(ink(6).width(), width, 0.3), "Start with a trailing space: {:?}", ink(6));
    // Unwrapped: the box holds all of the ink.
    let free = engine.layout(&fixture("rtl-number-alignment"), None);
    for r in &free.runs {
        assert!(r.origin.x >= free.bounds.x0 - 0.01 && r.origin.x + r.advance <= free.bounds.x1 + 0.01, "{:?} outside {:?}", r.range, free.bounds);
    }
}

// ---- bidi-parens-mirror -----------------------------------------------------------------------------------------

#[test]
fn brackets_are_mirrored_in_right_to_left_runs_only() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("bidi-parens-mirror"), None);
    let id = |engine: &mut TextEngine, s: &str| glyph_id(engine, s, 28.0);
    let (open, close_, open_brace) = (id(&mut engine, "("), id(&mut engine, ")"), id(&mut engine, "{"));
    let ranges: Vec<_> = layout.paragraphs.iter().map(|p| p.range.clone()).collect();
    // P1
    let p1 = glyphs_in(&layout, &ranges[0]);
    assert_eq!(p1.last().unwrap().1, close_, "the logical `(` at offset 0 is drawn as `)`, rightmost");
    assert_eq!(p1.first().unwrap().1, open_brace, "the logical `}}` is drawn as `{{`, leftmost");
    // P2: the picture reads `(hello)`.
    let hello = run_with_text(&layout, "hello");
    let p2 = glyphs_in(&layout, &ranges[1]);
    let left = p2.iter().rev().find(|g| g.0 < hello.origin.x - 0.01).unwrap();
    let right = p2.iter().find(|g| g.0 >= hello.origin.x + hello.advance - 0.01).unwrap();
    assert_eq!((left.1, right.1), (open, close_));
    // P3: no mirroring in a left-to-right paragraph.
    let a = glyph_id(&mut engine, "a", 28.0);
    let p3 = ids_in(&layout, &ranges[2]);
    assert_eq!((p3[0], p3[4]), (open, close_));
    assert_eq!(p3[1], a);
    // P4: all four parentheses are mirrored.
    let alef = glyph_id(&mut engine, "א", 28.0);
    assert_eq!(ids_in(&layout, &ranges[3]), vec![open, open, alef, close_, close_]);
}

// ---- arabic-joining ---------------------------------------------------------------------------------------------

#[test]
fn arabic_letters_join_and_ligate() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("arabic-joining"), None);
    let r: Vec<_> = layout.paragraphs.iter().map(|p| p.range.clone()).collect();
    let ids: Vec<Vec<u32>> = r.iter().map(|range| ids_in(&layout, range)).collect();
    // P1: final, medial, medial, initial.
    assert_eq!(ids[0].len(), 4);
    let (c, b, a) = (ids[0][0], ids[0][1], ids[0][3]);
    assert!(ids[0][1] == ids[0][2] && a != b && b != c && a != c, "{:?}", ids[0]);
    // P2: the isolated form.
    assert!(ids[1].len() == 1 && ![a, b, c].contains(&ids[1][0]));
    let isolated = ids[1][0];
    // P3: lam-alef is one glyph over two characters, with a caret stop in the middle of it.
    assert_eq!(ids[2].len(), 1);
    let lig = ids[2][0];
    let start = r[2].start;
    let (x0, xm, x1) =
        (caret_x(&layout, start, Affinity::Downstream), caret_x(&layout, start + 2, Affinity::Downstream), caret_x(&layout, start + 4, Affinity::Downstream));
    assert!(close(xm, f64::midpoint(x0, x1), 1.5) && x0 > xm && xm > x1, "{x0} {xm} {x1}");
    // P4: a ZWNJ in between prevents the ligature.
    assert!(ids[3].len() >= 2 && !ids[3].contains(&lig), "{:?}", ids[3]);
    // P5: a ZWJ forces the joined forms, like plain `بب`; P6: a ZWNJ gives two isolated forms.
    let pair = ids_in(&engine.layout(&para("بب", Direction::Auto, 40.0), None), &(0..4));
    assert_eq!(ids[4], pair);
    assert_eq!(ids[4], vec![c, a]);
    assert_eq!(ids[5], vec![isolated, isolated]);
    // Carets never stop inside the invisible joiner: the cluster `ب` + ZWJ is one step.
    let visited = stops(&layout, &r[4], Motion::Left);
    assert_eq!(visited, vec![r[4].start + 5, r[4].end]);
    // P7: letter spacing does not touch cursive text.
    assert_eq!(ids[6], ids[0]);
    let widths = |range: &Range<usize>| glyphs_in(&layout, range).iter().map(|g| g.2).sum::<f64>();
    assert!(close(widths(&r[6]), widths(&r[0]), 0.01));
    // P8: Arabic-Indic digits are a left-to-right run to the left of the word.
    let digits = run_with_text(&layout, "٢٠٢٤");
    assert!(!digits.rtl);
    let d: Vec<u32> = digits.glyphs.iter().map(|g| g.id).collect();
    assert!(d[0] == d[2] && d[1] != d[0] && d[3] != d[0] && d[3] != d[1], "{d:?}");
    let word = run_with_text(&layout, "عام ");
    assert!(word.origin.x >= digits.origin.x + digits.advance - 0.01);
    // P9: beh keeps its initial form before a lam-alef ligature, which takes its final form.
    assert_eq!(ids[8][ids[8].len() - 1], a);
    assert_eq!(&ids[8][..3], &[c, b, a]);
    assert!(![lig, isolated].contains(&ids[8][ids[8].len() - 2]));
}

// ---- rtl-wrap and rtl-justify -----------------------------------------------------------------------------------

#[test]
fn wrapped_hebrew_stays_flush_right_and_never_splits_numbers() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("rtl-wrap"), Some(330.0));
    let text = layout.text().to_owned();
    assert!(layout.lines.len() >= 4);
    // P1: contiguous ranges, top to bottom.
    assert_eq!(layout.lines[0].range.start, 0);
    assert_eq!(layout.lines.last().unwrap().range.end, text.len());
    for w in layout.lines.windows(2) {
        assert_eq!(w[0].range.end, w[1].range.start);
        assert!(w[1].rect.y0 > w[0].rect.y0);
    }
    for (i, line) in layout.lines.iter().enumerate() {
        assert!(line.rtl);
        // P2: flush right; P3: the ink stays inside the box.
        let right = layout.runs.iter().filter(|r| r.line == i).map(|r| r.origin.x + r.advance).fold(f64::MIN, f64::max);
        assert!(close(right, 330.0, 0.6), "line {i} ends at {right}");
        assert!(line.rect.x0 >= -0.01 && line.rect.x1 <= 330.6, "line {i}: {:?}", line.rect);
        // P6: nothing that cannot start a line starts one, and no opening bracket ends one.
        let s = text.get(line.range.clone()).unwrap();
        assert!(!s.starts_with([')', '.', ',']), "line {i} starts with {:?}", s.chars().next());
        assert!(!s.trim_end().ends_with('('), "line {i} ends with an opening bracket");
    }
    // P4: numbers stay on one line.
    for needle in ["054-1234567", "3.14", "2024", "(Daniel"] {
        let at = text.find(needle).unwrap();
        let end = at + needle.len();
        assert!(layout.lines.iter().any(|l| l.range.start <= at && end <= l.range.end), "{needle} was split");
    }
    // P7: the same offset at a soft wrap has two places.
    let wrap = layout.lines[0].range.end;
    assert!(caret_y(&layout, wrap, Affinity::Upstream) < caret_y(&layout, wrap, Affinity::Downstream));
    assert!(close(caret_x(&layout, wrap, Affinity::Downstream), 330.0, 0.6));
    assert!(caret_x(&layout, wrap, Affinity::Upstream) <= layout.lines[0].rect.x0 + 0.01);
}

#[test]
fn justified_rtl_lines_fill_the_box_and_only_stretch_spaces() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("rtl-justify"), Some(340.0));
    let n = layout.lines.len();
    assert!(n >= 4);
    for (i, line) in layout.lines.iter().enumerate() {
        if i + 1 < n {
            // P1
            assert!(close(line.rect.x0, 0.0, 0.8) && close(line.rect.x1, 340.0, 0.8), "line {i}: {:?}", line.rect);
            // P4: the extra is shared evenly by the word gaps (the hanging trailing space is not one of them).
            let text = layout.text();
            let hangs = text.get(line.range.clone()).unwrap().ends_with(' ');
            let mut gaps: Vec<(f64, f64)> =
                layout.runs.iter().filter(|r| r.line == i).flat_map(|r| r.glyphs.iter().filter(|g| g.id == 3).map(|g| (g.x, g.advance))).collect();
            gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
            if hangs {
                gaps.remove(0);
            }
            let first = gaps[0].1;
            assert!(gaps.iter().all(|g| close(g.1, first, 0.1)), "line {i}: {gaps:?}");
            assert!(first >= 7.0, "line {i}: gap {first}");
            // P6: a selection of the whole line tiles it.
            let end = line.range.end - usize::from(hangs);
            let mut rects = layout.selection_rects(line.range.start..end);
            rects.sort_by(|a, b| a.x0.total_cmp(&b.x0));
            assert!(close(rects[0].x0, 0.0, 0.8) && close(rects.last().unwrap().x1, 340.0, 0.8), "line {i}: {rects:?}");
            assert!(rects.windows(2).all(|w| w[1].x0 - w[0].x1 > -0.05 && w[1].x0 - w[0].x1 < 0.5), "line {i}: {rects:?}");
        } else {
            // P2
            // The last line is flush right and its word gaps keep the natural space width (it is not stretched).
            assert!(close(line.rect.x1, 340.0, 0.6), "last line: {:?}", line.rect);
            let space = natural_width(&mut engine, " ", 24.0);
            for run in layout.runs.iter().filter(|r| r.line == i) {
                for g in run.glyphs.iter().filter(|g| g.id == 3) {
                    assert!(close(g.advance, space, 0.05), "last line gap {} vs {space}", g.advance);
                }
            }
        }
    }
    // P3: numbers keep their natural width.
    for number in ["054-1234567", "2024", "3.14"] {
        let run = run_with_text(&layout, number);
        assert!(close(run.advance, natural_width(&mut engine, number, 24.0), 0.05), "{number}: {}", run.advance);
    }
    // P5 (corrected in the .expect.md): the first character of every line is at the right edge, except that a line
    // starting with a left-to-right number has its first character at the LEFT end of that number, which is the
    // rightmost run.
    for (i, line) in layout.lines.iter().enumerate() {
        let first = layout.runs.iter().find(|r| r.line == i && r.range.start == line.range.start).unwrap();
        let expected = if first.rtl { 340.0 } else { first.origin.x };
        assert!(close(caret_x(&layout, line.range.start, Affinity::Downstream), expected, 0.8), "line {i}");
        assert!(close(first.origin.x + first.advance, 340.0, 0.8), "line {i}: the first run is the rightmost");
    }
}

// ---- niqqud-carets ----------------------------------------------------------------------------------------------

#[test]
fn marks_stay_with_their_letters_for_carets_and_hit_tests() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("niqqud-carets"), None);
    let he = layout.paragraphs[0].range.clone();
    let la = layout.paragraphs[1].range.clone();
    let he_text = layout.text().get(he.clone()).unwrap().to_owned();
    // The cluster boundaries of paragraph 0: letters, each with the marks that follow it, and the space.
    let mut bounds = vec![he.start];
    for (i, ch) in he_text.char_indices() {
        let combining = ('\u{591}'..='\u{5C7}').contains(&ch);
        if !combining && i > 0 {
            bounds.push(he.start + i);
        }
    }
    bounds.push(he.end);
    assert_eq!(bounds.len(), 10, "nine clusters: {bounds:?}");
    // P1: nine steps to the left, only at cluster boundaries, moving left each time.
    let visited = stops(&layout, &he, Motion::Left);
    assert_eq!(visited.len(), 9, "{visited:?}");
    assert!(visited.iter().all(|o| bounds.contains(o)), "{visited:?} vs {bounds:?}");
    let xs: Vec<f64> = std::iter::once(he.start).chain(visited.iter().copied()).map(|o| caret_x(&layout, o, Affinity::Downstream)).collect();
    assert!(xs.windows(2).all(|w| w[0] - w[1] > 4.0), "{xs:?}");
    // P2: the Latin paragraph: five clusters (é, space, ạ̈, space, x).
    let la_text = layout.text().get(la.clone()).unwrap();
    assert_eq!(la_text.chars().count(), 8, "e + acute, space, a + two marks, space, x");
    let visited = stops(&layout, &la, Motion::Right);
    assert_eq!(visited.len(), 5, "{visited:?}");
    let xs: Vec<f64> = std::iter::once(la.start).chain(visited.iter().copied()).map(|o| caret_x(&layout, o, Affinity::Downstream)).collect();
    assert!(strictly_increasing(xs.into_iter()));
    // P3: a fine sweep of hit tests never lands inside a cluster.
    let mut allowed = bounds.clone();
    allowed.extend(stops(&layout, &la, Motion::Right));
    allowed.push(la.start);
    for (para, y) in [(&he, layout.lines[0].rect.center().y), (&la, layout.lines[1].rect.center().y)] {
        let mut x = -5.0;
        while x < 215.0 {
            let offset = layout.hit_test(anim_core::Point::new(x, y));
            assert!(allowed.contains(&offset) || !para.contains(&offset), "x {x} hit {offset} inside a cluster");
            x += 0.5;
        }
    }
    // P4, P5: the marks add no width; selecting the first word gives one rectangle of the width of its four letters.
    let word = he.start..he.start + "שָׁלוֹם".len();
    let rects = layout.selection_rects(word);
    assert_eq!(rects.len(), 1, "{rects:?}");
    assert!(close(rects[0].width(), natural_width(&mut engine, "שלום", 44.0), 0.5), "{} vs {}", rects[0].width(), natural_width(&mut engine, "שלום", 44.0));
}

// ---- selection-mixed --------------------------------------------------------------------------------------------

#[test]
fn selections_across_directions_split_into_the_right_rectangles() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("selection-mixed"), None);
    let hebrew = run_with_text(&layout, "שלום");
    let h = (hebrew.origin.x, hebrew.origin.x + hebrew.advance);
    // Visual glyph order of the word is mem, vav, lamed, shin.
    let adv: Vec<f64> = hebrew.glyphs.iter().map(|g| g.advance).collect();
    let def = run_with_text(&layout, "def");
    let a = run_with_text(&layout, "abc ");
    let _ = a;

    // P1..P4: `abc של` .. `de`: bytes 8..15.
    let rects = layout.selection_rects(8..15);
    assert_eq!(rects.len(), 2, "{rects:?}");
    assert!(close(rects[0].y0, rects[1].y0, 0.01) && close(rects[0].y1, rects[1].y1, 0.01));
    let (left, right) = if rects[0].x0 < rects[1].x0 { (rects[0], rects[1]) } else { (rects[1], rects[0]) };
    assert!(close(left.x0, h.0, 0.05) && close(left.x1, h.0 + adv[0] + adv[1], 0.05), "{left:?}");
    assert!(close(right.x0, h.1, 0.05) && close(right.x1, def.glyphs[2].x, 0.05), "{right:?}");
    assert!(close(right.x0 - left.x1, adv[2] + adv[3], 0.05));

    // P5: `c ` and the shin, the rightmost glyph of the word.
    let rects = layout.selection_rects(2..6);
    assert_eq!(rects.len(), 2, "{rects:?}");
    let (left, right) = if rects[0].x0 < rects[1].x0 { (rects[0], rects[1]) } else { (rects[1], rects[0]) };
    let abc = run_with_text(&layout, "abc ");
    assert!(close(left.x0, abc.glyphs[2].x, 0.05) && close(left.x1, h.0, 0.05), "{left:?}");
    assert!(close(right.x1, h.1, 0.05) && close(right.width(), adv[3], 0.05), "{right:?}");

    // Paragraph 1 is right to left: `שלום abc עולם`.
    let p1 = layout.paragraphs[1].range.clone();
    let line = layout.lines[1].rect;
    // P6: everything tiles the line.
    let mut rects = layout.selection_rects(p1.clone());
    rects.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    assert!(close(rects[0].x0, line.x0, 0.05) && close(rects.last().unwrap().x1, line.x1, 0.05), "{rects:?} vs {line:?}");
    assert!(rects.windows(2).all(|w| w[1].x0 - w[0].x1 > -0.05 && w[1].x0 - w[0].x1 < 0.5), "{rects:?}");
    // P7: `abc` alone.
    let text = layout.text().to_owned();
    let at = text[p1.start..].find("abc").unwrap() + p1.start;
    let rects = layout.selection_rects(at..at + 3);
    let abc1 = run_with_text(&layout, "abc");
    assert_eq!(rects.len(), 1);
    assert!(close(rects[0].x0, abc1.origin.x, 0.05) && close(rects[0].width(), abc1.advance, 0.05));
    // P8: from after the first shin to after the second word's ayin: contiguous on screen, from the left edge of the ayin
    // to the left edge of the shin.
    let w1 = layout.runs.iter().find(|r| r.rtl && r.range.start == p1.start || text_of(&layout, r) == "שלום ").unwrap();
    let w2 = run_with_text(&layout, "עולם");
    let shin = w1.glyphs.iter().map(|g| g.advance).fold(0.0, f64::max);
    let _ = shin;
    let word1_right = w1.origin.x + w1.advance;
    let shin_adv = w1.glyphs.iter().max_by(|a, b| a.x.total_cmp(&b.x)).unwrap().advance;
    let ayin_adv = w2.glyphs.iter().max_by(|a, b| a.x.total_cmp(&b.x)).unwrap().advance;
    let start = p1.start + "ש".len();
    let end = text.find("עולם").unwrap() + "ע".len();
    let mut rects = layout.selection_rects(start..end);
    rects.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    assert!(close(rects[0].x0, w2.origin.x + w2.advance - ayin_adv, 0.05), "{rects:?}");
    assert!(close(rects.last().unwrap().x1, word1_right - shin_adv, 0.05), "{rects:?}");
    let total: f64 = rects.iter().map(|r| r.width()).sum();
    assert!(close(total, rects.last().unwrap().x1 - rects[0].x0, 0.5), "{rects:?}");
}

// ---- rtl-base-latin ---------------------------------------------------------------------------------------------

#[test]
fn latin_in_a_right_to_left_paragraph_and_hebrew_in_a_left_to_right_one() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("rtl-base-latin"), Some(600.0));
    let text = layout.text().to_owned();
    // P1
    let hello = run_with_text(&layout, "Hello World");
    let bang = run_with_text(&layout, "!");
    assert!(!hello.rtl && bang.rtl);
    assert!(close(hello.origin.x + hello.advance, 600.0, 0.6));
    assert!(bang.origin.x + bang.advance <= hello.origin.x + 0.5);
    // P2
    let (hx0, hx1) = (hello.origin.x, hello.origin.x + hello.advance);
    assert!(close(caret_x(&layout, 11, Affinity::Downstream), hx0, 0.05));
    assert!(close(caret_x(&layout, 11, Affinity::Upstream), hx1, 0.05));
    assert!(close(caret_x(&layout, 12, Affinity::Downstream), bang.origin.x, 0.05));
    assert!(close(caret_x(&layout, 0, Affinity::Downstream), hx0, 0.05));
    // P3: `1)`: the digit is a number, the bracket after it is neutral, takes the base direction and mirrors. `(2)` stays
    // in the left-to-right run (W7: the digit after a Latin letter is left to right), so it is not mirrored.
    let one = text.find("1)").unwrap();
    let open = glyph_id(&mut engine, "(", 32.0);
    let close_ = glyph_id(&mut engine, ")", 32.0);
    let all = glyphs_in(&layout, &layout.paragraphs[1].range);
    let digit_run = layout.runs.iter().find(|r| r.range.contains(&one)).unwrap();
    let paren = all.iter().rfind(|g| g.0 < digit_run.origin.x).unwrap();
    assert_eq!(paren.1, open, "the `)` after the 1 is mirrored and drawn left of it");
    assert!(!digit_run.rtl && close(digit_run.origin.x + digit_run.advance, 600.0, 0.6));
    assert_eq!(all.last().unwrap().1, glyph_id(&mut engine, "1", 32.0), "the 1 is the rightmost glyph");
    let tail = run_with_text(&layout, "First - item (2)");
    assert!(!tail.rtl);
    assert_eq!(tail.glyphs.last().unwrap().id, close_, "(2) is not mirrored");
    assert_eq!(tail.glyphs[tail.glyphs.len() - 3].id, open);
    // P4
    let heb = run_with_text(&layout, "שלום");
    let bang = layout.runs.iter().find(|r| text_of(&layout, r) == "!" && r.range.start > 30).unwrap();
    assert!(!bang.rtl && bang.origin.x >= heb.origin.x + heb.advance - 0.01);
    assert!(close(caret_x(&layout, 33, Affinity::Downstream), heb.origin.x + heb.advance, 0.05));
    assert!(close(caret_x(&layout, text.len(), Affinity::Downstream), bang.origin.x + bang.advance, 0.05));
}

// ---- emoji-fallback ---------------------------------------------------------------------------------------------

#[test]
fn emoji_use_a_fallback_font_and_move_as_whole_clusters() {
    let Some(mut engine) = engine() else { return };
    if !engine.has_family("Liberation Sans") {
        return;
    }
    let layout = engine.layout(&fixture("emoji-fallback"), None);
    let text = layout.text().to_owned();
    // P1
    let hi = run_with_text(&layout, "Hi ");
    let smile = run_with_text(&layout, "😀");
    assert_ne!(hi.font.id, smile.font.id);
    assert!(layout.runs.iter().all(|r| r.glyphs.iter().all(|g| g.id != 0)), "a glyph is .notdef");
    // P2: the thumbs-up with skin tone, the flag and the family are one step each.
    let thumbs = text.find('👍').unwrap();
    let flag = text.find('🇮').unwrap();
    let family = text.find('👩').unwrap();
    for (start, len) in [(thumbs, "👍🏽".len()), (flag, "🇮🇱".len()), (family, "👩\u{200d}👩\u{200d}👧".len())] {
        assert_eq!(layout.move_caret(layout.caret_at(start), Motion::Right, None).offset, start + len);
        assert_eq!(layout.move_caret(layout.caret_at(start + len), Motion::Left, None).offset, start);
    }
    // P4: the check mark comes from some installed font.
    assert!(run_with_text(&layout, "✓").glyphs[0].id != 0);
    // P5, P6: the 60 px run sets the height of its line and the baseline is shared.
    let p2 = layout.paragraphs[1].lines.start;
    assert!(layout.lines[p2].rect.height() >= 60.0);
    let on_line: Vec<_> = layout.runs.iter().filter(|r| r.line == p2).collect();
    assert_eq!(on_line.len(), 3);
    assert!(on_line.iter().all(|r| close(r.origin.y, on_line[0].origin.y, 0.001)));
    let b_at = text.rfind('b').unwrap();
    let e_at = text[layout.paragraphs[1].range.start..].find('😀').unwrap() + layout.paragraphs[1].range.start;
    assert!(close(caret_x(&layout, b_at, Affinity::Downstream) - caret_x(&layout, e_at, Affinity::Downstream), smile_adv(&layout, e_at), 0.01));
}

fn smile_adv(layout: &TextLayout, at: usize) -> f64 {
    layout.runs.iter().find(|r| r.range.start == at).map(|r| r.advance).unwrap()
}

// ---- empty-lines-spaces -----------------------------------------------------------------------------------------

#[test]
fn empty_lines_and_trailing_spaces_keep_their_height_and_carets() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&fixture("empty-lines-spaces"), Some(400.0));
    // P1
    assert_eq!(layout.lines.len(), 8);
    let h30 = layout.lines[0].rect.height();
    // P2: an empty paragraph is as tall as its run says.
    assert!(close(layout.lines[1].rect.height(), 2.0 * h30, 0.01 * h30), "{} vs {h30}", layout.lines[1].rect.height());
    // P3
    assert!(close(caret_x(&layout, 7, Affinity::Downstream), 0.0, 0.01));
    assert!(close(caret_x(&layout, 8, Affinity::Downstream), 400.0, 0.01));
    let ys: Vec<f64> = [0usize, 7, 8, 9, 13, 15, 16, 18].iter().map(|&o| caret_y(&layout, o, Affinity::Downstream)).collect();
    assert!(strictly_increasing(ys.into_iter()));
    // P4 (as corrected in the .expect.md): trailing spaces that fit count for the alignment, so the line's last advance
    // edge is the box edge and the `c` ends three spaces before it. They are not part of the visible extent.
    let space = natural_width(&mut engine, " ", 30.0);
    assert!(close(caret_x(&layout, 6, Affinity::Downstream), 400.0, 0.01));
    assert!(close(layout.lines[0].rect.x1, 400.0 - 3.0 * space, 0.01), "{:?}", layout.lines[0].rect);
    // P5
    let d = caret_x(&layout, 12, Affinity::Downstream) - caret_x(&layout, 9, Affinity::Downstream);
    assert!(close(d, 3.0 * space, 0.01));
    // P6: forced breaks inside a paragraph give lines, the empty one in the middle included.
    assert_eq!(layout.lines[4].range.start, 13);
    assert_eq!(layout.lines[5].range, 15..16);
    assert!(close(layout.lines[5].rect.height(), h30, 0.01));
    assert!(close(caret_x(&layout, 15, Affinity::Downstream), 0.0, 0.01));
    assert!(caret_y(&layout, 15, Affinity::Downstream) > caret_y(&layout, 14, Affinity::Downstream));
    // P7: Down visits every line once.
    let mut caret = layout.caret_at(6);
    for expected in 1..8 {
        caret = layout.move_caret(caret, Motion::Down, None);
        assert_eq!(layout.line_of(caret), Some(expected), "after {expected} downs");
    }
    // P8: the breaks of the empty paragraphs show as narrow marks: left of the box start for LTR, at its end for RTL.
    let rects = layout.selection_rects(0..layout.text().len());
    let h60 = layout.lines[1].rect.height();
    let mark_ltr = rects.iter().find(|r| close(r.y0, layout.lines[1].rect.y0, 0.01)).expect("mark of the empty LTR paragraph");
    assert!(close(mark_ltr.x0, 0.0, 0.01) && close(mark_ltr.width(), 0.25 * h60, 0.5), "{mark_ltr:?}");
    let mark_rtl = rects.iter().find(|r| close(r.y0, layout.lines[2].rect.y0, 0.01)).expect("mark of the empty RTL paragraph");
    assert!(close(mark_rtl.x1, 400.0, 0.01) && mark_rtl.width() > 5.0, "{mark_rtl:?}");
}

// ---- narrow-width -----------------------------------------------------------------------------------------------

#[test]
fn a_box_narrower_than_a_letter_breaks_after_every_cluster() {
    let Some(mut engine) = engine() else { return };
    for width in [1.0, 0.001] {
        let layout = engine.layout(&fixture("narrow-width"), Some(width));
        let text = layout.text().to_owned();
        // P2: seven lines for each paragraph.
        assert_eq!(layout.paragraphs[0].lines.len(), 7, "width {width}");
        assert_eq!(layout.paragraphs[1].lines.len(), 7, "width {width}");
        // P1: no line starts or ends inside a cluster.
        let he = layout.paragraphs[0].range.clone();
        let shin = text.find('ש').unwrap();
        let shin_end = shin + "ש\u{5b8}\u{5c1}".len();
        let e = text.find('e').unwrap();
        let lam = text.find('ل').unwrap();
        let emoji = text.find('😀').unwrap();
        for l in &layout.lines {
            for inside in [shin + 1, shin + "ש".len() + 1, e + 1, lam + "ل".len(), emoji + 1, emoji + 2, emoji + 3] {
                let inside = inside.min(l.range.end);
                let _ = inside;
            }
            for cut in [l.range.start, l.range.end] {
                assert!(!(shin < cut && cut < shin_end), "line {:?} cuts the shin cluster", l.range);
                assert!(!(e < cut && cut < e + "e\u{301}".len()), "line {:?} cuts e + acute", l.range);
                assert!(!(lam < cut && cut < lam + "لا".len()), "line {:?} cuts lam-alef", l.range);
                assert!(!(emoji < cut && cut < emoji + "😀".len()), "line {:?} cuts the emoji", l.range);
            }
        }
        // P3: constant pitch, no empty line, nothing dropped.
        let pitch = layout.lines[1].rect.y0 - layout.lines[0].rect.y0;
        assert!(layout.lines.windows(2).all(|w| close(w[1].rect.y0 - w[0].rect.y0, pitch, 0.01)));
        assert!(layout.lines.iter().all(|l| !l.range.is_empty()));
        for p in &layout.paragraphs {
            let mut at = p.range.start;
            for l in layout.lines[p.lines.clone()].iter() {
                assert_eq!(l.range.start, at);
                at = l.range.end;
            }
            assert_eq!(at, p.range.end);
        }
        // P5: the right-to-left lines start at the right edge of the box and overflow to the left.
        for l in &layout.lines[layout.paragraphs[0].lines.clone()] {
            // The box is never narrower than one pixel.
            assert!(close(l.rect.x1, width.max(1.0), 0.5) && l.rect.x0 < 0.0, "{:?}", l.rect);
        }
        let _ = he;
    }
}

#[test]
fn unusable_widths_are_the_same_as_no_width() {
    let Some(mut engine) = engine() else { return };
    let free = engine.layout(&fixture("narrow-width"), None);
    for width in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        let layout = engine.layout(&fixture("narrow-width"), Some(width));
        assert_eq!(layout.lines.len(), free.lines.len(), "width {width}");
        assert!(layout.lines.iter().all(|l| l.rect.x0.is_finite() && l.rect.x1.is_finite() && l.baseline.is_finite()));
        assert!(layout.bounds.width().is_finite());
    }
}

/// Pictures for the eye only (written when an output folder is set): selection rectangles over the mixed-direction
/// fixture and zoomed crops of the detailed areas.
#[test]
fn write_selection_overlays_and_zoomed_crops() {
    let Some(mut engine) = engine() else { return };
    let Some(dir) = out_dir() else { return };
    let layout = engine.layout(&fixture("selection-mixed"), None);
    let text = layout.text().to_owned();
    let p1 = layout.paragraphs[1].range.clone();
    let mut selections = layout.selection_rects(8..15);
    selections.extend(layout.selection_rects(p1.start + "ש".len()..text.find("עולם").unwrap() + "ע".len()));
    render_to_file(&mut engine, &layout, &Overlay { selections, all_carets: false }, "selection-mixed-overlay");

    let niqqud = engine.layout(&fixture("niqqud-carets"), None);
    let canvas = render(&mut engine, &niqqud, &Overlay { selections: Vec::new(), all_carets: true });
    canvas.crop_zoom(340, 10, 450, 110, 8).save(&dir.join("niqqud-carets-shin-x8.png"));
    canvas.crop_zoom(0, 120, 250, 230, 4).save(&dir.join("niqqud-carets-latin-x4.png"));
    let arabic = engine.layout(&fixture("arabic-joining"), None);
    let canvas = render(&mut engine, &arabic, &Overlay { selections: Vec::new(), all_carets: true });
    canvas.crop_zoom(200, 190, 360, 380, 4).save(&dir.join("arabic-joining-lam-alef-x4.png"));
}

#[test]
fn alignment_enum_covers_every_variant_used_by_the_fixtures() {
    // Keeps the import used and documents which alignments the fixtures exercise.
    let t = fixture("rtl-number-alignment");
    let used: Vec<Alignment> = t.paragraphs.iter().map(|p| p.alignment).collect();
    assert_eq!(used, [Alignment::Start, Alignment::End, Alignment::Left, Alignment::Right, Alignment::Center, Alignment::Justify, Alignment::Start]);
}
