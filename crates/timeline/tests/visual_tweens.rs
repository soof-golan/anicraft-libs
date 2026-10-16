//! Visual verification of classic tweens: rotation, eases, guides, keyframe edges, colour transforms, onion skins.
//! Every assertion here was predicted in `tests/fixtures/visual/*.expect.md` before the first render.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops,
    clippy::manual_midpoint,
    clippy::items_after_statements,
    clippy::missing_const_for_fn
)]

mod common;

use std::f64::consts::{PI, TAU};

use anim_core::{Affine, BezPath, ColorTransform, Point, Rgba8};
use anim_display::{DisplayList, Image};
use anim_scene::{
    ColorEffect, Document, EaseCurve, EaseTarget, Element, Frame, LayerKind, ShapeElement, SymbolInstance, SymbolKind, TweenEase, TweenRotate, TweenType,
};
use anim_shape::{EdgeShape, PathInput, StrokeStyle};
use anim_timeline::{Evaluator, fade, xform};
use common::*;

fn bar_symbol(d: &mut Document) {
    add_symbol(d, "bar", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, -3.0, 30.0, 6.0, RED), rect(24.0, -3.0, 6.0, 6.0, BLUE)])])]);
}

fn box_symbol(d: &mut Document) {
    add_symbol(d, "box", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED)])])]);
}

/// A tween of `symbol` from `a` to `b` over frames 0..10 (end keyframe at 10).
fn tween_doc(symbol: &str, a: Affine, b: Affine, configure: impl FnOnce(&mut Frame)) -> Document {
    let mut d = doc(vec![]);
    bar_symbol(&mut d);
    box_symbol(&mut d);
    let mut start = keyframe(0, 10, vec![instance(symbol, a)]);
    start.tween_type = TweenType::Classic;
    configure(&mut start);
    d.scenes[0].layers = vec![layer("mover", vec![start, keyframe(10, 1, vec![instance(symbol, b)])])];
    d
}

fn matrix_at(ev: &mut Evaluator<'_>, frame: u32) -> Affine {
    let list = ev.frame(0, frame);
    top_groups(&list)[0].transform
}

fn render(ev: &mut Evaluator<'_>, frame: u32) -> Image {
    draw(&ev.frame(0, frame), ev.images())
}

fn deg(d: f64) -> f64 {
    d.to_radians()
}

/// Pixel under the tip marker and under its mirror image through the registration point (50, 50).
fn assert_tip(img: &Image, theta_deg: f64, label: &str) {
    let (s, c) = deg(theta_deg).sin_cos();
    let (tx, ty) = (50.0 + 27.0 * c, 50.0 + 27.0 * s);
    assert!(is(img, px(tx), px(ty), BLUE), "{label}: tip ({tx:.2}, {ty:.2}) is {:?}", pixel(img, px(tx), px(ty)));
    let (mx, my) = (50.0 - 27.0 * c, 50.0 - 27.0 * s);
    assert!(is(img, px(mx), px(my), WHITE), "{label}: mirror ({mx:.2}, {my:.2}) is {:?}", pixel(img, px(mx), px(my)));
}

// ---- rotation ----

