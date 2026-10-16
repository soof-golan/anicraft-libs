//! Timeline evaluation benchmarks on synthetic stress documents (the verification fixtures, scaled up): a thousand
//! rotating, fading instances, deeply nested looping symbols, hundreds of layers with masks and onion skins, shape
//! tweens across a whole scene and a hundred layers guided by one long motion path.
//!
//! The XFL samples cannot be read from this crate (the importer lives in the app repo), so there is no
//! sample-driven bench here.
//!
//! With `BENCH_CHECKSUM=1` each case prints a checksum of the display lists it produces before timing, so that an
//! optimisation can be checked to leave the output unchanged: compare the `checksum` lines of two runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    reason = "benchmark code over synthetic documents"
)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::hint::black_box;

use anim_core::kurbo::{Circle, Shape};
use anim_core::{Affine, BezPath, Point, Rgba8};
use anim_scene::{
    ColorEffect, Document, EaseCurve, EaseTarget, Element, Frame, Layer, LayerKind, LoopMode, ShapeElement, SymbolInstance, SymbolKind, TweenEase,
    TweenRotate, TweenType,
};
use anim_shape::{EdgeShape, FillStyle, PathInput, StrokeStyle};
use anim_timeline::{Evaluator, fade};
use criterion::{Criterion, criterion_group, criterion_main};

const RED: Rgba8 = Rgba8::opaque(220, 40, 40);
const BLUE: Rgba8 = Rgba8::opaque(40, 40, 220);

fn rect_path(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x, y));
    p.line_to((x + w, y));
    p.line_to((x + w, y + h));
    p.line_to((x, y + h));
    p.close_path();
    p
}

fn star_path(cx: f64, cy: f64, outer: f64, inner: f64, n: usize) -> BezPath {
    let mut p = BezPath::new();
    for i in 0..n * 2 {
        let r = if i % 2 == 0 { outer } else { inner };
        let a = i as f64 * std::f64::consts::PI / n as f64;
        let pt = Point::new(cx + r * a.cos(), cy + r * a.sin());
        if i == 0 {
            p.move_to(pt);
        } else {
            p.line_to(pt);
        }
    }
    p.close_path();
    p
}

fn filled(path: BezPath, color: Rgba8) -> Element {
    Element::Shape(ShapeElement::new(EdgeShape::from_paths(&[PathInput::new(path, Some(FillStyle::Solid(color)), None)])))
}

fn stroked(path: BezPath) -> Element {
    Element::Shape(ShapeElement::new(EdgeShape::from_paths(&[PathInput::new(path, None, Some(StrokeStyle::plain(1.0, Rgba8::BLACK)))])))
}

fn instance(name: &str, m: Affine) -> Element {
    Element::SymbolInstance(Box::new(SymbolInstance::new(name, m)))
}

fn layer(name: &str, kind: LayerKind, parent: Option<usize>, frames: Vec<Frame>) -> Layer {
    let mut l = Layer::new(name, Rgba8::opaque(1, 2, 3));
    l.kind = kind;
    l.parent_layer_index = parent;
    l.frames = frames;
    l
}

fn add_symbol(d: &mut Document, name: &str, kind: SymbolKind, layers: Vec<Layer>) {
    d.add_symbol(name, kind).unwrap();
    d.library.find_symbol_mut(name).unwrap().timeline.layers = layers;
}

fn document(layers: Vec<Layer>) -> Document {
    let mut d = Document::new(1000.0, 1000.0, 24.0);
    d.scenes[0].layers = layers;
    d
}

/// A rotating, fading tween of one instance from `from` to `to` over `frames` frames, with a custom ease.
fn tween_layer(name: &str, symbol: &str, from: Point, to: Point, frames: u32) -> Layer {
    let mut a = SymbolInstance::new(symbol, Affine::translate(from.to_vec2()));
    a.set_color_effect(ColorEffect::Alpha(1.0));
    let mut b = SymbolInstance::new(symbol, Affine::translate(to.to_vec2()) * Affine::rotate(0.4));
    b.set_color_effect(ColorEffect::Alpha(0.2));
    let mut start = Frame::new(0, frames, vec![Element::SymbolInstance(Box::new(a))]);
    start.tween_type = TweenType::Classic;
    start.motion_tween_rotate = TweenRotate::Clockwise;
    start.motion_tween_rotate_times = Some(2.0);
    start.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Custom(vec![[0.0, 0.0], [0.4, 0.0], [0.6, 1.0], [1.0, 1.0]]) });
    let end = Frame::new(frames, 1, vec![Element::SymbolInstance(Box::new(b))]);
    layer(name, LayerKind::Normal, None, vec![start, end])
}

