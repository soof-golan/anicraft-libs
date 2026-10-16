//! Classic, shape and motion-object tweens, and symbol playback, on hand-built documents.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::suboptimal_flops,
    clippy::manual_midpoint
)]

use crate::common;

use anim_core::{Affine, kurbo::Shape as _};
use anim_display::Node;
use anim_scene::{EaseCurve, EaseTarget, Element, Frame, LoopMode, SymbolInstance, SymbolKind, TweenEase, TweenRotate, TweenType};
use anim_timeline::{EvalOptions, Evaluator, xform};
use common::*;

/// A document with a red 10x10 graphic symbol "box" and a layer tweening an instance from `a` to `b` over
/// frames 0..10 (the end keyframe is at 10).
fn tween_doc(a: Affine, b: Affine, configure: impl FnOnce(&mut Frame)) -> anim_scene::Document {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "box", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED)])])]);
    let mut start = keyframe(0, 10, vec![instance("box", a)]);
    start.tween_type = TweenType::Classic;
    configure(&mut start);
    d.scenes[0].layers = vec![layer("t", vec![start, keyframe(10, 1, vec![instance("box", b)])])];
    d
}

fn first_transform(ev: &mut Evaluator<'_>, frame: u32) -> Affine {
    let list = ev.frame(0, frame);
    top_groups(&list)[0].transform
}

#[test]
fn created_motion_tween_plays_back_its_keys() {
    use anim_scene::motion::{self, MotionProp};
    let mut d = tween_doc(Affine::translate((10.0, 20.0)), Affine::IDENTITY, |_| {});
    // One keyframe spanning 11 frames, no end keyframe: a motion tween span.
    d.scenes[0].layers[0].frames = vec![keyframe(0, 11, vec![instance("box", Affine::translate((10.0, 20.0)))])];
    let id = anim_scene::TimelineId::Scene(0);
    motion::create_motion_tween(&mut d, &id, 0, 0).unwrap();
    motion::set_motion_key(&mut d, &id, 0, 0, MotionProp::X, 10, 110.0).unwrap();
    motion::set_motion_key(&mut d, &id, 0, 0, MotionProp::Y, 5, 70.0).unwrap();
    motion::set_motion_key(&mut d, &id, 0, 0, MotionProp::Alpha, 10, 0.0).unwrap();
    let mut ev = Evaluator::new(&d);
    assert!((first_transform(&mut ev, 0).translation().x - 10.0).abs() < 1e-9);
    assert!((first_transform(&mut ev, 5).translation().x - 60.0).abs() < 1e-9);
    assert!((first_transform(&mut ev, 5).translation().y - 70.0).abs() < 1e-9);
    assert!((first_transform(&mut ev, 10).translation().x - 110.0).abs() < 1e-9);
    assert!((first_transform(&mut ev, 10).translation().y - 70.0).abs() < 1e-9, "the last key holds");
}

#[test]
fn linear_position_tween() {
    let d = tween_doc(Affine::translate((0.0, 20.0)), Affine::translate((100.0, 20.0)), |_| {});
    let mut ev = Evaluator::new(&d);
    for frame in [0u32, 1, 5, 9, 10] {
        let t = first_transform(&mut ev, frame).translation();
        assert!((t.x - f64::from(frame) * 10.0).abs() < 1e-9, "frame {frame}: {t:?}");
        assert!((t.y - 20.0).abs() < 1e-9);
    }
    // Rendered: the box moves right.
    let img = draw(&ev.frame(0, 5), ev.images());
    assert!(is(&img, 55, 25, RED));
    assert!(is(&img, 5, 25, WHITE));
}

#[test]
fn tween_without_a_following_keyframe_holds() {
    let mut d = tween_doc(Affine::IDENTITY, Affine::translate((100.0, 0.0)), |_| {});
    d.scenes[0].layers[0].frames.truncate(1);
    let mut ev = Evaluator::new(&d);
    assert_eq!(first_transform(&mut ev, 5), Affine::IDENTITY);
}