#[test]
fn rot_cw2_matches_the_predicted_angle_every_frame() {
    let d = tween_doc("bar", Affine::translate((50.0, 50.0)), Affine::translate((50.0, 50.0)) * Affine::rotate(deg(90.0)), |f| {
        f.motion_tween_rotate = TweenRotate::Clockwise;
        f.motion_tween_rotate_times = Some(2.0);
    });
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for f in 0..=10u32 {
        let theta = 81.0 * f64::from(f);
        let img = render(&mut ev, f);
        assert_tip(&img, theta, &format!("rot_cw2 f{f}"));
        let m = matrix_at(&mut ev, f);
        let dec = xform::decompose(m);
        assert!((m.determinant() - 1.0).abs() < 1e-9, "f{f}: determinant {}", m.determinant());
        assert!(xform::wrap_angle(dec.rotation - deg(theta)).abs() < 1e-9, "f{f}: rotation {} want {theta}", dec.rotation.to_degrees());
        assert!((dec.tx - 50.0).abs() < 1e-9 && (dec.ty - 50.0).abs() < 1e-9);
        frames.push(img);
    }
    assert_eq!(matrix_at(&mut ev, 10), Affine::translate((50.0, 50.0)) * Affine::rotate(deg(90.0)));
    assert!(is(&frames[5], 69, 69, BLUE) && is(&frames[5], 31, 31, WHITE));
    let sheet = sheet(&frames, 4, 2);
    save(&sheet, "rot_cw2");
    golden(&sheet, "rot_cw2");
}

#[test]
fn rot_auto_wraps_the_short_way_across_zero() {
    let rot = |a: f64| Affine::translate((50.0, 50.0)) * Affine::rotate(deg(a));
    let d = tween_doc("bar", rot(350.0), rot(10.0), |f| f.motion_tween_rotate = TweenRotate::Auto);
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for f in 0..=10u32 {
        let theta = 350.0 + 2.0 * f64::from(f);
        let img = render(&mut ev, f);
        assert_tip(&img, theta, &format!("auto f{f}"));
        let tip_x = 50.0 + 27.0 * deg(theta).cos();
        assert!(tip_x >= 50.0 + 27.0 * deg(10.0).cos() - 1e-9, "tip never swings left");
        frames.push(img);
    }
    assert!(is(&frames[5], 77, 50, BLUE) && is(&frames[5], 23, 50, WHITE));
    assert!(is(&frames[2], 76, 47, BLUE));
    save(&sheet(&frames, 4, 2), "rot_auto_wrap");
}

#[test]
fn rot_ccw_goes_the_long_way() {
    let rot = |a: f64| Affine::translate((50.0, 50.0)) * Affine::rotate(deg(a));
    let d = tween_doc("bar", rot(350.0), rot(10.0), |f| {
        f.motion_tween_rotate = TweenRotate::CounterClockwise;
        f.motion_tween_rotate_times = Some(0.0);
    });
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for f in 0..=10u32 {
        let theta = 350.0 - 34.0 * f64::from(f);
        let img = render(&mut ev, f);
        assert_tip(&img, theta, &format!("ccw f{f}"));
        frames.push(img);
    }
    assert!(is(&frames[5], 23, 50, BLUE) && is(&frames[5], 77, 50, WHITE));
    assert!(is(&frames[1], 69, 31, BLUE));
    assert!(xform::wrap_angle(xform::decompose(matrix_at(&mut ev, 10)).rotation - deg(10.0)).abs() < 1e-9);
    save(&sheet(&frames, 4, 2), "rot_ccw_long");
}

// ---- eases ----

