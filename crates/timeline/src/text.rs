//! Text elements to glyph outlines through `anim-text`.

use anim_core::Affine;
use anim_display::Node;
use anim_scene::{TextAlign, TextDirection, TextElement, TextRun};
use anim_text::{Alignment, Direction, Paragraph, RichText, Run, TextEngine};

/// Converts the runs of a text element into a [`RichText`].
///
/// Paragraph breaks (`\r`, `\n`, `\r\n`) split the runs. Alignment and direction are per paragraph in `anim-text`
/// but per run in XFL, so each paragraph takes them from its first run. Font names are resolved to installed
/// families (`Arial-BoldMT` becomes the bold face of a substitute).
pub fn rich_text(engine: &mut TextEngine, element: &TextElement) -> RichText {
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut current = Paragraph::new(Vec::new());
    let mut started = false;
    let mut last_run: Option<Run> = None;
    for source in &element.text_runs {
        let style = convert_run(engine, source);
        let normalised = source.characters.replace("\r\n", "\n");
        for (i, piece) in normalised.split(['\r', '\n']).enumerate() {
            if i > 0 {
                finish(&mut paragraphs, &mut current, last_run.as_ref());
                started = false;
            }
            if !started {
                let (alignment, direction) = paragraph_style(source);
                current.alignment = alignment;
                current.direction = direction;
                current.line_spacing = spacing_attr(source, "lineSpacing");
                current.indent = spacing_attr(source, "indent");
                current.margin_left = spacing_attr(source, "leftMargin");
                current.margin_right = spacing_attr(source, "rightMargin");
                started = true;
            }
            if !piece.is_empty() {
                current.runs.push(style.with_text(piece));
            }
            last_run = Some(style.clone());
        }
    }
    finish(&mut paragraphs, &mut current, last_run.as_ref());
    RichText { paragraphs }
}

fn finish(out: &mut Vec<Paragraph>, current: &mut Paragraph, style: Option<&Run>) {
    let mut done = std::mem::replace(current, Paragraph::new(Vec::new()));
    if done.runs.is_empty() {
        // An empty paragraph still takes the height of its style.
        done.runs.push(style.map_or_else(Run::default, |s| s.with_text("")));
    }
    out.push(done);
}

fn convert_run(engine: &mut TextEngine, run: &TextRun) -> Run {
    let resolved = engine.resolve_font_name(&run.face);
    Run {
        text: String::new(),
        family: resolved.family,
        size: f64::from(run.size),
        bold: run.bold || resolved.bold,
        italic: run.italic || resolved.italic,
        color: run.fill_color.with_alpha_f32(run.alpha),
        letter_spacing: f64::from(run.letter_spacing),
        underline: false,
    }
}

/// A paragraph spacing (`lineSpacing`, `indent`, `leftMargin`, `rightMargin`) that XFL keeps on the text attributes,
/// in pixels; 0 when absent or not a finite number.
fn spacing_attr(run: &TextRun, name: &str) -> f64 {
    run.extra.attr(name).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0)
}

const fn paragraph_style(run: &TextRun) -> (Alignment, Direction) {
    let alignment = match run.alignment {
        TextAlign::Left => Alignment::Left,
        TextAlign::Right => Alignment::Right,
        TextAlign::Center => Alignment::Center,
        TextAlign::Justify => Alignment::Justify,
    };
    let direction = match run.direction {
        TextDirection::Auto => Direction::Auto,
        TextDirection::LeftToRight => Direction::Ltr,
        TextDirection::RightToLeft => Direction::Rtl,
    };
    (alignment, direction)
}

/// Glyph nodes of a text element in the element's own space (apply the element matrix on the group around them).
///
/// A boxed element wraps at its width; an auto-expanding one does not wrap. The `left` attribute that XFL stores
/// on text elements (kept in `extra`) moves the text box inside the element space.
pub fn text_nodes(engine: &mut TextEngine, element: &TextElement) -> Vec<Node> {
    let rich = rich_text(engine, element);
    if rich.is_empty() {
        return Vec::new();
    }
    let max_width = (!element.auto_expand && element.width.is_finite() && element.width > 0.0).then_some(element.width);
    let layout = engine.layout(&rich, max_width);
    let nodes = engine.to_display(&layout);
    let left = element.extra.attr("left").and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0);
    if nodes.is_empty() || left == 0.0 {
        return nodes;
    }
    vec![Node::Group(anim_display::Group { transform: Affine::translate((left, 0.0)), children: nodes, ..anim_display::Group::default() })]
}

#[cfg(test)]
mod tests {
    use anim_scene::TextKind;

    use super::*;

    #[test]
    fn paragraph_spacing_comes_from_the_text_attributes() {
        let mut run = TextRun::new("one\rtwo", "Arial", 20.0);
        run.extra.set_attr("lineSpacing", "6");
        run.extra.set_attr("indent", "4.5");
        run.extra.set_attr("leftMargin", "8");
        run.extra.set_attr("rightMargin", "not a number");
        let element = TextElement::new(TextKind::Static, Affine::IDENTITY, 200.0, 80.0, vec![run]);
        let rich = rich_text(&mut TextEngine::new(), &element);
        assert_eq!(rich.paragraphs.len(), 2);
        for paragraph in &rich.paragraphs {
            assert!((paragraph.line_spacing - 6.0).abs() < f64::EPSILON);
            assert!((paragraph.indent - 4.5).abs() < f64::EPSILON);
            assert!((paragraph.margin_left - 8.0).abs() < f64::EPSILON);
            assert!(paragraph.margin_right.abs() < f64::EPSILON, "garbage counts as absent");
        }
    }
}
