//! Builds a small animation in code and renders a contact sheet of frames through `anim-render`: a classic tween with
//! ease, rotation and fade, a shape tween (square to circle), a masked layer, a looping graphic symbol, Hebrew and
//! Arabic text, and an onion skin.
//!
//! ```text
//! cargo run -p anim-timeline --example showcase -- OUT.png
//! ```

use std::error::Error;
use std::fs::File;
use std::io::BufWriter;

use anim_core::kurbo::{Circle, Shape};
use anim_core::{Affine, BezPath, Point, Rgba8, Size};
use anim_display::{DisplayList, Fill, FillRule, Group, Node, Paint};
use anim_render::{RenderOptions, Renderer, straight_rgba};
use anim_scene::{
    Document, EaseCurve, EaseTarget, Element, Frame, Layer, LayerKind, LoopMode, ShapeElement, SymbolInstance, SymbolKind, TextAlign, TextDirection,
    TextElement, TextKind, TextRun, TweenEase, TweenRotate, TweenType,
};
use anim_shape::{EdgeShape, FillStyle, PathInput};
use anim_text::{Alignment, Direction, Paragraph, RichText, Run, TextEngine};
use anim_timeline::{Evaluator, fade};

const CELL_W: f64 = 240.0;
const CELL_H: f64 = 180.0;
const GAP: f64 = 12.0;
const COLUMNS: usize = 3;

fn path_shape(path: BezPath, color: Rgba8) -> EdgeShape {
    EdgeShape::from_paths(&[PathInput::new(path, Some(FillStyle::Solid(color)), None)])
}

fn rect_path(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    anim_core::Rect::new(x, y, x + w, y + h).to_path(0.1)
}

fn shape(path: BezPath, color: Rgba8) -> Element {
    Element::Shape(ShapeElement::new(path_shape(path, color)))
}