/// Independent solve of a chained cubic bezier ease: bisection on time, then the value.
fn bez_ease(points: &[[f64; 2]], p: f64) -> f64 {
    let n = (points.len() - 1) / 3;
    let seg = (0..n).find(|i| p <= points[i * 3 + 3][0]).unwrap_or(n - 1);
    let [p0, p1, p2, p3] = [points[seg * 3], points[seg * 3 + 1], points[seg * 3 + 2], points[seg * 3 + 3]];
    let b = |t: f64, k: usize| {
        let u = 1.0 - t;
        u * u * u * p0[k] + 3.0 * u * u * t * p1[k] + 3.0 * u * t * t * p2[k] + t * t * t * p3[k]
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..80 {
        let mid = (lo + hi) / 2.0;
        if b(mid, 0) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    b((lo + hi) / 2.0, 1)
}

fn x_at(d: &Document, f: u32) -> f64 {
    Evaluator::new(d)
        .frame(0, f)
        .nodes
        .iter()
        .find_map(|n| if let anim_display::Node::Group(g) = n { Some(g.transform.translation().x) } else { None })
        .unwrap()
}

fn slide_doc(configure: impl FnOnce(&mut Frame)) -> Document {
    tween_doc("box", Affine::translate((0.0, 20.0)), Affine::translate((100.0, 20.0)), configure)
}

#[test]
fn ease_custom_chain_hits_the_anchor_and_overshoots() {
    let pts = vec![[0.0, 0.0], [0.1, 0.0], [0.2, 1.2], [0.5, 1.2], [0.6, 1.2], [0.9, 1.0], [1.0, 1.0]];
    let d = slide_doc(|f| f.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Custom(pts.clone()) }));
    let mut max = 0.0f64;
    for f in 0..=10u32 {
        let want = 100.0 * bez_ease(&pts, f64::from(f) / 10.0);
        let got = x_at(&d, f);
        assert!((got - want).abs() < 0.05, "f{f}: got {got} want {want}");
        max = max.max(got);
    }
    assert!((x_at(&d, 5) - 120.0).abs() < 0.05, "anchor: {}", x_at(&d, 5));
    assert!(max > 100.0, "overshoot");
    assert!((x_at(&d, 10) - 100.0).abs() < 1e-9 && x_at(&d, 0).abs() < 1e-9);
}

#[test]
fn ease_named_presets_match_the_published_formulas() {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    let bounce_out = |x: f64| {
        let (n, dd) = (7.5625, 2.75);
        if x < 1.0 / dd {
            n * x * x
        } else if x < 2.0 / dd {
            let x = x - 1.5 / dd;
            n * x * x + 0.75
        } else if x < 2.5 / dd {
            let x = x - 2.25 / dd;
            n * x * x + 0.9375
        } else {
            let x = x - 2.625 / dd;
            n * x * x + 0.984_375
        }
    };
    type F = Box<dyn Fn(f64) -> f64>;
    let cases: Vec<(&str, F)> = vec![
        ("quartInOut", Box::new(|p| if p < 0.5 { 8.0 * p.powi(4) } else { 1.0 - (-2.0 * p + 2.0).powi(4) / 2.0 })),
        ("backOut", Box::new(move |p| 1.0 + c3 * (p - 1.0).powi(3) + c1 * (p - 1.0).powi(2))),
        ("bounceOut", Box::new(bounce_out)),
        ("elasticOut", Box::new(|p| (-10.0 * p).exp2() * ((p * 10.0 - 0.75) * (TAU / 3.0)).sin() + 1.0)),
        ("sineInOut", Box::new(|p| -((PI * p).cos() - 1.0) / 2.0)),
        ("cubicIn", Box::new(|p| p * p * p)),
        ("expoOut", Box::new(|p| 1.0 - (-10.0 * p).exp2())),
        (
            "circInOut",
            Box::new(|p| if p < 0.5 { (1.0 - (1.0 - (2.0 * p).powi(2)).sqrt()) / 2.0 } else { ((1.0 - (-2.0 * p + 2.0).powi(2)).sqrt() + 1.0) / 2.0 }),
        ),
    ];
    for (name, f) in &cases {
        let d = slide_doc(|k| k.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Named((*name).into()) }));
        for frame in 0..=10u32 {
            let p = f64::from(frame) / 10.0;
            let want = if frame == 0 {
                0.0
            } else if frame == 10 {
                100.0
            } else {
                100.0 * f(p)
            };
            let got = x_at(&d, frame);
            assert!((got - want).abs() < 1e-6, "{name} f{frame}: got {got} want {want}");
        }
    }
    assert!((x_at(&slide_doc(|k| k.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Named("bounceOut".into()) })), 5) - 76.5625).abs() < 1e-6);
}

