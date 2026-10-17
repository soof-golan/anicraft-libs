//! Predictions first.
//!
//! Buttons. The symbol `btn` has two layers. Layer `art` holds a red square (0,0)-(20,20) on Up (frame 1), a green one
//! on Over (frame 2) and nothing of its own on Down (so Down shows what Over shows). Layer `hit` holds a blue 40x40
//! square on frame 4 only. Two instances sit on the stage at (10,10) and (60,10).
//!
//! - Drawn plainly, both show red.
//! - With `button_state` naming the first instance and Over, the first shows green and the second stays red; with Down
//!   the first still shows green (the fallback); with Up it is red again.
//! - `frame_of_button(Hit)` draws the blue square only: the art layers fall back to Up, so the hit area also holds the
//!   Up square. A button with just an Up keyframe shows it in every state.
//!
//! 9-slice. The symbol `panel` is 100x100: a red field, a blue 20x20 square in its top-left corner and a green strip on
//! its right edge from y 40 to 60 that is 20 wide. Guides at 20..80 both ways. Scaled by (2, 1) on a 300x150 stage:
//!
//! - the blue square stays 20 wide: (10,10) is blue, (30,10) is red;
//! - the green strip keeps its 20 pixels against the new right edge at x = 200: (190,50) is green, (170,50) is red;
//! - drawn without guides the blue square would be 40 wide and (30,10) would be blue.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::float_cmp, clippy::assert_is_empty)]

use anim_core::{Affine, Rgba8};
use anim_scene::{ButtonState, Document, SymbolKind};
use anim_timeline::{EvalOptions, Evaluator, element_key};

use crate::common::{BLUE, GREEN, RED, add_symbol, draw, instance, is, keyframe, layer, rect};

fn button_doc() -> Document {
    let mut d = crate::common::doc(vec![layer(
        "stage",
        vec![keyframe(0, 1, vec![instance("btn", Affine::translate((10.0, 10.0))), instance("btn", Affine::translate((60.0, 10.0)))])],
    )]);
    let art = layer("art", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 20.0, 20.0, RED)]), keyframe(1, 1, vec![rect(0.0, 0.0, 20.0, 20.0, GREEN)])]);
    let hit = layer("hit", vec![keyframe(3, 1, vec![rect(0.0, 0.0, 40.0, 40.0, BLUE)])]);
    add_symbol(&mut d, "btn", SymbolKind::Button, vec![hit, art]);
    d
}

fn shows(d: &Document, options: EvalOptions, at: (u32, u32), colour: Rgba8) -> bool {
    let mut ev = Evaluator::new(d).with_options(options);
    let list = ev.frame(0, 0);
    is(&draw(&list, ev.images()), at.0, at.1, colour)
}

fn state_of(instance: usize, state: ButtonState) -> EvalOptions {
    EvalOptions { button_state: Some((element_key(0, instance), state)), ..EvalOptions::default() }
}

#[test]
fn buttons_show_up_unless_an_instance_is_given_a_state() {
    let d = button_doc();
    assert!(shows(&d, EvalOptions::default(), (15, 15), RED));
    assert!(shows(&d, EvalOptions::default(), (65, 15), RED));
}

#[test]
fn the_named_instance_shows_its_state_and_the_others_stay_up() {
    let d = button_doc();
    assert!(shows(&d, state_of(0, ButtonState::Over), (15, 15), GREEN));
    assert!(shows(&d, state_of(0, ButtonState::Over), (65, 15), RED), "the other instance is untouched");
    assert!(shows(&d, state_of(1, ButtonState::Over), (65, 15), GREEN));
    assert!(shows(&d, state_of(1, ButtonState::Over), (15, 15), RED));
}

#[test]
fn all_buttons_puts_every_instance_in_one_state_and_one_instance_overrides_it() {
    let d = button_doc();
    let all = EvalOptions { all_buttons: Some(ButtonState::Over), ..EvalOptions::default() };
    assert!(shows(&d, all, (15, 15), GREEN) && shows(&d, all, (65, 15), GREEN));
    let mixed = EvalOptions { all_buttons: Some(ButtonState::Over), button_state: Some((element_key(0, 1), ButtonState::Up)), ..EvalOptions::default() };
    assert!(shows(&d, mixed, (15, 15), GREEN), "the rest follow all_buttons");
    assert!(shows(&d, mixed, (65, 15), RED), "the named instance wins");
}

#[test]
fn down_without_its_own_keyframe_shows_over_and_up_returns_to_red() {
    let d = button_doc();
    assert!(shows(&d, state_of(0, ButtonState::Down), (15, 15), GREEN));
    assert!(shows(&d, state_of(0, ButtonState::Up), (15, 15), RED));
}

#[test]
fn the_hit_frame_is_drawn_for_hit_testing_only() {
    let d = button_doc();
    let mut ev = Evaluator::new(&d);
    let hit = ev.frame_of_button("btn", ButtonState::Hit);
    let image = draw(&hit, ev.images());
    // The 40x40 blue hit shape, with the Up art (red, 20x20) under it where it falls back.
    assert!(is(&image, 30, 30, BLUE), "the hit shape covers the corner");
    assert!(is(&image, 5, 5, BLUE) || is(&image, 5, 5, RED), "the Up art is drawn too, under the hit shape");
    // The plain scene never draws the hit shape.
    assert!(!shows(&d, EvalOptions::default(), (40, 40), BLUE));
    assert!(ev.frame_of_button("missing", ButtonState::Up).nodes.is_empty());
}

