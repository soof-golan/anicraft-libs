//! Layer, keyframe, mask, folder and visibility rules on hand-built documents.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::float_cmp, clippy::assert_is_empty)]

use crate::common;

use anim_core::Affine;
use anim_display::Node;
use anim_scene::{LayerKind, SymbolKind};
use anim_timeline::{EvalOptions, Evaluator, element_key, split_key};
use common::*;

#[test]
fn keyframe_span_covers_its_frames() {
    let d = doc(vec![layer("a", vec![keyframe(0, 3, vec![rect(0.0, 0.0, 50.0, 50.0, RED)]), keyframe(3, 3, vec![rect(0.0, 0.0, 50.0, 50.0, BLUE)])])]);
    let mut ev = Evaluator::new(&d);
    for (frame, expect) in [(0, RED), (1, RED), (2, RED), (3, BLUE), (5, BLUE)] {
        let list = ev.frame(0, frame);
        assert!(is(&draw(&list, ev.images()), 10, 10, expect), "frame {frame}");
    }
    let past = ev.frame(0, 6);
    assert!(past.nodes.is_empty(), "past the last span nothing is drawn");
    assert!(is(&draw(&past, ev.images()), 10, 10, WHITE));
}

#[test]
fn display_list_carries_stage_and_background() {
    let mut d = doc(vec![]);
    d.width = 320.0;
    d.height = 180.0;
    d.background_color = anim_core::Rgba8::opaque(10, 20, 30);
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    assert_eq!((list.size.width, list.size.height), (320.0, 180.0));
    assert_eq!(list.background, Some(anim_core::Rgba8::opaque(10, 20, 30)));
    assert!(ev.frame(9, 0).nodes.is_empty(), "unknown scene");
}

#[test]
fn first_layer_is_the_top_layer() {
    // XFL lists the top layer first: layer 0 must be painted last.
    let d = doc(vec![
        layer("top", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 60.0, 60.0, GREEN)])]),
        layer("bottom", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 100.0, RED)])]),
    ]);
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    assert_eq!(list.nodes.len(), 2);
    let keys: Vec<_> = top_groups(&list).iter().map(|g| split_key(g.key.unwrap()).unwrap().0).collect();
    assert_eq!(keys, vec![1, 0], "painter's order is bottom layer first");
    let img = draw(&list, ev.images());
    assert!(is(&img, 10, 10, GREEN));
    assert!(is(&img, 80, 80, RED));
}

#[test]
fn guide_layers_are_skipped_but_guided_layers_draw() {
    let d = doc(vec![
        layer_kind("guide", LayerKind::Guide, None, vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 100.0, RED)])]),
        layer_kind("guided", LayerKind::Normal, Some(0), vec![keyframe(0, 1, vec![rect(0.0, 0.0, 30.0, 30.0, BLUE)])]),
    ]);
    let mut ev = Evaluator::new(&d);
    let img = draw(&ev.frame(0, 0), ev.images());
    assert!(is(&img, 10, 10, BLUE));
    assert!(is(&img, 80, 80, WHITE), "the guide itself does not draw");
    ev.set_options(EvalOptions { include_guides: true, ..EvalOptions::default() });
    let img = draw(&ev.frame(0, 0), ev.images());
    assert!(is(&img, 80, 80, RED));
}

#[test]
fn hidden_layers_are_skipped_unless_included() {
    let mut hidden = layer("hidden", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 100.0, RED)])]);
    hidden.visible = false;
    let d = doc(vec![hidden]);
    let mut ev = Evaluator::new(&d);
    assert!(ev.frame(0, 0).nodes.is_empty());
    ev.set_options(EvalOptions { include_hidden: true, ..EvalOptions::default() });
    assert_eq!(ev.frame(0, 0).nodes.len(), 1);
}

#[test]
fn folders_group_their_layers() {
    let d = doc(vec![
        layer_kind("folder", LayerKind::Folder, None, vec![]),
        layer_kind("in a", LayerKind::Normal, Some(0), vec![keyframe(0, 1, vec![rect(0.0, 0.0, 40.0, 40.0, BLUE)])]),
        layer_kind("in b", LayerKind::Normal, Some(0), vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 90.0, RED)])]),
        layer("outside", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 100.0, GREEN)])]),
    ]);
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    assert_eq!(list.nodes.len(), 2, "the outside layer and one folder group");
    let Node::Group(folder) = &list.nodes[1] else { panic!("folder group") };
    assert_eq!(folder.children.len(), 2);
    let img = draw(&list, ev.images());
    assert!(is(&img, 10, 10, BLUE), "inside the folder, layer a is above layer b");
    assert!(is(&img, 80, 20, RED));
    assert!(is(&img, 50, 95, GREEN), "the layer below the folder shows where the folder does not cover it");
}