#[test]
fn classic_acceleration_eases() {
    let d = tween_doc(Affine::IDENTITY, Affine::translate((100.0, 0.0)), |f| f.acceleration = 100);
    let mut ev = Evaluator::new(&d);
    // Ease out: p' = p + p(1-p), so at p = 0.5 the box has covered 75 %.
    let x = first_transform(&mut ev, 5).translation().x;
    assert!((x - 75.0).abs() < 1e-9, "{x}");
    let d = tween_doc(Affine::IDENTITY, Affine::translate((100.0, 0.0)), |f| f.acceleration = -100);
    let mut ev = Evaluator::new(&d);
    let x = first_transform(&mut ev, 5).translation().x;
    assert!((x - 25.0).abs() < 1e-9, "{x}");
}

#[test]
fn custom_bezier_ease_is_used() {
    let d = tween_doc(Affine::IDENTITY, Affine::translate((100.0, 0.0)), |f| {
        f.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Custom(vec![[0.0, 0.0], [0.443, 0.1337], [0.7498, 0.4956], [1.0, 1.0]]) });
    });
    let mut ev = Evaluator::new(&d);
    let early = first_transform(&mut ev, 2).translation().x;
    let late = first_transform(&mut ev, 8).translation().x;
    assert!(early < 20.0, "slow start, got {early}");
    assert!(late > early + 30.0 && late < 100.0, "{early} {late}");
    assert!((first_transform(&mut ev, 10).translation().x - 100.0).abs() < 1e-9);
}

#[test]
fn per_property_ease_targets() {
    let d = tween_doc(Affine::IDENTITY, Affine::translate((100.0, 0.0)) * Affine::scale(3.0), |f| {
        f.eases.push(TweenEase { target: EaseTarget::Position, curve: EaseCurve::Named("cubicIn".into()) });
        f.eases.push(TweenEase { target: EaseTarget::Scale, curve: EaseCurve::Named("linear".into()) });
    });
    let mut ev = Evaluator::new(&d);
    let m = first_transform(&mut ev, 5);
    assert!((m.translation().x - 12.5).abs() < 1e-9, "position follows cubicIn");
    let scale = xform::decompose(m).sx;
    assert!((scale - 2.0).abs() < 1e-9, "scale is linear: {scale}");
}

#[test]
fn rotation_direction_and_turns() {
    let end = Affine::rotate(90f64.to_radians());
    let cw = tween_doc(Affine::IDENTITY, end, |f| {
        f.motion_tween_rotate = TweenRotate::Clockwise;
        f.motion_tween_rotate_times = Some(2.0);
    });
    let mut ev = Evaluator::new(&cw);
    let r = xform::decompose(first_transform(&mut ev, 5)).rotation;
    // Halfway through (90 degrees + two turns) is 2.25 turns, which is a quarter turn on screen.
    let expected = (90f64.to_radians() + 4.0 * std::f64::consts::PI) * 0.5;
    assert!(xform::wrap_angle(r - expected).abs() < 1e-6, "{r}");
    let quarter = xform::decompose(first_transform(&mut ev, 1)).rotation;
    let expected = (90f64.to_radians() + 4.0 * std::f64::consts::PI) * 0.1;
    assert!(xform::wrap_angle(quarter - expected).abs() < 1e-6);
    let ccw = tween_doc(Affine::IDENTITY, end, |f| {
        f.motion_tween_rotate = TweenRotate::CounterClockwise;
        f.motion_tween_rotate_times = Some(0.0);
    });
    let mut ev = Evaluator::new(&ccw);
    let r = xform::decompose(first_transform(&mut ev, 5)).rotation;
    assert!(r < 0.0, "counter-clockwise goes negative first: {r}");
    let none = tween_doc(Affine::IDENTITY, end, |f| f.motion_tween_rotate = TweenRotate::None);
    let mut ev = Evaluator::new(&none);
    assert!(xform::decompose(first_transform(&mut ev, 5)).rotation.abs() < 1e-9);
}

#[test]
fn scale_flag_holds_the_scale() {
    let d = tween_doc(Affine::IDENTITY, Affine::scale(4.0), |f| f.motion_tween_scale = false);
    let mut ev = Evaluator::new(&d);
    assert!((xform::decompose(first_transform(&mut ev, 5)).sx - 1.0).abs() < 1e-9);
}

