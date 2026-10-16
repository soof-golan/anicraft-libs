//! Visual verification of shape tweens through the evaluator. Predictions: `tests/fixtures/visual/shape_tween.expect.md`.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod common;

use anim_core::kurbo::Shape as _;
use anim_core::{Point, Rgba8};
use anim_display::{Image, Node};
use anim_scene::{Document, Element, ShapeElement, ShapeHint, TweenType};
use anim_shape::{EdgeShape, FillStyle, PathInput};
use anim_timeline::Evaluator;
use common::*;

type Rc = (f64, f64, f64, f64);

/// A filled shape: the outer rectangle (x0, y0, x1, y1) and optionally a hole.
fn shape(outer: Rc, hole: Option<Rc>, color: Rgba8) -> Element {
    let mut path = rect_path(outer.0, outer.1, outer.2 - outer.0, outer.3 - outer.1);
    if let Some(h) = hole {
        for el in rect_path(h.0, h.1, h.2 - h.0, h.3 - h.1).elements() {
            path.push(*el);
        }
    }
    Element::Shape(ShapeElement::new(EdgeShape::from_paths(&[PathInput::new(path, Some(FillStyle::Solid(color)), None)])))
}

fn shape_doc(a: Element, b: Element, hints: Vec<ShapeHint>) -> Document {
    let mut d = doc(vec![]);
    let mut start = keyframe(0, 10, vec![a]);
    start.tween_type = TweenType::Shape;
    start.shape_hints = hints;
    d.scenes[0].layers = vec![layer("s", vec![start, keyframe(10, 1, vec![b])])];
    d
}

fn frames(d: &Document) -> Vec<Image> {
    let mut ev = Evaluator::new(d);
    (0..=10u32).map(|f| draw(&ev.frame(0, f), ev.images())).collect()
}

fn probe(img: &Image, f: usize, points: &[(u32, u32, Rgba8)]) {
    for &(x, y, c) in points {
        assert!(is(img, x, y, c), "f{f} ({x}, {y}) is {:?}, want {c:?}", pixel(img, x, y));
    }
}

const SQUARE: Rc = (20.0, 20.0, 80.0, 80.0);

#[test]
fn ring_grow_hole_appears_and_stays_open() {
    let d = shape_doc(shape(SQUARE, None, RED), shape(SQUARE, Some((40.0, 40.0, 60.0, 60.0)), RED), vec![]);
    let fs = frames(&d);
    probe(&fs[0], 0, &[(50, 50, RED)]);
    probe(&fs[1], 1, &[(50, 50, WHITE), (47, 50, RED)]);
    probe(&fs[5], 5, &[(50, 50, WHITE), (46, 50, WHITE), (44, 50, RED), (50, 44, RED), (21, 21, RED), (19, 50, WHITE)]);
    probe(&fs[9], 9, &[(42, 50, WHITE), (40, 50, RED), (58, 58, WHITE), (60, 60, RED)]);
    probe(&fs[10], 10, &[(41, 50, WHITE), (39, 50, RED)]);
    for (f, img) in fs.iter().enumerate().skip(1) {
        probe(img, f, &[(50, 50, WHITE), (19, 50, WHITE), (20, 50, RED)]);
    }
    let sheet = sheet(&fs, 4, 2);
    save(&sheet, "ring_grow");
    golden(&sheet, "ring_grow");
}

#[test]
fn ring_shrink_hole_collapses() {
    let d = shape_doc(shape(SQUARE, Some((40.0, 40.0, 60.0, 60.0)), RED), shape(SQUARE, None, RED), vec![]);
    let fs = frames(&d);
    probe(&fs[0], 0, &[(41, 50, WHITE), (39, 50, RED)]);
    probe(&fs[5], 5, &[(50, 50, WHITE), (46, 50, WHITE), (44, 50, RED)]);
    probe(&fs[9], 9, &[(50, 50, WHITE), (48, 50, RED)]);
    probe(&fs[10], 10, &[(50, 50, RED)]);
    save(&sheet(&fs, 4, 2), "ring_shrink");
}

#[test]
fn ring_offcentre_hole_grows_around_its_own_centre() {
    let d = shape_doc(shape(SQUARE, None, RED), shape(SQUARE, Some((30.0, 40.0, 45.0, 60.0)), RED), vec![]);
    let fs = frames(&d);
    probe(&fs[5], 5, &[(37, 50, WHITE), (34, 50, WHITE), (33, 50, RED), (40, 50, WHITE), (41, 50, RED), (37, 44, RED), (37, 56, RED), (50, 50, RED)]);
    probe(&fs[10], 10, &[(31, 50, WHITE), (44, 50, WHITE), (46, 50, RED), (29, 50, RED)]);
    save(&sheet(&fs, 4, 2), "ring_offcentre");
}

#[test]
fn ring_colour_tween_keeps_the_hole() {
    let d = shape_doc(shape(SQUARE, None, RED), shape(SQUARE, Some((40.0, 40.0, 60.0, 60.0)), BLUE), vec![]);
    let fs = frames(&d);
    probe(&fs[0], 0, &[(25, 50, RED)]);
    assert!(near(&fs[5], 25, 50, [128, 0, 128], 2), "{:?}", pixel(&fs[5], 25, 50));
    probe(&fs[10], 10, &[(25, 50, BLUE)]);
    probe(&fs[8], 8, &[(50, 50, WHITE)]);
    save(&sheet(&fs, 4, 2), "ring_colour");
}

fn polygon_area(ev: &mut Evaluator<'_>, frame: u32) -> f64 {
    let list = ev.frame(0, frame);
    let mut area = 0.0;
    list.visit(&mut |n| {
        if let Node::Fill(f) = n {
            area += f.path.area().abs();
        }
    });
    area
}

#[test]
fn hint_pins_corner_to_corner_and_makes_a_diamond() {
    let hint = ShapeHint { id: 0, start: Point::new(20.0, 20.0), end: Point::new(80.0, 20.0) };
    // Without hints a square tweening to the same square stays a square.
    let plain = frames(&shape_doc(shape(SQUARE, None, RED), shape(SQUARE, None, RED), vec![]));
    for (f, img) in plain.iter().enumerate() {
        probe(img, f, &[(22, 22, RED), (78, 78, RED), (78, 22, RED), (22, 78, RED)]);
    }
    let d = shape_doc(shape(SQUARE, None, RED), shape(SQUARE, None, RED), vec![hint]);
    let fs = frames(&d);
    save(&sheet(&fs, 4, 2), "hint_diamond");
    probe(&fs[0], 0, &[(22, 22, RED)]);
    probe(&fs[10], 10, &[(22, 22, RED)]);
    probe(
        &fs[5],
        5,
        &[(22, 22, WHITE), (78, 22, WHITE), (22, 78, WHITE), (78, 78, WHITE), (50, 50, RED), (50, 25, RED), (25, 50, RED), (75, 50, RED), (50, 82, WHITE)],
    );
    let mut ev = Evaluator::new(&d);
    let area = polygon_area(&mut ev, 5);
    assert!((1500.0..2100.0).contains(&area), "diamond area {area}");
    save(&sheet(&fs, 4, 2), "hint_diamond");
    golden(&sheet(&fs, 4, 2), "hint_diamond");
}