#[test]
fn hidden_folder_hides_children() {
    let mut folder = layer_kind("folder", LayerKind::Folder, None, vec![]);
    folder.visible = false;
    let d = doc(vec![folder, layer_kind("child", LayerKind::Normal, Some(0), vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 100.0, RED)])])]);
    let mut ev = Evaluator::new(&d);
    assert!(ev.frame(0, 0).nodes.is_empty());
    ev.set_options(EvalOptions { include_hidden: true, ..EvalOptions::default() });
    assert_eq!(ev.frame(0, 0).nodes.len(), 1);
}

fn mask_doc() -> anim_scene::Document {
    doc(vec![
        layer_kind("mask", LayerKind::Mask, None, vec![keyframe(0, 1, vec![rect(20.0, 20.0, 30.0, 30.0, BLUE)])]),
        layer_kind("masked", LayerKind::Normal, Some(0), vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 100.0, RED)])]),
        layer("unmasked", vec![keyframe(0, 1, vec![rect(0.0, 80.0, 100.0, 20.0, GREEN)])]),
    ])
}

#[test]
fn mask_layer_clips_the_layers_it_covers() {
    let d = mask_doc();
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    assert_eq!(list.nodes.len(), 2);
    let Node::Group(g) = &list.nodes[1] else { panic!("mask group") };
    assert!(g.mask.as_ref().is_some_and(|m| !m.is_empty()), "the mask group carries the mask layer's artwork");
    assert_eq!(g.children.len(), 1);
    let img = draw(&list, ev.images());
    assert!(is(&img, 30, 30, RED), "inside the mask");
    assert!(is(&img, 70, 70, WHITE), "outside the mask the covered layer is cut away");
    assert!(is(&img, 10, 90, GREEN), "the layer below is not masked");
    assert!(!is(&img, 30, 30, BLUE), "the mask layer itself does not draw");
}

#[test]
fn hidden_mask_does_not_mask() {
    let mut d = mask_doc();
    d.scenes[0].layers[0].visible = false;
    let mut ev = Evaluator::new(&d);
    let img = draw(&ev.frame(0, 0), ev.images());
    assert!(is(&img, 70, 70, RED));
}

#[test]
fn empty_mask_hides_the_covered_layers() {
    let mut d = mask_doc();
    d.scenes[0].layers[0].frames = vec![keyframe(0, 1, vec![])];
    let mut ev = Evaluator::new(&d);
    let img = draw(&ev.frame(0, 0), ev.images());
    assert!(is(&img, 30, 30, WHITE));
}

#[test]
fn hidden_masked_layer_is_skipped_but_mask_group_stays_valid() {
    let mut d = mask_doc();
    d.scenes[0].layers[1].visible = false;
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    assert_eq!(list.nodes.len(), 1, "only the unmasked layer");
}

#[test]
fn camera_layers_draw_nothing() {
    let d = doc(vec![layer_kind("camera", LayerKind::Camera, None, vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 100.0, RED)])])]);
    let mut ev = Evaluator::new(&d);
    assert!(ev.frame(0, 0).nodes.is_empty());
}

#[test]
fn element_keys_identify_layer_and_element() {
    let d = doc(vec![layer("a", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED), rect(20.0, 0.0, 10.0, 10.0, RED)])])]);
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    let keys: Vec<_> = top_groups(&list).iter().map(|g| g.key).collect();
    assert_eq!(keys, vec![Some(element_key(0, 0)), Some(element_key(0, 1))]);
    assert_eq!(split_key(element_key(7, 9)), Some((7, 9)));
    assert_eq!(split_key(0), None);
}

#[test]
fn instance_properties_reach_the_group() {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "box", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED)])])]);
    let mut inst = anim_scene::SymbolInstance::new("box", Affine::translate((30.0, 40.0)));
    inst.set_color_effect(anim_scene::ColorEffect::Alpha(0.5));
    inst.blend_mode = anim_core::BlendMode::Multiply;
    inst.filters.push(anim_scene::InstanceFilter::mapped(anim_display::Filter::Blur { x: 2.0, y: 2.0, quality: 1 }));
    d.scenes[0].layers = vec![layer("a", vec![keyframe(0, 1, vec![anim_scene::Element::SymbolInstance(Box::new(inst))])])];
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    let g = top_groups(&list)[0];
    assert_eq!(g.transform, Affine::translate((30.0, 40.0)));
    assert!((g.color.mul[3] - 0.5).abs() < 1e-6);
    assert_eq!(g.blend, anim_core::BlendMode::Multiply);
    assert_eq!(g.filters.len(), 1);
    assert_eq!(g.children.len(), 1);
}