#[test]
fn ease_classic_acceleration_blends_linear_and_quadratic() {
    assert!((x_at(&slide_doc(|f| f.acceleration = 50), 5) - 62.5).abs() < 1e-9);
    assert!((x_at(&slide_doc(|f| f.acceleration = -50), 5) - 37.5).abs() < 1e-9);
    assert!((x_at(&slide_doc(|f| f.acceleration = 0), 5) - 50.0).abs() < 1e-9);
    // A custom ease on the keyframe overrides the acceleration.
    let d = slide_doc(|f| {
        f.acceleration = 100;
        f.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Named("linear".into()) });
    });
    assert!((x_at(&d, 5) - 50.0).abs() < 1e-9);
    // Out-of-range acceleration is clamped, not extrapolated.
    assert!((x_at(&slide_doc(|f| f.acceleration = 250), 5) - 75.0).abs() < 1e-9);
}

#[test]
fn ease_targets_are_independent() {
    let d = tween_doc("bar", Affine::translate((0.0, 50.0)), Affine::translate((100.0, 50.0)) * Affine::rotate(deg(90.0)), |f| {
        f.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Named("cubicIn".into()) });
        f.eases.push(TweenEase { target: EaseTarget::Position, curve: EaseCurve::Named("linear".into()) });
        f.acceleration = 100;
    });
    let mut ev = Evaluator::new(&d);
    let m = xform::decompose(matrix_at(&mut ev, 5));
    assert!((m.tx - 50.0).abs() < 1e-9, "position linear: {}", m.tx);
    assert!((m.rotation.to_degrees() - 11.25).abs() < 1e-6, "rotation follows All: {}", m.rotation.to_degrees());
    // Scale has neither entry nor All?  It has All (cubicIn): 1 -> 1 so nothing to see; check colour falls to All too.
    let none = tween_doc("bar", Affine::translate((0.0, 50.0)), Affine::translate((100.0, 50.0)) * Affine::rotate(deg(90.0)), |f| {
        f.acceleration = 100;
        f.eases.push(TweenEase { target: EaseTarget::Position, curve: EaseCurve::Named("linear".into()) });
    });
    let mut ev = Evaluator::new(&none);
    let m = xform::decompose(matrix_at(&mut ev, 5));
    // Rotation has no entry and no All: the classic acceleration (+100 -> p(2-p) = 0.75) applies.
    assert!((m.rotation.to_degrees() - 67.5).abs() < 1e-6, "{}", m.rotation.to_degrees());
}

// ---- keyframes at the edges ----

#[test]
fn keyframe_edges() {
    let mut d = doc(vec![]);
    box_symbol(&mut d);
    let tw = |index, dur, y: f64| {
        let mut f = keyframe(index, dur, vec![instance("box", Affine::translate((0.0, y)))]);
        f.tween_type = TweenType::Classic;
        f
    };
    let at = |x: f64, y: f64| vec![instance("box", Affine::translate((x, y)))];
    d.scenes[0].layers = vec![
        layer("late", vec![tw(0, 9, 10.0), keyframe(9, 1, at(90.0, 10.0))]),
        layer("gap", vec![keyframe(3, 4, at(0.0, 30.0)), keyframe(7, 3, vec![])]),
        layer("dangling", vec![tw(0, 5, 50.0), keyframe(5, 5, vec![])]),
        layer("short", vec![tw(0, 1, 70.0), keyframe(1, 1, at(40.0, 70.0))]),
    ];
    let mut ev = Evaluator::new(&d);
    assert_eq!(ev.scene_frames(0), 10);
    assert_eq!(ev.scene_frames(5), 0);
    assert_eq!(ev.frame(5, 0).nodes.len(), 0);
    let mut frames = Vec::new();
    for f in 0..10u32 {
        let list = ev.frame(0, f);
        let img = draw(&list, ev.images());
        // late: layer index 0.
        let late = list.nodes.iter().find_map(|n| match n {
            anim_display::Node::Group(g) if g.key.and_then(anim_timeline::split_key).map(|k| k.0) == Some(0) => Some(g.transform.translation()),
            _ => None,
        });
        let late = late.unwrap_or_else(|| panic!("late layer missing at f{f}"));
        assert!((late.x - 9.0f64.min(f64::from(f)) * 10.0).abs() < 1e-9 && (late.y - 10.0).abs() < 1e-9, "f{f}: {late:?}");
        let gap = if (3..7).contains(&f) { RED } else { WHITE };
        assert!(is(&img, 5, 35, gap), "gap f{f}: {:?}", pixel(&img, 5, 35));
        let dangling = if f < 5 { RED } else { WHITE };
        assert!(is(&img, 5, 55, dangling), "dangling f{f}");
        match f {
            0 => assert!(is(&img, 5, 75, RED) && is(&img, 45, 75, WHITE)),
            1 => assert!(is(&img, 45, 75, RED) && is(&img, 5, 75, WHITE)),
            _ => assert!(is(&img, 5, 75, WHITE) && is(&img, 45, 75, WHITE)),
        }
        frames.push(img);
    }
    assert!(is(&frames[9], 95, 15, RED) && is(&frames[9], 85, 15, WHITE));
    for f in [10, 11, 1000, u32::MAX] {
        assert!(ev.frame(0, f).nodes.is_empty(), "frame {f} is past every layer");
    }
    save(&sheet(&frames, 5, 2), "keyframe_edges");
}

