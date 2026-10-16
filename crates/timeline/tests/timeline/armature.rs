//! Armature layers through the evaluator and the renderer.
//!
//! Prediction (100 x 100 stage, y down): a red bar covers x 10..50, y 45..55 and a green bar x 50..90, y 45..55. Bone 0
//! runs from the joint (10, 50) to (50, 50) and moves the red bar; bone 1 runs from (50, 50) to (90, 50), is the
//! child of bone 0 and moves the green bar. A pose that turns bone 0 a quarter turn clockwise swings its tip to
//! (10, 90): the red bar becomes x 5..15, y 50..90 and carries the green bar along to x 5..15, y 90..130. Turning
//! bone 1 by a further quarter turn bends the green bar back to the right of the joint at (10, 90).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::expect_used, clippy::arithmetic_side_effects, clippy::missing_const_for_fn)]

use crate::common;

use anim_core::{Affine, Point, Rgba8, Vec2};
use anim_scene::{Armature, ArmatureBone, ArmatureKind, Frame, Pose, TweenType};
use anim_timeline::Evaluator;
use common::*;
use std::f64::consts::FRAC_PI_2;

fn document() -> anim_scene::Document {
    let mut l = layer("rig", vec![keyframe(0, 10, vec![rect(10.0, 45.0, 40.0, 10.0, RED), rect(50.0, 45.0, 40.0, 10.0, GREEN)])]);
    let mut a = Armature::new("rig", ArmatureKind::Symbol);
    let mut b0 = ArmatureBone::between("a", None, Point::new(10.0, 50.0), Point::new(50.0, 50.0));
    b0.element = Some(0);
    a.push(b0).unwrap();
    let mut b1 = ArmatureBone::between("b", Some(0), Point::new(50.0, 50.0), Point::new(90.0, 50.0));
    b1.element = Some(1);
    a.push(b1).unwrap();
    l.armature = Some(a);
    let mut doc = doc(vec![l]);
    let layer = &mut doc.scenes[0].layers[0];
    layer.frames[0].duration = 5;
    layer.frames[0].pose = Some(Pose::rest(2));
    layer.frames[0].tween_type = TweenType::Classic;
    let mut posed = Frame::new(5, 5, Vec::new());
    let mut pose = Pose::rest(2);
    pose.set_angle(0, FRAC_PI_2);
    posed.pose = Some(pose);
    layer.frames.push(posed);
    doc
}

fn render_at(doc: &anim_scene::Document, frame: u32) -> anim_display::Image {
    let mut ev = Evaluator::new(doc);
    let list = ev.frame(0, frame);
    draw(&list, ev.images())
}

#[test]
fn the_rest_pose_draws_the_artwork_as_stored() {
    let doc = document();
    let image = render_at(&doc, 0);
    assert!(is(&image, 30, 50, RED) && is(&image, 70, 50, GREEN));
    assert!(!is(&image, 10, 70, RED));
}

#[test]
fn a_pose_swings_the_bars_with_their_bones() {
    let doc = document();
    let image = render_at(&doc, 5);
    assert!(is(&image, 10, 70, RED), "the red bar hangs down from the joint");
    assert!(is(&image, 10, 95, GREEN), "the green bar follows it");
    assert!(!is(&image, 30, 50, RED) && !is(&image, 70, 50, GREEN), "nothing is left in the rest position");
    assert!(is(&image, 14, 60, RED) && !is(&image, 16, 60, RED), "the bar is 10 px wide, centred on x = 10");
}

#[test]
fn the_pose_tweens_between_keyframes() {
    let doc = document();
    // Frame 2 of 5, linear: 40 per cent of the quarter turn = 36 degrees about (10, 50).
    let image = render_at(&doc, 2);
    let angle = FRAC_PI_2 * 0.4;
    let probe = Affine::rotate_about(angle, Point::new(10.0, 50.0)) * Point::new(40.0, 50.0);
    assert!(is(&image, px(probe.x), px(probe.y), RED), "{probe:?}");
}

#[test]
fn a_child_bone_bends_relative_to_its_parent() {
    let mut doc = document();
    let layer = &mut doc.scenes[0].layers[0];
    layer.frames[1].pose.as_mut().unwrap().set_angle(1, FRAC_PI_2);
    let image = render_at(&doc, 5);
    // The joint is at (10, 90); the green bar now points to the left (a further clockwise quarter turn from "down"):
    // x -30..10, y 85..95.
    assert!(is(&image, 10, 70, RED));
    assert!(is(&image, 3, 90, GREEN), "{:?}", pixel(&image, 3, 90));
    assert!(!is(&image, 10, 100 - 1, GREEN), "it no longer hangs down");
}

#[test]
fn the_armature_offset_moves_the_whole_rig() {
    let mut doc = document();
    let layer = &mut doc.scenes[0].layers[0];
    layer.frames[1].pose = Some(Pose { offset: Vec2::new(30.0, -20.0), angles: vec![0.0, 0.0] });
    let image = render_at(&doc, 5);
    assert!(is(&image, 60, 30, RED) && is(&image, 90, 30, GREEN), "the rig moved right by 30 and up by 20");
    assert!(!is(&image, 30, 50, RED));
}

#[test]
fn elements_without_a_bone_stay_put_and_other_layers_are_unaffected() {
    let mut doc = document();
    let layer = &mut doc.scenes[0].layers[0];
    layer.frames[0].elements.push(rect(0.0, 0.0, 10.0, 10.0, BLUE));
    let other = layer_with_blue_dot();
    doc.scenes[0].layers.push(other);
    let image = render_at(&doc, 5);
    assert!(is(&image, 5, 5, BLUE), "an unbound element is not moved");
    assert!(is(&image, 95, 5, Rgba8::opaque(1, 2, 255)));
}

fn layer_with_blue_dot() -> anim_scene::Layer {
    layer("dot", vec![keyframe(0, 10, vec![rect(90.0, 0.0, 10.0, 10.0, Rgba8::opaque(1, 2, 255))])])
}
