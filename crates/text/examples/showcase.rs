//! Renders a contact sheet of what the text engine does: bidi paragraphs, Arabic joining, justification, rich runs,
//! selection rectangles and caret positions, drawn through `anim-render`.
//!
//! ```text
//! cargo run -p anim-text --example showcase -- OUT.png
//! ```

use std::error::Error;
use std::ops::Range;

use anim_core::kurbo::Shape;
use anim_core::{Affine, Point, Rect, Rgba8, Size};
use anim_display::{DisplayList, Fill, FillRule, Group, ImageStore, Node, Paint, Stroke, StrokeStyle};
use anim_render::{RenderOptions, Renderer, straight_rgba};
use anim_text::{Affinity, Alignment, Caret, Direction, Paragraph, RichText, Run, TextEngine, TextLayout};

const WIDTH: f64 = 1040.0;
const COLUMN: f64 = 480.0;
const MARGIN: f64 = 24.0;
const PAD: f64 = 12.0;
const INK: Rgba8 = Rgba8::opaque(28, 32, 44);
const MUTED: Rgba8 = Rgba8::opaque(110, 118, 136);

fn rect_node(rect: Rect, color: Rgba8) -> Node {
    Node::Fill(Fill { transform: Affine::IDENTITY, path: rect.to_path(0.1), rule: FillRule::NonZero, paint: Paint::Solid(color) })
}

fn outline_node(rect: Rect, color: Rgba8) -> Node {
    let style = StrokeStyle { width: 1.0, ..StrokeStyle::default() };
    Node::Stroke(Stroke { transform: Affine::IDENTITY, path: rect.to_path(0.1), style, paint: Paint::Solid(color) })
}

fn group(at: Point, children: Vec<Node>) -> Node {
    Node::Group(Group { transform: Affine::translate(at.to_vec2()), children, ..Group::default() })
}

fn run(text: &str, size: f64) -> Run {
    Run { size, color: INK, ..Run::new(text) }
}

/// The byte range of the first `needle` in `text` (empty when absent).
fn range_of(text: &str, needle: &str) -> Range<usize> {
    text.find(needle).map_or(0..0, |start| start..start.saturating_add(needle.len()))
}

/// One framed sample: a caption, the text, highlighted ranges and caret positions.
struct Panel {
    caption: &'static str,
    text: RichText,
    selections: Vec<Range<usize>>,
    carets: Vec<usize>,
    /// Draw a caret at every character boundary (both sides at direction changes).
    all_carets: bool,
}

impl Panel {
    const fn new(caption: &'static str, text: RichText) -> Self {
        Self { caption, text, selections: Vec::new(), carets: Vec::new(), all_carets: false }
    }
}

/// Draws the panel at `origin` and returns its node and its height.
fn draw(engine: &mut TextEngine, panel: &Panel, origin: Point) -> (Node, f64) {
    let layout = engine.layout(&panel.text, Some(2.0f64.mul_add(-PAD, COLUMN)));
    let caption = engine.layout(&RichText::new(vec![Paragraph::new(vec![Run { color: MUTED, ..run(panel.caption, 12.0) }])]), None);
    let body_top = caption.bounds.height() + PAD;
    let height = body_top + layout.bounds.height() + PAD;

    let frame = Rect::new(0.0, 0.0, COLUMN, height);
    let mut nodes = vec![rect_node(frame, Rgba8::opaque(250, 251, 253)), outline_node(frame, Rgba8::opaque(205, 211, 224))];
    nodes.push(group(Point::new(PAD, 6.0), engine.to_display(&caption)));

    let mut body = Vec::new();
    for range in &panel.selections {
        for rect in layout.selection_rects(range.clone()) {
            body.push(rect_node(rect, Rgba8::new(70, 130, 240, 90)));
        }
    }
    for line in &layout.lines {
        // Faint baselines show where each line sits.
        body.push(rect_node(Rect::new(line.rect.x0, line.baseline, line.rect.x1.max(line.rect.x0 + 1.0), line.baseline + 0.6), Rgba8::new(220, 90, 90, 90)));
    }
    body.extend(engine.to_display(&layout));
    body.extend(carets(&layout, panel));
    nodes.push(group(Point::new(PAD, body_top), body));
    (group(origin, nodes), height)
}

fn carets(layout: &TextLayout, panel: &Panel) -> Vec<Node> {
    let red = Rgba8::opaque(214, 40, 57);
    let mut nodes = Vec::new();
    if panel.all_carets {
        let text = layout.text();
        for offset in (0..=text.len()).filter(|&o| text.is_char_boundary(o)) {
            for (affinity, color) in [(Affinity::Downstream, Rgba8::new(0, 150, 140, 220)), (Affinity::Upstream, Rgba8::new(240, 150, 30, 220))] {
                if let Some(rect) = layout.caret_rect(Caret { offset, affinity }, 1.0) {
                    nodes.push(rect_node(Rect::new(rect.x0, rect.height().mul_add(0.8, rect.y0), rect.x1, rect.y1), color));
                }
            }
        }
    }
    for &offset in &panel.carets {
        if let Some(rect) = layout.caret_rect(layout.caret_at(offset), 1.5) {
            nodes.push(rect_node(rect, red));
        }
    }
    nodes
}

