//! Text layout benchmarks on the bundled test fonts (the same on every machine): a document of mixed Hebrew, Arabic and
//! Latin paragraphs (the verification fixtures scaled up), the 1 px box that breaks between every character, one huge
//! paragraph, glyph outlines, and the editor queries (caret, selection, hit testing, movement).
//!
//! The Adobe sample files carry no text frames, so there is no sample-gated bench. With `BENCH_CHECKSUM=1` the benches
//! print a checksum of what they produce, to compare two runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stderr,
    reason = "benchmark code over trusted local fixtures"
)]

use std::hint::black_box;

use anim_core::Point;
use anim_text::{Alignment, Direction, Motion, Paragraph, RichText, Run, TextEngine, TextLayout};
use criterion::{Criterion, criterion_group, criterion_main};

const FONTS: [&[u8]; 2] = [include_bytes!("../tests/fonts/AnicraftTestSans-Regular.ttf"), include_bytes!("../tests/fonts/AnicraftTestSans-Bold.ttf")];

const HEBREW: &str = "הלכתי לבית הספר עם החבר שלי (Daniel Cohen) ואז התקשרתי למספר 054-1234567 כדי לדווח שאני בדרך, והגעתי ב-2024 בשעה 3.14 אחר הצהריים.";
const ARABIC: &str = "ذهبت إلى المدرسة مع صديقي (Daniel) ثم اتصلت بالرقم 054-1234567 لأخبرهم أنني في الطريق، لا إله إلا الله، في عام 2024.";
const LATIN: &str = "The quick brown fox (jumps) over the lazy dog, then calls 054-1234567 at 3.14 pm in 2024 -- emoji \u{1F600} and e\u{301}.";
const NIQQUD: &str = "שָׁלוֹם עוֹלָם, בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ.";

fn engine() -> TextEngine {
    let mut engine = TextEngine::without_system_fonts();
    for font in FONTS {
        engine.register_font(font.to_vec());
    }
    engine
}

fn paragraph(text: &str, alignment: Alignment, direction: Direction) -> Paragraph {
    // Two styles per paragraph, so runs are split in the middle of the text.
    let split = text.char_indices().nth(text.chars().count() / 2).map_or(text.len(), |(i, _)| i);
    let (a, b) = text.split_at(split);
    Paragraph { alignment, direction, runs: vec![Run { size: 20.0, ..Run::new(a) }, Run { size: 24.0, bold: true, ..Run::new(b) }], ..Paragraph::default() }
}

/// `count` paragraphs cycling through the fixtures' texts.
fn document(count: usize, alignment: Alignment) -> RichText {
    let texts = [HEBREW, ARABIC, LATIN, NIQQUD];
    let direction = [Direction::Auto, Direction::Rtl, Direction::Ltr];
    RichText::new((0..count).map(|i| paragraph(texts[i % texts.len()], alignment, direction[i % direction.len()])).collect())
}

fn checksum(layout: &TextLayout) -> u64 {
    let mut h = layout.lines.len() as u64;
    for run in &layout.runs {
        for g in &run.glyphs {
            h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(g.id) ^ g.x.to_bits() ^ g.y.to_bits().rotate_left(17);
        }
    }
    h ^ layout.bounds.x1.to_bits() ^ layout.bounds.y1.to_bits().rotate_left(7)
}

fn report(name: &str, layout: &TextLayout) {
    if std::env::var_os("BENCH_CHECKSUM").is_some() {
        eprintln!("checksum {name}: {:016x}", checksum(layout));
    }
}