#[test]
fn a_button_with_only_an_up_keyframe_looks_the_same_in_every_state() {
    let mut d = crate::common::doc(vec![layer("stage", vec![keyframe(0, 1, vec![instance("only", Affine::IDENTITY)])])]);
    add_symbol(&mut d, "only", SymbolKind::Button, vec![layer("art", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 20.0, 20.0, RED)])])]);
    for state in ButtonState::ALL {
        assert!(shows(&d, state_of(0, state), (10, 10), RED), "{state:?}");
    }
}

#[test]
fn a_blank_keyframe_on_a_state_shows_nothing_there() {
    let mut d = crate::common::doc(vec![layer("stage", vec![keyframe(0, 1, vec![instance("b", Affine::IDENTITY)])])]);
    let art = layer("art", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 20.0, 20.0, RED)]), keyframe(1, 3, vec![])]);
    add_symbol(&mut d, "b", SymbolKind::Button, vec![art]);
    assert!(shows(&d, state_of(0, ButtonState::Over), (10, 10), Rgba8::WHITE), "Over was blanked on purpose");
    assert!(shows(&d, state_of(0, ButtonState::Up), (10, 10), RED));
}

fn panel_doc(grid: bool) -> Document {
    let mut d = crate::common::doc(vec![layer("stage", vec![keyframe(0, 1, vec![instance("panel", Affine::scale_non_uniform(2.0, 1.0))])])]);
    d.width = 300.0;
    d.height = 150.0;
    let art =
        layer("art", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 100.0, RED), rect(0.0, 0.0, 20.0, 20.0, BLUE), rect(80.0, 40.0, 20.0, 20.0, GREEN)])]);
    add_symbol(&mut d, "panel", SymbolKind::MovieClip, vec![art]);
    if grid {
        anim_scene::symbols::set_scale9_grid(&mut d, "panel", Some(anim_core::Rect::new(20.0, 20.0, 80.0, 80.0))).unwrap();
    }
    d
}

fn paint(d: &Document) -> anim_display::Image {
    let mut ev = Evaluator::new(d);
    let list = ev.frame(0, 0);
    draw(&list, ev.images())
}

#[test]
fn guides_keep_the_corners_and_stretch_the_edges() {
    let image = paint(&panel_doc(true));
    crate::common::save(&image, "nine-slice-scaled");
    assert!(is(&image, 10, 10, BLUE), "the corner is where it was");
    assert!(is(&image, 30, 10, RED), "and it is still 20 wide");
    assert!(is(&image, 190, 50, GREEN), "the right strip keeps its width against the new edge");
    assert!(is(&image, 170, 50, RED));
    assert!(is(&image, 100, 50, RED), "the middle stretches");
    assert!(is(&image, 199, 99, RED));
    assert!(is(&image, 205, 50, Rgba8::WHITE), "nothing beyond the scaled edge at x = 200");
}

#[test]
fn without_guides_everything_scales() {
    let image = paint(&panel_doc(false));
    assert!(is(&image, 30, 10, BLUE), "the blue square is 40 wide");
    assert!(is(&image, 190, 50, GREEN));
    assert!(is(&image, 165, 50, GREEN), "and the strip 40 wide");
}

#[test]
fn the_seams_between_the_pieces_are_closed() {
    let image = paint(&panel_doc(true));
    // The red middle meets the red edges along x = 20 and x = 180 and y = 20 and y = 80; no background shows through.
    for x in [19_u32, 20, 21, 179, 180, 181] {
        assert!(is(&image, x, 30, RED), "x = {x}");
    }
    for y in [19_u32, 20, 21, 79, 80, 81] {
        assert!(is(&image, 100, y, RED), "y = {y}");
    }
}

#[test]
fn an_unscaled_instance_with_guides_is_drawn_plainly() {
    let mut d = panel_doc(true);
    d.scenes[0].layers[0].frames[0].elements = vec![instance("panel", Affine::translate((5.0, 5.0)))];
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    let group = crate::common::top_groups(&list).into_iter().next().unwrap();
    assert!(group.children.iter().all(|c| !matches!(c, anim_display::Node::Group(g) if g.mask.is_some())), "no pieces for a unit scale");
}

#[test]
fn a_scaled_instance_has_nine_masked_pieces_and_keeps_its_key() {
    let d = panel_doc(true);
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    let group = crate::common::top_groups(&list).into_iter().next().unwrap();
    assert_eq!(group.key, Some(element_key(0, 0)), "picking still finds the instance");
    assert_eq!(group.children.len(), 9);
    assert!(group.children.iter().all(|c| matches!(c, anim_display::Node::Group(g) if g.mask.is_some())));
}

#[test]
fn guides_work_in_a_nested_symbol_and_with_rotation() {
    let mut d = panel_doc(true);
    // A quarter turn of the same scale: the blue corner is still 20 square, now at the top right.
    d.scenes[0].layers[0].frames[0].elements =
        vec![instance("panel", Affine::translate((150.0, 0.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale_non_uniform(2.0, 1.0))];
    let image = paint(&d);
    crate::common::save(&image, "nine-slice-rotated");
    // x' = 150 - y, y' = x for the rotated matrix; the corner (10,10) of the symbol lands at (140,10).
    assert!(is(&image, 140, 10, BLUE), "the corner follows the turn");
    assert!(is(&image, 140, 30, BLUE) || is(&image, 140, 30, RED));
    assert!(is(&image, 140, 25, RED), "20 pixels along the long side, then the stretch (red)");
}

#[test]
fn switching_the_guides_off_returns_to_plain_scaling() {
    let mut d = panel_doc(true);
    anim_scene::symbols::set_scale9_grid(&mut d, "panel", None).unwrap();
    assert!(is(&paint(&d), 30, 10, BLUE));
}