/// One layer whose single keyframe has a thousand tweened instances of "dot".
fn many_instances() -> Document {
    let mut d = document(Vec::new());
    add_symbol(&mut d, "dot", SymbolKind::Graphic, vec![layer("d", LayerKind::Normal, None, vec![Frame::new(0, 1, vec![filled(rect_path(-4.0, -4.0, 8.0, 8.0), RED)])])]);
    let n = 1000;
    let mut a = Vec::new();
    let mut b = Vec::new();
    for i in 0..n {
        let x = f64::from(i % 40) * 20.0;
        let y = f64::from(i / 40) * 20.0;
        let mut ia = SymbolInstance::new("dot", Affine::translate((x, y)));
        ia.set_color_effect(ColorEffect::Alpha(1.0));
        let mut ib = SymbolInstance::new("dot", Affine::translate((x + 10.0, y + 10.0)) * Affine::rotate(0.8));
        ib.set_color_effect(ColorEffect::Alpha(0.3));
        a.push(Element::SymbolInstance(Box::new(ia)));
        b.push(Element::SymbolInstance(Box::new(ib)));
    }
    let mut start = Frame::new(0, 60, a);
    start.tween_type = TweenType::Classic;
    start.motion_tween_rotate = TweenRotate::Clockwise;
    start.motion_tween_rotate_times = Some(2.0);
    d.scenes[0].layers = vec![layer("tweens", LayerKind::Normal, None, vec![start, Frame::new(60, 1, b)])];
    d
}

/// Looping graphic symbols nested seven deep, four instances per level: 4^6 leaves.
fn nested_symbols() -> Document {
    let mut d = document(Vec::new());
    let depth = 7;
    add_symbol(&mut d, "L6", SymbolKind::Graphic, vec![layer("leaf", LayerKind::Normal, None, vec![Frame::new(0, 4, vec![filled(rect_path(0.0, 0.0, 2.0, 2.0), BLUE)])])]);
    for level in (0..depth - 1).rev() {
        let child = format!("L{}", level + 1);
        let elements: Vec<Element> = (0..4)
            .map(|i| {
                let mut inst = SymbolInstance::new(&child, Affine::translate((f64::from(i) * 3.0, f64::from(i) * 2.0)));
                inst.symbol_type = Some(SymbolKind::Graphic);
                inst.loop_mode = Some(LoopMode::Loop);
                inst.first_frame = i as u32;
                Element::SymbolInstance(Box::new(inst))
            })
            .collect();
        add_symbol(&mut d, &format!("L{level}"), SymbolKind::Graphic, vec![layer("kids", LayerKind::Normal, None, vec![Frame::new(0, 4, elements)])]);
    }
    d.scenes[0].layers = vec![layer("root", LayerKind::Normal, None, vec![Frame::new(0, 8, vec![instance("L0", Affine::IDENTITY)])])];
    d
}

/// Four hundred tweened layers, a quarter of them under animated masks.
fn many_layers() -> Document {
    let mut d = document(Vec::new());
    add_symbol(&mut d, "dot", SymbolKind::Graphic, vec![layer("d", LayerKind::Normal, None, vec![Frame::new(0, 1, vec![filled(rect_path(-4.0, -4.0, 8.0, 8.0), RED)])])]);
    let mut layers = Vec::new();
    for i in 0..100u32 {
        let base = layers.len();
        let y = f64::from(i) * 9.0;
        // Mask first in the list: it masks the layers whose parent it is.
        let mut grow = Frame::new(0, 30, vec![filled(Circle::new(Point::new(200.0, y), 20.0).to_path(0.1), Rgba8::BLACK)]);
        grow.tween_type = TweenType::Shape;
        let to = Frame::new(30, 1, vec![filled(Circle::new(Point::new(300.0, y), 60.0).to_path(0.1), Rgba8::BLACK)]);
        layers.push(layer("mask", LayerKind::Mask, None, vec![grow, to]));
        for k in 0..3 {
            let l = tween_layer("m", "dot", Point::new(f64::from(k) * 40.0, y), Point::new(300.0 + f64::from(k) * 40.0, y + 20.0), 30);
            layers.push(layer("masked", LayerKind::Masked, Some(base), l.frames));
        }
    }
    d.scenes[0].layers = layers;
    d
}