/// The samples, each with the column it goes in.
fn panels() -> Vec<(usize, Panel)> {
    let latin = "Typography is the craft of arranging type. Justified paragraphs stretch the spaces so that every line but the last reaches both margins.";
    let hebrew = "שלום עולם! זוהי פסקה בעברית. השורות מתחילות בצד ימין, והסמן נע ימינה כלפי תחילת הטקסט.";
    let arabic = "مرحبا بالعالم، هذا نص عربي تتصل حروفه ببعضها: بببب، لا، الله.";
    let mixed = "The word שלום means peace; call 054-1234567 or write سلام in 2024.";

    let mut justified = Panel::new(
        "Latin, justified, wrapped at 456 px: a selection and a caret",
        RichText::new(vec![Paragraph { alignment: Alignment::Justify, direction: Direction::Ltr, ..Paragraph::new(vec![run(latin, 20.0)]) }]),
    );
    justified.selections = vec![range_of(latin, "arranging type. Justified")];
    justified.carets = vec![range_of(latin, "stretch").start];

    let mut hebrew_panel =
        Panel::new("Hebrew: right to left, start edge on the right; the first sentence selected", RichText::new(vec![Paragraph::new(vec![run(hebrew, 22.0)])]));
    hebrew_panel.selections = vec![range_of(hebrew, "שלום עולם!")];
    hebrew_panel.carets = vec![0];

    let mut arabic_panel =
        Panel::new("Arabic: letters take initial, medial, final and ligature forms", RichText::new(vec![Paragraph::new(vec![run(arabic, 24.0)])]));
    arabic_panel.carets = vec![0];

    let mut mixed_panel = Panel::new(
        "Mixed text: a selection ending inside a Hebrew word is two separate rectangles",
        RichText::new(vec![Paragraph { direction: Direction::Ltr, ..Paragraph::new(vec![run(mixed, 20.0)]) }]),
    );
    mixed_panel.selections = vec![range_of(mixed, "d של"), range_of(mixed, "054-1234567")];

    let mut carets_panel = Panel::new(
        "Caret places: teal sticks to the next letter, orange to the previous",
        RichText::new(vec![Paragraph { direction: Direction::Ltr, ..Paragraph::new(vec![run("abc שלום def", 28.0)]) }]),
    );
    carets_panel.all_carets = true;

    let rich = RichText::new(vec![
        Paragraph::new(vec![
            Run { bold: true, ..run("Bold ", 22.0) },
            Run { italic: true, color: Rgba8::opaque(200, 60, 30), ..run("italic red ", 22.0) },
            Run { underline: true, color: Rgba8::opaque(30, 110, 200), ..run("underlined blue ", 22.0) },
            Run { letter_spacing: 4.0, ..run("spaced ", 22.0) },
            run("BIG", 38.0),
        ]),
        Paragraph {
            alignment: Alignment::Center,
            line_spacing: 6.0,
            margin_left: 30.0,
            margin_right: 30.0,
            ..Paragraph::new(vec![run("Centered with margins and extra line spacing, wrapping over several lines of the panel.", 18.0)])
        },
        Paragraph {
            alignment: Alignment::End, direction: Direction::Rtl, ..Paragraph::new(vec![run("סוף השורה: יישור End בפסקה מימין לשמאל", 18.0)])
        },
    ]);
    let rich_panel = Panel::new("Rich runs; per-paragraph alignment, margins and spacing", rich);

    vec![(0, justified), (0, hebrew_panel), (0, arabic_panel), (1, mixed_panel), (1, carets_panel), (1, rich_panel)]
}

fn main() -> Result<(), Box<dyn Error>> {
    let out = std::env::args().nth(1).ok_or("usage: showcase OUT.png")?;
    let mut engine = TextEngine::new();

    let title = RichText::new(vec![Paragraph::new(vec![Run { bold: true, ..run("anim-text: shaping, bidi, carets and selections", 26.0) }])]);
    let title_layout = engine.layout(&title, None);
    let mut nodes = vec![group(Point::new(MARGIN, 18.0), engine.to_display(&title_layout))];
    let top = 18.0 + title_layout.bounds.height() + 18.0;

    let mut y = [top, top];
    for (column, panel) in panels() {
        let Some(slot) = y.get_mut(column) else { continue };
        let x = f64::from(u8::try_from(column)?).mul_add(COLUMN + MARGIN, MARGIN);
        let (node, height) = draw(&mut engine, &panel, Point::new(x, *slot));
        nodes.push(node);
        *slot += height + 16.0;
    }
    let height = y.iter().copied().fold(0.0, f64::max) + 8.0;

    let list = DisplayList { size: Size::new(WIDTH, height), background: Some(Rgba8::opaque(238, 241, 247)), nodes };
    let options = RenderOptions::scaled_stage(list.size, 1.5);
    let image = Renderer::new().render(&list, &ImageStore::new(), &options)?;
    anim_testkit::write_png(std::path::Path::new(&out), image.width, image.height, &straight_rgba(&image))?;
    println!("wrote {out} ({}x{})", image.width, image.height);
    Ok(())
}