fn star(radius: f64, color: Rgba8) -> Element {
    let mut path = BezPath::new();
    for i in 0..10u8 {
        let r = if i % 2 == 0 { radius } else { radius * 0.45 };
        let a = f64::from(i) * std::f64::consts::PI / 5.0 - std::f64::consts::FRAC_PI_2;
        let p = Point::new(r * a.cos(), r * a.sin());
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.close_path();
    shape(path, color)
}

const fn key(index: u32, duration: u32, elements: Vec<Element>) -> Frame {
    Frame::new(index, duration, elements)
}

fn layer(name: &str, kind: LayerKind, parent: Option<usize>, frames: Vec<Frame>) -> Layer {
    let mut l = Layer::new(name, Rgba8::opaque(90, 90, 200));
    l.kind = kind;
    l.parent_layer_index = parent;
    l.frames = frames;
    l
}

fn text(characters: &str, size: f32, color: Rgba8, align: TextAlign, direction: TextDirection) -> TextRun {
    let mut run = TextRun::new(characters, "Arial", size);
    run.fill_color = color;
    run.alignment = align;
    run.direction = direction;
    run
}

fn build() -> Result<Document, Box<dyn Error>> {
    let mut doc = Document::new(CELL_W, CELL_H, 12.0);
    doc.background_color = Rgba8::opaque(238, 242, 250);
    doc.add_symbol("spinner", SymbolKind::Graphic)?;
    doc.add_symbol("star", SymbolKind::Graphic)?;
    if let Some(symbol) = doc.library.find_symbol_mut("spinner") {
        let colors = [Rgba8::opaque(231, 76, 60), Rgba8::opaque(243, 156, 18), Rgba8::opaque(46, 204, 113), Rgba8::opaque(52, 152, 219)];
        let frames = colors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let angle = f64::from(u8::try_from(i).unwrap_or(0)) * std::f64::consts::FRAC_PI_2;
                let (x, y) = (14.0 * angle.cos(), 14.0 * angle.sin());
                key(u32::try_from(i).unwrap_or(0), 1, vec![shape(Circle::new(Point::new(x, y), 7.0).to_path(0.1), *c)])
            })
            .collect();
        symbol.timeline.layers = vec![layer("dots", LayerKind::Normal, None, frames)];
    }
    if let Some(symbol) = doc.library.find_symbol_mut("star") {
        symbol.timeline.layers = vec![layer("star", LayerKind::Normal, None, vec![key(0, 1, vec![star(16.0, Rgba8::opaque(250, 190, 40))])])];
    }

    // The tweened star: eased, one clockwise turn, growing and fading out.
    let mut start_star = SymbolInstance::new("star", Affine::translate((30.0, 140.0)));
    start_star.set_color_effect(anim_scene::ColorEffect::Alpha(1.0));
    let mut end_star = SymbolInstance::new("star", Affine::translate((200.0, 140.0)) * Affine::scale(1.7));
    end_star.set_color_effect(anim_scene::ColorEffect::Alpha(0.35));
    let mut tween_start = key(0, 11, vec![Element::SymbolInstance(Box::new(start_star))]);
    tween_start.tween_type = TweenType::Classic;
    tween_start.motion_tween_rotate = TweenRotate::Clockwise;
    tween_start.motion_tween_rotate_times = Some(1.0);
    tween_start.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Custom(vec![[0.0, 0.0], [0.4, 0.0], [0.6, 1.0], [1.0, 1.0]]) });
    let tween_end = key(11, 1, vec![Element::SymbolInstance(Box::new(end_star))]);

    // The shape tween: a red square becomes a blue circle.
    let mut morph_start = key(0, 11, vec![shape(rect_path(18.0, 20.0, 36.0, 36.0), Rgba8::opaque(231, 76, 60))]);
    morph_start.tween_type = TweenType::Shape;
    morph_start.acceleration = -40;
    let morph_end = key(11, 1, vec![shape(Circle::new(Point::new(84.0, 38.0), 22.0).to_path(0.1), Rgba8::opaque(41, 128, 185))]);

    // A mask: a circle reveals diagonal stripes.
    let mut stripes = Vec::new();
    for i in 0..8u8 {
        let color = if i % 2 == 0 { Rgba8::opaque(39, 174, 96) } else { Rgba8::opaque(22, 120, 70) };
        stripes.push(shape(rect_path(f64::from(i).mul_add(10.0, 150.0), 20.0, 10.0, 60.0), color));
    }
    let mask = shape(Circle::new(Point::new(190.0, 60.0), 26.0).to_path(0.1), Rgba8::BLACK);

    // A looping graphic symbol, locked to the parent timeline.
    let mut spinner = SymbolInstance::new("spinner", Affine::translate((120.0, 78.0)));
    spinner.symbol_type = Some(SymbolKind::Graphic);
    spinner.loop_mode = Some(LoopMode::Loop);

    let hebrew = TextElement::new(
        TextKind::Static,
        Affine::translate((10.0, 92.0)),
        220.0,
        30.0,
        vec![text("שלום עולם", 20.0, Rgba8::opaque(30, 40, 70), TextAlign::Right, TextDirection::RightToLeft)],
    );
    let arabic = TextElement::new(
        TextKind::Static,
        Affine::translate((10.0, 112.0)),
        220.0,
        30.0,
        vec![text("مرحبا بالعالم", 16.0, Rgba8::opaque(120, 40, 80), TextAlign::Right, TextDirection::Auto)],
    );
    let layers = vec![
        layer("text", LayerKind::Normal, None, vec![key(0, 12, vec![Element::Text(Box::new(hebrew)), Element::Text(Box::new(arabic))])]),
        layer("star tween", LayerKind::Normal, None, vec![tween_start, tween_end]),
        layer("spinner", LayerKind::Normal, None, vec![key(0, 12, vec![Element::SymbolInstance(Box::new(spinner))])]),
        layer("mask", LayerKind::Mask, None, vec![key(0, 12, vec![mask])]),
        layer("stripes", LayerKind::Normal, Some(3), vec![key(0, 12, stripes)]),
        layer("morph", LayerKind::Normal, None, vec![morph_start, morph_end]),
    ];
    if let Some(scene) = doc.scenes.first_mut() {
        scene.layers = layers;
    }
    Ok(doc)
}