// ---- colour ----

fn sq_symbol(d: &mut Document) {
    add_symbol(d, "sq", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 40.0, 40.0, RED)])])]);
}

fn coloured(effect: ColorEffect) -> Element {
    let mut i = SymbolInstance::new("sq", Affine::IDENTITY);
    i.set_color_effect(effect);
    Element::SymbolInstance(Box::new(i))
}

fn colour_doc(a: ColorEffect, b: ColorEffect, configure: impl FnOnce(&mut Frame)) -> Document {
    let mut d = doc(vec![]);
    sq_symbol(&mut d);
    let mut start = keyframe(0, 10, vec![coloured(a)]);
    start.tween_type = TweenType::Classic;
    configure(&mut start);
    d.scenes[0].layers = vec![layer("c", vec![start, keyframe(10, 1, vec![coloured(b)])])];
    d
}

#[test]
fn colour_alpha_tween_over_white() {
    let d = colour_doc(ColorEffect::Alpha(1.0), ColorEffect::Alpha(0.0), |_| {});
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for f in 0..=10u32 {
        let img = render(&mut ev, f);
        let g = (25.5 * f64::from(f)).round() as u8;
        assert!(near(&img, 20, 20, [255, g, g], 2), "f{f}: {:?} want 255,{g},{g}", pixel(&img, 20, 20));
        frames.push(img);
    }
    save(&sheet(&frames, 6, 1), "colour_alpha");
}

#[test]
fn colour_tint_and_advanced_tweens() {
    let d = colour_doc(ColorEffect::Tint { color: BLUE, amount: 0.0 }, ColorEffect::Tint { color: BLUE, amount: 1.0 }, |_| {});
    let mut ev = Evaluator::new(&d);
    assert!(near(&render(&mut ev, 5), 20, 20, [128, 0, 128], 2));
    assert!(near(&render(&mut ev, 10), 20, 20, [0, 0, 255], 1));
    assert!(near(&render(&mut ev, 0), 20, 20, [255, 0, 0], 1));
    let a = ColorEffect::Advanced(ColorTransform::IDENTITY);
    let b = ColorEffect::Advanced(ColorTransform { mul: [0.5, 2.0, 1.0, 0.5], add: [0.0, 0.2, 0.0, 0.0] });
    let d = colour_doc(a, b, |_| {});
    let mut ev = Evaluator::new(&d);
    assert!(near(&render(&mut ev, 5), 20, 20, [207, 83, 64], 3), "{:?}", pixel(&render(&mut ev, 5), 20, 20));
}