#[test]
fn colour_transform_tweens() {
    let mut d = tween_doc(Affine::IDENTITY, Affine::IDENTITY, |_| {});
    let frames = &mut d.scenes[0].layers[0].frames;
    for (i, alpha) in [(0, 1.0), (1, 0.0)] {
        if let Element::SymbolInstance(inst) = &mut frames[i].elements[0] {
            inst.set_color_effect(anim_scene::ColorEffect::Alpha(alpha));
        }
    }
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 4);
    assert!((top_groups(&list)[0].color.mul[3] - 0.6).abs() < 1e-6);
}

#[test]
fn tweens_only_pair_the_same_library_item() {
    let mut d = tween_doc(Affine::IDENTITY, Affine::translate((100.0, 0.0)), |_| {});
    add_symbol(&mut d, "other", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, BLUE)])])]);
    d.scenes[0].layers[0].frames[1].elements = vec![instance("other", Affine::translate((100.0, 0.0)))];
    let mut ev = Evaluator::new(&d);
    assert_eq!(first_transform(&mut ev, 5), Affine::IDENTITY, "different items: held");
}

#[test]
fn shape_tween_morphs_between_keyframes() {
    let mut d = doc(vec![]);
    let mut start = keyframe(0, 10, vec![rect(0.0, 0.0, 10.0, 10.0, RED)]);
    start.tween_type = TweenType::Shape;
    d.scenes[0].layers = vec![layer("s", vec![start, keyframe(10, 1, vec![rect(0.0, 0.0, 50.0, 50.0, RED)])])];
    let mut ev = Evaluator::new(&d);
    let area_at = |ev: &mut Evaluator<'_>, frame: u32| -> f64 {
        let list = ev.frame(0, frame);
        let mut area = 0.0;
        list.visit(&mut |n| {
            if let Node::Fill(f) = n {
                area += f.path.area().abs();
            }
        });
        area
    };
    let a0 = area_at(&mut ev, 0);
    let a5 = area_at(&mut ev, 5);
    let a10 = area_at(&mut ev, 10);
    assert!((a0 - 100.0).abs() < 1e-6 && (a10 - 2500.0).abs() < 1e-6, "{a0} {a10}");
    assert!(a5 > a0 && a5 < a10, "{a5}");
    // Rendered: halfway the square covers (0..30).
    let img = draw(&ev.frame(0, 5), ev.images());
    assert!(is(&img, 25, 25, RED));
    assert!(is(&img, 45, 45, WHITE));
}

#[test]
fn shape_tween_with_a_missing_partner_holds() {
    let mut d = doc(vec![]);
    let mut start = keyframe(0, 10, vec![rect(0.0, 0.0, 10.0, 10.0, RED)]);
    start.tween_type = TweenType::Shape;
    d.scenes[0].layers = vec![layer("s", vec![start, keyframe(10, 1, vec![])])];
    let mut ev = Evaluator::new(&d);
    let img = draw(&ev.frame(0, 5), ev.images());
    assert!(is(&img, 5, 5, RED));
}

#[test]
fn motion_object_tween_reads_what_it_can() {
    let mut d = doc(vec![]);
    let mut start = keyframe(0, 11, vec![rect(0.0, 0.0, 10.0, 10.0, RED)]);
    start.tween_type = TweenType::MotionObject;
    start.motion_object_xml = Some(
        r#"<AnimationCore duration="100"><Property id="Motion_X"><Keyframe timevalue="0" value="0"/><Keyframe timevalue="100" value="80"/></Property></AnimationCore>"#
            .to_owned(),
    );
    d.scenes[0].layers = vec![layer("m", vec![start])];
    let mut ev = Evaluator::new(&d);
    assert!((first_transform(&mut ev, 5).translation().x - 40.0).abs() < 1e-9);
    assert!((first_transform(&mut ev, 10).translation().x - 80.0).abs() < 1e-9);
    // Unreadable XML holds the first keyframe.
    d.scenes[0].layers[0].frames[0].motion_object_xml = Some("garbage".to_owned());
    let mut ev = Evaluator::new(&d);
    assert_eq!(first_transform(&mut ev, 5), Affine::IDENTITY);
}