#[test]
fn invisible_instances_are_skipped() {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "box", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED)])])]);
    let mut inst = anim_scene::SymbolInstance::new("box", Affine::IDENTITY);
    inst.visible = false;
    d.scenes[0].layers = vec![layer("a", vec![keyframe(0, 1, vec![anim_scene::Element::SymbolInstance(Box::new(inst))])])];
    let mut ev = Evaluator::new(&d);
    assert!(ev.frame(0, 0).nodes.is_empty());
}

#[test]
fn groups_nest_with_their_matrix() {
    let g = anim_scene::Group { matrix: Affine::translate((50.0, 50.0)), ..anim_scene::Group::new(vec![rect(0.0, 0.0, 20.0, 20.0, RED)]) };
    let d = doc(vec![layer("a", vec![keyframe(0, 1, vec![anim_scene::Element::Group(g)])])]);
    let mut ev = Evaluator::new(&d);
    let img = draw(&ev.frame(0, 0), ev.images());
    assert!(is(&img, 60, 60, RED));
    assert!(is(&img, 10, 10, WHITE));
}

#[test]
fn deeply_nested_groups_stop_at_the_cap() {
    let mut element = rect(0.0, 0.0, 10.0, 10.0, RED);
    for _ in 0..200 {
        element = anim_scene::Element::Group(anim_scene::Group::new(vec![element]));
    }
    let d = doc(vec![layer("a", vec![keyframe(0, 1, vec![element])])]);
    let mut ev = Evaluator::new(&d);
    let _ = ev.frame(0, 0);
}

#[test]
fn frame_of_symbol_previews_its_timeline() {
    let mut d = doc(vec![]);
    add_symbol(
        &mut d,
        "anim",
        SymbolKind::Graphic,
        vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED)]), keyframe(1, 1, vec![rect(50.0, 50.0, 10.0, 10.0, BLUE)])])],
    );
    let mut ev = Evaluator::new(&d);
    let a = ev.frame_of_symbol("anim", 0);
    assert!(is(&draw(&a, ev.images()), 5, 5, RED));
    let b = ev.frame_of_symbol("anim", 1);
    assert!(is(&draw(&b, ev.images()), 55, 55, BLUE));
    assert_eq!(a.background, None);
    assert!(ev.frame_of_symbol("nope", 0).nodes.is_empty());
    let bounds = ev.symbol_bounds("anim", 1).unwrap();
    assert!((bounds.x0 - 50.0).abs() < 1e-6);
}

#[test]
fn onion_skin_frames_and_alphas() {
    let frames: Vec<_> = (0..10).map(|i| keyframe(i, 1, vec![rect(f64::from(i) * 5.0, 0.0, 5.0, 5.0, RED)])).collect();
    let d = doc(vec![layer("a", frames)]);
    let mut ev = Evaluator::new(&d);
    let skins = ev.onion(0, 5, 2, 3);
    assert_eq!(skins.len(), 5);
    let alphas: Vec<f32> = skins.iter().map(|s| s.1).collect();
    // Earliest first: frames 3, 4 (before) then 6, 7, 8.
    assert!((alphas[0] - 0.25).abs() < 1e-6 && (alphas[1] - 0.5).abs() < 1e-6, "{alphas:?}");
    assert!((alphas[2] - 0.5).abs() < 1e-6 && alphas[3] < alphas[2] && alphas[4] < alphas[3], "{alphas:?}");
    assert!(skins.iter().all(|(l, a)| !l.nodes.is_empty() && *a > 0.0 && *a <= 0.5));
    // At the ends the skins are clipped to the timeline.
    assert_eq!(ev.onion(0, 0, 3, 1).len(), 1);
    assert_eq!(ev.onion(0, 9, 1, 5).len(), 1);
    assert!(ev.onion(0, 4, 0, 0).is_empty());
    assert!(ev.onion(3, 4, 2, 2).is_empty(), "unknown scene has no frames");
}

#[test]
fn fade_wraps_a_list_with_alpha() {
    let d = doc(vec![layer("a", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 10.0, 10.0, RED)])])]);
    let mut ev = Evaluator::new(&d);
    let mut list = ev.frame(0, 0);
    anim_timeline::fade(&mut list, 0.25);
    let Node::Group(g) = &list.nodes[0] else { panic!("group") };
    assert!((g.color.mul[3] - 0.25).abs() < 1e-6);
}