/// Twenty layers of a 120-point circle turning into a 120-spike star over 48 frames.
fn shape_tweens() -> Document {
    let mut layers = Vec::new();
    for i in 0..20 {
        let y = f64::from(i) * 40.0;
        let mut start = Frame::new(0, 48, vec![filled(Circle::new(Point::new(100.0, y), 15.0).to_path(0.001), RED)]);
        start.tween_type = TweenType::Shape;
        let end = Frame::new(48, 1, vec![filled(star_path(300.0, y, 18.0, 8.0, 120), BLUE)]);
        layers.push(layer("shape", LayerKind::Normal, None, vec![start, end]));
    }
    document(layers)
}

/// A guide layer with a 300-point polyline and a hundred guided, oriented tweens.
fn guided() -> Document {
    let mut d = document(Vec::new());
    add_symbol(&mut d, "dot", SymbolKind::Graphic, vec![layer("d", LayerKind::Normal, None, vec![Frame::new(0, 1, vec![filled(rect_path(-4.0, -2.0, 24.0, 4.0), RED)])])]);
    let mut path = BezPath::new();
    for i in 0..300 {
        let p = Point::new(f64::from(i) * 3.0, 400.0 + 100.0 * (f64::from(i) * 0.1).sin());
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    let mut layers = vec![layer("guide", LayerKind::Guide, None, vec![Frame::new(0, 31, vec![stroked(path)])])];
    for i in 0..100 {
        let mut l = tween_layer("g", "dot", Point::new(0.0, 0.0), Point::new(900.0, 0.0), 30);
        l.kind = LayerKind::Guided;
        l.parent_layer_index = Some(0);
        if let Some(f) = l.frames.first_mut() {
            f.motion_tween_snap = true;
            f.motion_tween_orient_to_path = i % 2 == 0;
        }
        layers.push(l);
    }
    d.scenes[0].layers = layers;
    d
}

fn checksum(lists: &[anim_display::DisplayList]) -> u64 {
    let mut h = DefaultHasher::new();
    format!("{lists:?}").hash(&mut h);
    h.finish()
}

fn bench(c: &mut Criterion) {
    let check = std::env::var_os("BENCH_CHECKSUM").is_some();
    let docs: [(&str, Document, u32); 5] = [
        ("many_instances_1000", many_instances(), 29),
        ("nested_symbols_4pow6", nested_symbols(), 5),
        ("many_layers_masks_400", many_layers(), 14),
        ("shape_tweens_20", shape_tweens(), 23),
        ("guided_100", guided(), 14),
    ];
    for (name, doc, frame) in &docs {
        let mut ev = Evaluator::new(doc);
        if check {
            let lists: Vec<_> = [0, *frame, frame + 1].iter().map(|f| ev.frame(0, *f)).collect();
            eprintln!("checksum {name} {:016x}", checksum(&lists));
        }
        // Warm: the evaluator keeps its shape-tween plans between frames.
        c.bench_function(&format!("frame/{name}"), |b| b.iter(|| ev.frame(0, black_box(*frame))));
    }
    // Cold: a new evaluator per frame pays for every shape-tween plan.
    let doc = shape_tweens();
    c.bench_function("frame_cold/shape_tweens_20", |b| b.iter(|| Evaluator::new(&doc).frame(0, black_box(23))));

    // Playing the whole scene.
    let doc = many_layers();
    let mut ev = Evaluator::new(&doc);
    c.bench_function("play_31_frames/many_layers_masks_400", |b| {
        b.iter(|| {
            for f in 0..31 {
                black_box(ev.frame(0, f));
            }
        });
    });
    // Onion skins: five frames either side.
    if check {
        let skins: Vec<_> = ev
            .onion(0, 14, 5, 5)
            .into_iter()
            .map(|(mut l, a)| {
                fade(&mut l, a);
                l
            })
            .collect();
        eprintln!("checksum onion {:016x}", checksum(&skins));
    }
    c.bench_function("onion_5_5/many_layers_masks_400", |b| b.iter(|| ev.onion(0, black_box(14), 5, 5)));
}

criterion_group!(benches, bench);
criterion_main!(benches);