/// A graphic symbol of three frames, each a 10x10 box one step further right: box at x = 20 * frame.
fn stepping_doc(kind: SymbolKind, configure: impl FnOnce(&mut SymbolInstance)) -> anim_scene::Document {
    let mut d = doc(vec![]);
    let frames = (0..3).map(|i| keyframe(i, 1, vec![rect(f64::from(i) * 20.0, 0.0, 10.0, 10.0, RED)])).collect();
    add_symbol(&mut d, "steps", kind, vec![layer("l", frames)]);
    let mut inst = SymbolInstance::new("steps", Affine::IDENTITY);
    inst.symbol_type = Some(kind);
    configure(&mut inst);
    d.scenes[0].layers = vec![layer("p", vec![keyframe(0, 20, vec![Element::SymbolInstance(Box::new(inst))])])];
    d
}

/// The x of the red box a frame shows (the box is at x = 20 * symbol frame).
fn box_slot(ev: &mut Evaluator<'_>, frame: u32) -> Option<u32> {
    let img = draw(&ev.frame(0, frame), ev.images());
    (0..3u32).find(|i| is(&img, i * 20 + 5, 5, RED))
}

#[test]
fn graphic_loop_is_locked_to_the_parent() {
    let d = stepping_doc(SymbolKind::Graphic, |i| {
        i.loop_mode = Some(LoopMode::Loop);
        i.first_frame = 1;
    });
    let mut ev = Evaluator::new(&d);
    let slots: Vec<_> = (0..6).map(|f| box_slot(&mut ev, f).unwrap()).collect();
    assert_eq!(slots, vec![1, 2, 0, 1, 2, 0]);
}

#[test]
fn graphic_play_once_holds_the_last_frame() {
    let d = stepping_doc(SymbolKind::Graphic, |i| i.loop_mode = Some(LoopMode::PlayOnce));
    let mut ev = Evaluator::new(&d);
    let slots: Vec<_> = (0..5).map(|f| box_slot(&mut ev, f).unwrap()).collect();
    assert_eq!(slots, vec![0, 1, 2, 2, 2]);
}

#[test]
fn graphic_single_frame_shows_first_frame() {
    let d = stepping_doc(SymbolKind::Graphic, |i| {
        i.loop_mode = Some(LoopMode::SingleFrame);
        i.first_frame = 2;
    });
    let mut ev = Evaluator::new(&d);
    assert!((0..5).all(|f| box_slot(&mut ev, f) == Some(2)));
    // A first frame beyond the end clamps.
    let d = stepping_doc(SymbolKind::Graphic, |i| {
        i.loop_mode = Some(LoopMode::SingleFrame);
        i.first_frame = 99;
    });
    let mut ev = Evaluator::new(&d);
    assert_eq!(box_slot(&mut ev, 0), Some(2));
}

#[test]
fn graphic_playback_restarts_at_each_keyframe() {
    let mut d = stepping_doc(SymbolKind::Graphic, |_| {});
    // Second keyframe at frame 4 with the same instance: playback counts from there.
    let inst = d.scenes[0].layers[0].frames[0].elements.clone();
    d.scenes[0].layers[0].frames[0].duration = 4;
    d.scenes[0].layers[0].frames.push(keyframe(4, 10, inst));
    let mut ev = Evaluator::new(&d);
    let slots: Vec<_> = (3..7).map(|f| box_slot(&mut ev, f).unwrap()).collect();
    assert_eq!(slots, vec![0, 0, 1, 2], "frame 3 is the fourth frame of the first span (3 % 3), then frame 4 restarts");
}

#[test]
fn movie_clips_show_first_frame_in_authoring() {
    let d = stepping_doc(SymbolKind::MovieClip, |i| i.first_frame = 1);
    let mut ev = Evaluator::new(&d);
    assert!((0..6).all(|f| box_slot(&mut ev, f) == Some(1)));
}

#[test]
fn movie_clips_can_advance_with_the_global_frame() {
    let d = stepping_doc(SymbolKind::MovieClip, |_| {});
    let mut ev = Evaluator::new(&d).with_options(EvalOptions { advance_movie_clips: true, ..EvalOptions::default() });
    let slots: Vec<_> = (0..6).map(|f| box_slot(&mut ev, f).unwrap()).collect();
    assert_eq!(slots, vec![0, 1, 2, 0, 1, 2]);
}