#[test]
fn colour_ease_is_independent_of_position() {
    let mut d = colour_doc(ColorEffect::Alpha(1.0), ColorEffect::Alpha(0.0), |f| {
        f.eases.push(TweenEase { target: EaseTarget::Color, curve: EaseCurve::Named("cubicIn".into()) });
    });
    // Also move the instance 0 -> 100 along x with the (default, linear) position ease.
    let frames = &mut d.scenes[0].layers[0].frames;
    if let Element::SymbolInstance(i) = &mut frames[0].elements[0] {
        i.matrix = Affine::IDENTITY;
    }
    if let Element::SymbolInstance(i) = &mut frames[1].elements[0] {
        i.matrix = Affine::translate((100.0, 0.0));
    }
    let mut ev = Evaluator::new(&d);
    let img = render(&mut ev, 5);
    // x = 50 (linear), alpha 0.875 -> 255 * 0.125 = 32.
    assert!(near(&img, 70, 20, [255, 32, 32], 2), "{:?}", pixel(&img, 70, 20));
    assert!((matrix_at(&mut ev, 5).translation().x - 50.0).abs() < 1e-9);
}

#[test]
fn colour_alpha_multiplies_through_nested_instances() {
    let mut d = doc(vec![]);
    sq_symbol(&mut d);
    let mut half = SymbolInstance::new("sq", Affine::IDENTITY);
    half.set_color_effect(ColorEffect::Alpha(0.5));
    add_symbol(&mut d, "wrap", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![Element::SymbolInstance(Box::new(half))])])]);
    let wrap = |a: f32| {
        let mut i = SymbolInstance::new("wrap", Affine::IDENTITY);
        i.set_color_effect(ColorEffect::Alpha(a));
        Element::SymbolInstance(Box::new(i))
    };
    let mut start = keyframe(0, 10, vec![wrap(1.0)]);
    start.tween_type = TweenType::Classic;
    d.scenes[0].layers = vec![layer("c", vec![start, keyframe(10, 1, vec![wrap(0.0)])])];
    let mut ev = Evaluator::new(&d);
    assert!(near(&render(&mut ev, 5), 20, 20, [255, 191, 191], 2), "{:?}", pixel(&render(&mut ev, 5), 20, 20));
    assert!(near(&render(&mut ev, 0), 20, 20, [255, 128, 128], 2));
    assert!(near(&render(&mut ev, 10), 20, 20, [255, 255, 255], 1));
}

// ---- onion skins ----

fn onion_doc() -> Document {
    let mut d = doc(vec![]);
    box_symbol(&mut d);
    let mut start = keyframe(0, 7, vec![instance("box", Affine::translate((0.0, 45.0)))]);
    start.tween_type = TweenType::Classic;
    d.scenes[0].layers = vec![layer("b", vec![start, keyframe(7, 1, vec![instance("box", Affine::translate((70.0, 45.0)))])])];
    d
}

#[test]
fn onion_ranges_and_alphas() {
    let d = onion_doc();
    let mut ev = Evaluator::new(&d);
    let x_of = |l: &DisplayList| top_groups(l)[0].transform.translation().x;
    type Row<'a> = (u32, u32, u32, &'a [(f64, f32)]);
    let table: [Row; 8] = [
        (3, 2, 2, &[(10.0, 0.25), (20.0, 0.5), (40.0, 0.5), (50.0, 0.25)]),
        (0, 3, 0, &[]),
        (7, 0, 2, &[]),
        (6, 0, 3, &[(70.0, 0.5)]),
        (9, 3, 0, &[(60.0, 1.0 / 6.0), (70.0, 1.0 / 3.0)]),
        (20, 3, 3, &[]),
        (3, 1, 1, &[(20.0, 0.5), (40.0, 0.5)]),
        (0, 0, 8, &[(10.0, 0.5), (20.0, 0.4375), (30.0, 0.375), (40.0, 0.3125), (50.0, 0.25), (60.0, 0.1875), (70.0, 0.125)]),
    ];
    for (frame, before, after, want) in table {
        let got = ev.onion(0, frame, before, after);
        assert_eq!(got.len(), want.len(), "onion({frame}, {before}, {after})");
        for ((list, alpha), (x, a)) in got.iter().zip(want) {
            assert!((x_of(list) - x).abs() < 1e-9, "onion({frame}, {before}, {after}): x {} want {x}", x_of(list));
            assert!((alpha - a).abs() < 1e-6, "onion({frame}, {before}, {after}): alpha {alpha} want {a}");
        }
    }
}