fn label(engine: &mut TextEngine, caption: &str) -> Vec<Node> {
    let run = Run { size: 12.0, color: Rgba8::opaque(60, 70, 90), ..Run::new(caption) };
    let rich = RichText { paragraphs: vec![Paragraph { alignment: Alignment::Left, direction: Direction::Ltr, ..Paragraph::new(vec![run]) }] };
    let layout = engine.layout(&rich, None);
    engine.to_display(&layout)
}

fn cell(origin: Point, background: Rgba8, content: Vec<Node>, caption: Vec<Node>) -> Vec<Node> {
    let frame =
        Node::Fill(Fill { transform: Affine::IDENTITY, path: rect_path(0.0, 0.0, CELL_W, CELL_H), rule: FillRule::NonZero, paint: Paint::Solid(background) });
    let caption = Node::Group(Group { transform: Affine::translate((6.0, 4.0)), children: caption, ..Group::default() });
    vec![Node::Group(Group { transform: Affine::translate(origin.to_vec2()), children: [vec![frame], content, vec![caption]].concat(), ..Group::default() })]
}

fn main() -> Result<(), Box<dyn Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "timeline-showcase.png".to_owned());
    let doc = build()?;
    let issues = doc.validate();
    if !issues.is_empty() {
        return Err(format!("the demo document has warnings: {issues:?}").into());
    }
    let mut ev = Evaluator::new(&doc);
    let mut labels = TextEngine::new();
    let plan: [(u32, &str); 5] = [(0, "frame 0"), (3, "frame 3: ease, turn, morph"), (6, "frame 6"), (9, "frame 9"), (11, "frame 11: end keyframes")];
    let mut nodes: Vec<Node> = Vec::new();
    let place = |index: usize, content: Vec<Node>, caption: &str, nodes: &mut Vec<Node>, labels: &mut TextEngine| {
        let (col, row) = (index % COLUMNS, index / COLUMNS);
        let origin = Point::new(
            f64::from(u8::try_from(col).unwrap_or(0)).mul_add(CELL_W + GAP, GAP),
            f64::from(u8::try_from(row).unwrap_or(0)).mul_add(CELL_H + GAP, GAP),
        );
        let caption = label(labels, caption);
        nodes.extend(cell(origin, doc.background_color, content, caption));
    };
    let mut index = 0usize;
    for (frame, caption) in plan {
        let list = ev.frame(0, frame);
        place(index, list.nodes, caption, &mut nodes, &mut labels);
        index = index.saturating_add(1);
        if index == 5 {
            // The sixth cell: frame 7 with three onion skins before it and two after.
            let mut content = Vec::new();
            for (mut skin, alpha) in ev.onion(0, 7, 3, 2) {
                fade(&mut skin, alpha);
                content.extend(skin.nodes);
            }
            content.extend(ev.frame(0, 7).nodes);
            place(index, content, "frame 7 + onion skins (3 before, 2 after)", &mut nodes, &mut labels);
        }
    }
    let rows: f64 = 2.0;
    let width = f64::from(u8::try_from(COLUMNS).unwrap_or(3)).mul_add(CELL_W + GAP, GAP);
    let height = rows.mul_add(CELL_H + GAP, GAP);
    let sheet = DisplayList { size: Size::new(width, height), background: Some(Rgba8::opaque(200, 206, 220)), nodes };
    let options = RenderOptions::for_stage(sheet.size).with_view(Affine::scale(2.0)).with_antialias(true);
    let options = RenderOptions { width: options.width.saturating_mul(2), height: options.height.saturating_mul(2), ..options };
    let image = Renderer::new().render(&sheet, ev.images(), &options)?;
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(&out)?), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&straight_rgba(&image))?;
    println!("wrote {out} ({}x{})", image.width, image.height);
    Ok(())
}