#[test]
fn buttons_show_the_up_frame() {
    let d = stepping_doc(SymbolKind::Button, |_| {});
    let mut ev = Evaluator::new(&d);
    assert_eq!(box_slot(&mut ev, 3), Some(0), "no _up label: frame 0");
    let mut d = stepping_doc(SymbolKind::Button, |_| {});
    d.library.find_symbol_mut("steps").unwrap().timeline.layers[0].frames[2].label = Some("_up".to_owned());
    let mut ev = Evaluator::new(&d);
    assert_eq!(box_slot(&mut ev, 3), Some(2), "the _up label wins");
}

#[test]
fn symbol_cycles_are_cut() {
    let mut d = doc(vec![]);
    add_symbol(
        &mut d,
        "a",
        SymbolKind::Graphic,
        vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED), instance("b", Affine::IDENTITY)])])],
    );
    add_symbol(
        &mut d,
        "b",
        SymbolKind::Graphic,
        vec![layer("l", vec![keyframe(0, 1, vec![rect(20.0, 0.0, 10.0, 10.0, BLUE), instance("a", Affine::IDENTITY)])])],
    );
    add_symbol(
        &mut d,
        "self",
        SymbolKind::Graphic,
        vec![layer("l", vec![keyframe(0, 1, vec![rect(40.0, 0.0, 10.0, 10.0, GREEN), instance("self", Affine::IDENTITY)])])],
    );
    d.scenes[0].layers = vec![layer("p", vec![keyframe(0, 1, vec![instance("a", Affine::IDENTITY), instance("self", Affine::IDENTITY)])])];
    let mut ev = Evaluator::new(&d);
    let img = draw(&ev.frame(0, 0), ev.images());
    assert!(is(&img, 5, 5, RED) && is(&img, 25, 5, BLUE) && is(&img, 45, 5, GREEN));
}

#[test]
fn deep_symbol_chains_hit_the_limit_without_overflow() {
    let mut d = doc(vec![]);
    for i in 0..60 {
        let mut els = vec![rect(f64::from(i % 10) * 3.0, f64::from(i / 10) * 3.0, 2.0, 2.0, RED)];
        if i < 59 {
            els.push(instance(&format!("s{}", i + 1), Affine::IDENTITY));
        }
        add_symbol(&mut d, &format!("s{i}"), SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, els)])]);
    }
    d.scenes[0].layers = vec![layer("p", vec![keyframe(0, 1, vec![instance("s0", Affine::IDENTITY)])])];
    let mut ev = Evaluator::new(&d);
    let mut count = 0;
    ev.frame(0, 0).visit(&mut |n| count += usize::from(matches!(n, Node::Fill(_))));
    assert_eq!(count, 16, "default depth limit 16");
    ev.set_options(EvalOptions { max_symbol_depth: 64, ..EvalOptions::default() });
    let mut count = 0;
    ev.frame(0, 0).visit(&mut |n| count += usize::from(matches!(n, Node::Fill(_))));
    assert_eq!(count, 60);
}

#[test]
fn missing_symbols_draw_nothing() {
    let d = doc(vec![layer("p", vec![keyframe(0, 1, vec![instance("ghost", Affine::IDENTITY), rect(0.0, 0.0, 10.0, 10.0, RED)])])]);
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    let img = draw(&list, ev.images());
    assert!(is(&img, 5, 5, RED));
}

#[test]
fn nested_symbol_matrices_compose() {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "inner", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED)])])]);
    add_symbol(&mut d, "outer", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![instance("inner", Affine::translate((20.0, 0.0)))])])]);
    d.scenes[0].layers = vec![layer("p", vec![keyframe(0, 1, vec![instance("outer", Affine::translate((0.0, 30.0)) * Affine::scale(2.0))])])];
    let mut ev = Evaluator::new(&d);
    let img = draw(&ev.frame(0, 0), ev.images());
    assert!(is(&img, 50, 40, RED), "inner box at x 40..60, y 30..50");
    assert!(is(&img, 10, 40, WHITE));
}