#[test]
fn onion_composites_with_fading() {
    let d = onion_doc();
    let mut ev = Evaluator::new(&d);
    let mut list = ev.frame(0, 3);
    let mut nodes = Vec::new();
    for (mut skin, alpha) in ev.onion(0, 3, 2, 2) {
        fade(&mut skin, alpha);
        nodes.extend(skin.nodes);
    }
    nodes.extend(list.nodes);
    list.nodes = nodes;
    let img = draw(&list, ev.images());
    assert!(near(&img, 35, 50, [255, 0, 0], 1));
    assert!(near(&img, 25, 50, [255, 128, 128], 2) && near(&img, 45, 50, [255, 128, 128], 2));
    assert!(near(&img, 15, 50, [255, 191, 191], 2) && near(&img, 55, 50, [255, 191, 191], 2));
    assert!(near(&img, 5, 50, [255, 255, 255], 0) && near(&img, 65, 50, [255, 255, 255], 0));
    save(&zoom(&img, 0, 30, 70, 40, 6), "onion");
    golden(&zoom(&img, 0, 30, 70, 40, 3), "onion");
}

// ---- motion guides ----

fn stroked(path: BezPath) -> Element {
    Element::Shape(ShapeElement::new(EdgeShape::from_paths(&[PathInput::new(path, None, Some(StrokeStyle::plain(1.0, Rgba8::BLACK)))])))
}

/// A guide layer holding `path` and a guided layer tweening a `bar` from `from` to `to` (stage points) over
/// `frames` frames, linear.
fn guide_doc(path: BezPath, from: Point, to: Point, frames: u32, orient: bool) -> Document {
    let mut d = doc(vec![]);
    bar_symbol(&mut d);
    let mut start = keyframe(0, frames, vec![instance("bar", Affine::translate(from.to_vec2()))]);
    start.tween_type = TweenType::Classic;
    start.motion_tween_snap = true;
    start.motion_tween_orient_to_path = orient;
    let end = keyframe(frames, 1, vec![instance("bar", Affine::translate(to.to_vec2()))]);
    d.scenes[0].layers = vec![
        layer_kind("guide", LayerKind::Guide, None, vec![keyframe(0, frames + 1, vec![stroked(path)])]),
        layer_kind("mover", LayerKind::Normal, Some(0), vec![start, end]),
    ];
    d
}

fn polyline() -> BezPath {
    let mut p = BezPath::new();
    p.move_to((10.0, 10.0));
    p.line_to((10.0, 30.0));
    p.line_to((90.0, 30.0));
    p
}

/// Bar tip centre for a registration point and screen angle.
fn tip(pos: Point, theta_deg: f64) -> (u32, u32) {
    let (s, c) = deg(theta_deg).sin_cos();
    (px(pos.x + 27.0 * c), px(pos.y + 27.0 * s))
}