fn layout_benches(c: &mut Criterion) {
    let mut engine = engine();
    let mut group = c.benchmark_group("layout");
    let doc = document(200, Alignment::Start);
    report("doc-200", &engine.layout(&doc, Some(400.0)));
    group.bench_function("doc-200-wrapped", |b| b.iter(|| black_box(engine.layout(black_box(&doc), Some(400.0)))));
    group.bench_function("doc-200-unwrapped", |b| b.iter(|| black_box(engine.layout(black_box(&doc), None))));
    let justified = document(200, Alignment::Justify);
    report("doc-200-justify", &engine.layout(&justified, Some(400.0)));
    group.bench_function("doc-200-justified", |b| b.iter(|| black_box(engine.layout(black_box(&justified), Some(400.0)))));

    // One paragraph being typed into: the whole document is laid out again for every key.
    let small = document(20, Alignment::Start);
    group.bench_function("doc-20-relayout", |b| b.iter(|| black_box(engine.layout(black_box(&small), Some(400.0)))));

    // Typing: every iteration edits one paragraph of a 200-paragraph document, then lays the whole document out again.
    let mut typed = document(200, Alignment::Start);
    let base = typed.paragraphs[100].runs[0].text.clone();
    let mut keys = 0u32;
    group.bench_function("doc-200-edit-one", |b| {
        b.iter(|| {
            keys += 1;
            typed.paragraphs[100].runs[0].text = format!("{base}{}", char::from_u32(0x5d0 + keys % 27).unwrap());
            black_box(engine.layout(black_box(&typed), Some(400.0)))
        });
    });

    // The same text at a different width every time (a text box being resized).
    let mut width = 300.0;
    group.bench_function("doc-200-resize", |b| {
        b.iter(|| {
            width = if width > 500.0 { 300.0 } else { width + 1.0 };
            black_box(engine.layout(black_box(&doc), Some(width)))
        });
    });

    // Worst cases: a box one pixel wide breaks between every character; one paragraph of 60 000 characters.
    let narrow = document(20, Alignment::Start);
    report("narrow", &engine.layout(&narrow, Some(1.0)));
    group.bench_function("narrow-1px", |b| b.iter(|| black_box(engine.layout(black_box(&narrow), Some(1.0)))));
    let huge = RichText::new(vec![paragraph(&format!("{HEBREW} {LATIN} {ARABIC} ").repeat(120), Alignment::Start, Direction::Auto)]);
    report("huge", &engine.layout(&huge, Some(600.0)));
    group.sample_size(10);
    group.bench_function("one-paragraph-60k", |b| b.iter(|| black_box(engine.layout(black_box(&huge), Some(600.0)))));
    group.finish();
}

fn display_benches(c: &mut Criterion) {
    let mut engine = engine();
    let mut group = c.benchmark_group("display");
    let layout = engine.layout(&document(200, Alignment::Start), Some(400.0));
    let nodes = engine.to_display(&layout);
    if std::env::var_os("BENCH_CHECKSUM").is_some() {
        eprintln!("checksum display: {} nodes", nodes.len());
    }
    group.bench_function("to_display-warm", |b| b.iter(|| black_box(engine.to_display(black_box(&layout)))));
    group.bench_function("to_display-cold", |b| b.iter(|| black_box(layout.to_display())));
    group.bench_function("ink_bounds", |b| b.iter(|| black_box(engine.ink_bounds(black_box(&layout)))));
    group.finish();
}

fn query_benches(c: &mut Criterion) {
    let mut engine = engine();
    let mut group = c.benchmark_group("query");
    let layout = engine.layout(&document(200, Alignment::Start), Some(400.0));
    let len = layout.text().len();
    let offsets: Vec<usize> = (0..len).step_by(7).collect();
    group.bench_function("caret_rect-every-7th-offset", |b| {
        b.iter(|| {
            for &o in &offsets {
                black_box(layout.caret_rect(layout.caret_at(o), 1.0));
            }
        });
    });
    group.bench_function("selection_rects-all", |b| b.iter(|| black_box(layout.selection_rects(0..len))));
    group.bench_function("selection_rects-sliding", |b| {
        b.iter(|| {
            for &o in offsets.iter().step_by(20) {
                black_box(layout.selection_rects(o..o.saturating_add(300).min(len)));
            }
        });
    });
    let height = layout.bounds.y1;
    group.bench_function("hit_test-grid", |b| {
        b.iter(|| {
            for i in 0..200 {
                let y = height * f64::from(i) / 200.0;
                for x in [5.0, 120.0, 390.0] {
                    black_box(layout.hit_test(Point::new(x, y)));
                }
            }
        });
    });
    group.bench_function("move_caret-down-walk", |b| {
        b.iter(|| {
            let mut caret = layout.caret_at(0);
            let mut goal = None;
            for _ in 0..400 {
                caret = layout.move_caret(caret, Motion::Down, goal);
                goal = layout.caret_x(caret);
            }
            black_box(caret)
        });
    });
    group.bench_function("move_caret-right-walk", |b| {
        b.iter(|| {
            let mut caret = layout.caret_at(0);
            for _ in 0..1000 {
                caret = layout.move_caret(caret, Motion::Right, None);
            }
            black_box(caret)
        });
    });
    group.bench_function("word_range_at", |b| {
        b.iter(|| {
            for &o in &offsets {
                black_box(layout.word_range_at(o));
            }
        });
    });
    group.finish();
}

criterion_group!(benches, layout_benches, display_benches, query_benches);
criterion_main!(benches);