#[test]
fn guide_polyline_moves_at_constant_arc_speed() {
    let d = guide_doc(polyline(), Point::new(10.0, 10.0), Point::new(90.0, 30.0), 10, false);
    let mut ev = Evaluator::new(&d);
    let want = [(0, 10.0, 10.0), (1, 10.0, 20.0), (2, 10.0, 30.0), (3, 20.0, 30.0), (5, 40.0, 30.0), (7, 60.0, 30.0), (10, 90.0, 30.0)];
    let mut frames = Vec::new();
    for (f, x, y) in want {
        let t = matrix_at(&mut ev, f).translation();
        assert!((t.x - x).abs() < 1e-6 && (t.y - y).abs() < 1e-6, "f{f}: {t:?} want ({x}, {y})");
        let img = render(&mut ev, f);
        // Orient off: the bar stays horizontal, tip marker at +27 in x.
        if x + 27.0 < 100.0 {
            assert!(is(&img, px(x + 27.0), px(y), BLUE), "f{f}: horizontal tip");
        }
        frames.push(img);
    }
    // The guide itself is invisible.
    let img = render(&mut ev, 8);
    assert!(is(&img, 10, 15, WHITE), "guide stroke must not be drawn: {:?}", pixel(&img, 10, 15));
    save(&sheet(&frames, 4, 2), "guide_polyline");
}

#[test]
fn guide_orient_to_path_aligns_the_baseline() {
    let d = guide_doc(polyline(), Point::new(10.0, 10.0), Point::new(90.0, 30.0), 10, true);
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for (f, x, y, theta) in [(0, 10.0, 10.0, 90.0), (1, 10.0, 20.0, 90.0), (3, 20.0, 30.0, 0.0), (5, 40.0, 30.0, 0.0), (7, 60.0, 30.0, 0.0)] {
        let img = render(&mut ev, f);
        let (tx, ty) = tip(Point::new(x, y), theta);
        assert!(is(&img, tx, ty, BLUE), "f{f}: tip ({tx}, {ty}) is {:?}", pixel(&img, tx, ty));
        frames.push(img);
    }
    assert!(is(&frames[3], 67, 30, BLUE) && is(&frames[3], 40, 57, WHITE));
    assert!(is(&frames[1], 10, 47, BLUE));
    save(&sheet(&frames, 3, 2), "guide_orient");
}

fn circle_path() -> BezPath {
    // Clockwise on screen (y down), starting at (90, 50), four cubics.
    let k = 0.552_284_749_8 * 40.0;
    let mut p = BezPath::new();
    p.move_to((90.0, 50.0));
    p.curve_to((90.0, 50.0 + k), (50.0 + k, 90.0), (50.0, 90.0));
    p.curve_to((50.0 - k, 90.0), (10.0, 50.0 + k), (10.0, 50.0));
    p.curve_to((10.0, 50.0 - k), (50.0 - k, 10.0), (50.0, 10.0));
    p.curve_to((50.0 + k, 10.0), (90.0, 50.0 - k), (90.0, 50.0));
    p
}

#[test]
fn guide_circle_with_orientation() {
    let d = guide_doc(circle_path(), Point::new(90.0, 50.0), Point::new(90.0, 50.0), 12, true);
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for f in 0..=12u32 {
        let phi = TAU * f64::from(f) / 12.0;
        let (want_x, want_y) = (50.0 + 40.0 * phi.cos(), 50.0 + 40.0 * phi.sin());
        let m = matrix_at(&mut ev, f);
        let t = m.translation();
        assert!((t.x - want_x).abs() < 0.3 && (t.y - want_y).abs() < 0.3, "f{f}: {t:?} want ({want_x:.2}, {want_y:.2})");
        // Tangent of a clockwise circle: phi + 90 degrees.
        let theta = (phi + PI / 2.0).to_degrees();
        let rot = xform::decompose(m).rotation;
        assert!(xform::wrap_angle(rot - deg(theta)).abs() < 0.02, "f{f}: rotation {} want {}", rot.to_degrees(), theta % 360.0);
        frames.push(render(&mut ev, f));
    }
    assert!(is(&frames[3], 23, 90, BLUE) && is(&frames[6], 10, 23, BLUE) && is(&frames[9], 77, 10, BLUE));
    save(&sheet(&frames, 5, 2), "guide_circle");
}
