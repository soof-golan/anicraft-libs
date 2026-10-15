//! Groups: transforms, colour transforms, blend modes, masks and the offscreen bookkeeping.

use anim_core::{Affine, BlendMode, ColorTransform, Rgba8};
use anim_display::{Group, Node};

use crate::common::*;

fn group_node(group: Group) -> Node {
    Node::Group(group)
}

#[test]
fn nested_group_transforms_compose() {
    let mut scene = Scene::new(40, 40);
    let inner = Group { transform: Affine::scale(2.0), children: vec![solid_rect(0.0, 0.0, 3.0, 2.0, RED)], ..Group::default() };
    let outer = Group { transform: Affine::translate((10.0, 20.0)), children: vec![group_node(inner)], ..Group::default() };
    scene.add(group_node(outer));
    let (img, stats) = scene.render_stats();
    assert_eq!(count(&img, |px| px == [255, 0, 0, 255]), 6 * 4);
    assert_px(&img, 10, 20, [255, 0, 0, 255], 0);
    assert_px(&img, 15, 23, [255, 0, 0, 255], 0);
    assert_px(&img, 16, 23, [255, 255, 255, 255], 0);
    assert_eq!((stats.direct_groups, stats.isolated_groups), (2, 0), "transform-only groups never use an offscreen layer");
}

#[test]
fn colour_transform_applies_to_the_composited_group() {
    let mut scene = Scene::new(20, 10);
    // Two overlapping opaque rects: a per-shape alpha would show the overlap darker; per group it does not.
    let children = vec![solid_rect(0.0, 0.0, 12.0, 10.0, RED), solid_rect(8.0, 0.0, 20.0, 10.0, BLUE)];
    scene.add(with_color(children, ColorTransform::alpha(0.5)));
    let (img, stats) = scene.render_stats();
    assert_px(&img, 2, 5, [255, 128, 128, 255], 2);
    assert_px(&img, 10, 5, [128, 128, 255, 255], 2);
    assert_px(&img, 18, 5, [128, 128, 255, 255], 2);
    assert_eq!(stats.isolated_groups, 1);
    save("groups_alpha_unit", &img);
}

#[test]
fn colour_transform_tint_brightness_and_offset() {
    let render = |ct: ColorTransform| {
        let mut scene = Scene::new(4, 4);
        scene.add(with_color(vec![solid_rect(0.0, 0.0, 4.0, 4.0, Rgba8::opaque(200, 100, 0))], ct));
        pixel(&scene.render(), 2, 2)
    };
    assert_eq!(render(ColorTransform::IDENTITY), [200, 100, 0, 255]);
    assert_eq!(render(ColorTransform::brightness(1.0)), [255, 255, 255, 255]);
    assert_eq!(render(ColorTransform::brightness(-1.0)), [0, 0, 0, 255]);
    assert_eq!(render(ColorTransform::tint(Rgba8::opaque(0, 0, 255), 0.5)), [100, 50, 128, 255]);
    assert_eq!(render(ColorTransform { mul: [0.5, 1.0, 1.0, 1.0], add: [0.0, 100.0 / 255.0, 0.0, 0.0] }), [100, 200, 0, 255]);
}

#[test]
fn nested_colour_transforms_multiply() {
    let mut scene = Scene::new(4, 4);
    let inner = with_color(vec![solid_rect(0.0, 0.0, 4.0, 4.0, RED)], ColorTransform::alpha(0.5));
    scene.add(with_color(vec![inner], ColorTransform::alpha(0.5)));
    assert_px(&scene.render(), 1, 1, [255, 191, 191, 255], 2);
}

#[test]
fn colour_transform_leaves_transparent_pixels_alone() {
    let mut scene = Scene::with_background(20, 20, None);
    scene.add(with_color(vec![solid_rect(5.0, 5.0, 10.0, 10.0, RED)], ColorTransform { mul: [1.0; 4], add: [0.0, 0.0, 0.0, 0.5] }));
    let img = scene.render();
    assert_eq!(pixel(&img, 1, 1), [0, 0, 0, 0], "an alpha offset must not paint the group's bounding box");
    assert_px(&img, 7, 7, [255, 0, 0, 255], 0);
}

fn blend(mode: BlendMode, backdrop: Rgba8, source: Rgba8) -> [u8; 4] {
    let mut scene = Scene::with_background(8, 8, Some(backdrop));
    scene.add(group_node(Group { blend: mode, children: vec![solid_rect(0.0, 0.0, 8.0, 8.0, source)], ..Group::default() }));
    pixel(&scene.render(), 4, 4)
}

#[test]
fn blend_modes_follow_their_formulas() {
    let backdrop = Rgba8::opaque(200, 100, 50);
    let source = Rgba8::opaque(100, 200, 255);
    let (b, s) = ([200_i32, 100, 50], [100_i32, 200, 255]);
    let per_channel = |f: &dyn Fn(i32, i32) -> i32| -> [u8; 4] {
        let c = |i: usize| u8::try_from(f(b[i], s[i]).clamp(0, 255)).unwrap();
        [c(0), c(1), c(2), 255]
    };
    let cases: Vec<(BlendMode, [u8; 4])> = vec![
        (BlendMode::Normal, [100, 200, 255, 255]),
        (BlendMode::Layer, [100, 200, 255, 255]),
        (BlendMode::PassThrough, [100, 200, 255, 255]),
        (BlendMode::Multiply, per_channel(&|b, s| b * s / 255)),
        (BlendMode::Screen, per_channel(&|b, s| 255 - (255 - b) * (255 - s) / 255)),
        (BlendMode::Add, per_channel(&|b, s| b + s)),
        (BlendMode::Subtract, per_channel(&|b, s| b - s)),
        (BlendMode::Difference, per_channel(&|b, s| (b - s).abs())),
        (BlendMode::Darken, per_channel(&|b, s| b.min(s))),
        (BlendMode::Lighten, per_channel(&|b, s| b.max(s))),
        (BlendMode::Invert, per_channel(&|b, _| 255 - b)),
        (BlendMode::Overlay, per_channel(&|b, s| if b < 128 { 2 * b * s / 255 } else { 255 - 2 * (255 - b) * (255 - s) / 255 })),
        (BlendMode::HardLight, per_channel(&|b, s| if s < 128 { 2 * b * s / 255 } else { 255 - 2 * (255 - b) * (255 - s) / 255 })),
        (BlendMode::Exclusion, per_channel(&|b, s| b + s - 2 * b * s / 255)),
    ];
    for (mode, expected) in cases {
        let got = blend(mode, backdrop, source);
        let close = got.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 3);
        assert!(close, "{mode:?}: got {got:?}, expected {expected:?}");
    }
}

#[test]
fn colour_blend_modes_run_and_keep_luminosity_rules() {
    let backdrop = Rgba8::opaque(200, 60, 60);
    let source = Rgba8::opaque(30, 30, 220);
    for mode in
        [BlendMode::ColorDodge, BlendMode::ColorBurn, BlendMode::SoftLight, BlendMode::Hue, BlendMode::Saturation, BlendMode::Color, BlendMode::Luminosity]
    {
        let got = blend(mode, backdrop, source);
        assert_eq!(got[3], 255, "{mode:?} stays opaque: {got:?}");
    }
    // Luminosity of the source on the backdrop's colour: result is dark because the source is dark.
    let lum = blend(BlendMode::Luminosity, backdrop, source);
    assert!(u32::from(lum[0]) + u32::from(lum[1]) + u32::from(lum[2]) < 400, "{lum:?}");
}

#[test]
fn blend_modes_respect_partial_source_alpha() {
    let half_black = Rgba8::new(0, 0, 0, 128);
    let got = blend(BlendMode::Multiply, WHITE, half_black);
    assert_px_arr(got, [127, 127, 127, 255], 2);
    let got = blend(BlendMode::Add, Rgba8::opaque(100, 100, 100), Rgba8::new(100, 100, 100, 128));
    assert_px_arr(got, [150, 150, 150, 255], 2);
    let got = blend(BlendMode::Subtract, Rgba8::opaque(200, 200, 200), Rgba8::new(100, 100, 100, 128));
    assert_px_arr(got, [150, 150, 150, 255], 2);
    let got = blend(BlendMode::Invert, Rgba8::opaque(200, 0, 100), Rgba8::new(0, 0, 0, 128));
    assert_px_arr(got, [127, 128, 128, 255], 3);
}

#[track_caller]
fn assert_px_arr(got: [u8; 4], expected: [u8; 4], tolerance: u8) {
    assert!(got.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= tolerance), "got {got:?}, expected {expected:?}");
}

#[test]
fn blend_modes_apply_to_the_group_as_a_unit() {
    // Two overlapping source rects multiplied onto the backdrop as one group: the overlap is not multiplied twice.
    let mut scene = Scene::with_background(20, 10, Some(Rgba8::opaque(128, 128, 128)));
    let children = vec![solid_rect(0.0, 0.0, 12.0, 10.0, Rgba8::opaque(128, 128, 128)), solid_rect(8.0, 0.0, 20.0, 10.0, Rgba8::opaque(128, 128, 128))];
    scene.add(group_node(Group { blend: BlendMode::Multiply, children, ..Group::default() }));
    let img = scene.render();
    assert_px(&img, 2, 5, [64, 64, 64, 255], 2);
    assert_px(&img, 10, 5, [64, 64, 64, 255], 2);
}

#[test]
fn layer_blend_isolates_the_children() {
    let build = |mode| {
        let mut scene = Scene::new(20, 10);
        scene.add(solid_rect(0.0, 0.0, 20.0, 10.0, RED));
        let multiply_child = group_node(Group {
            blend: BlendMode::Multiply,
            children: vec![solid_rect(0.0, 0.0, 20.0, 10.0, Rgba8::opaque(128, 255, 255))],
            ..Group::default()
        });
        scene.add(group_node(Group { blend: mode, children: vec![multiply_child], ..Group::default() }));
        scene.render()
    };
    // Without isolation the child multiplies with the red behind it; inside a Layer it meets nothing and covers it.
    assert_px(&build(BlendMode::Normal), 5, 5, [128, 0, 0, 255], 2);
    assert_px(&build(BlendMode::Layer), 5, 5, [128, 255, 255, 255], 2);
}

fn circle_group(blend: BlendMode) -> Node {
    group_node(Group { blend, children: vec![solid_circle(10.0, 10.0, 6.0, BLACK)], ..Group::default() })
}

#[test]
fn alpha_blend_keeps_only_what_the_child_covers() {
    let mut scene = Scene::new(20, 20);
    let layer = Group { blend: BlendMode::Layer, children: vec![solid_rect(0.0, 0.0, 20.0, 20.0, RED), circle_group(BlendMode::Alpha)], ..Group::default() };
    scene.add(group_node(layer));
    let img = scene.render();
    assert_px(&img, 10, 10, [255, 0, 0, 255], 0);
    assert_px(&img, 2, 2, [255, 255, 255, 255], 0);
    assert_px(&img, 17, 17, [255, 255, 255, 255], 0);
    let ink: f64 = img.premultiplied_rgba.chunks_exact(4).map(|px| f64::from(255 - px[1]) / 255.0).sum();
    let expected = std::f64::consts::PI * 36.0;
    assert!((ink - expected).abs() < expected * 0.03, "area kept by the alpha child: {ink} vs {expected}");
    save("groups_alpha_blend", &img);
}

#[test]
fn erase_blend_cuts_the_child_out() {
    let mut scene = Scene::new(20, 20);
    let layer = Group { blend: BlendMode::Layer, children: vec![solid_rect(0.0, 0.0, 20.0, 20.0, RED), circle_group(BlendMode::Erase)], ..Group::default() };
    scene.add(group_node(layer));
    let img = scene.render();
    assert_px(&img, 10, 10, [255, 255, 255, 255], 0);
    assert_px(&img, 2, 2, [255, 0, 0, 255], 0);
    save("groups_erase_blend", &img);
}

#[test]
fn alpha_blend_with_nothing_visible_clears_the_layer() {
    let mut scene = Scene::new(20, 20);
    let away = group_node(Group { blend: BlendMode::Alpha, children: vec![solid_circle(500.0, 500.0, 6.0, BLACK)], ..Group::default() });
    let layer = Group { blend: BlendMode::Layer, children: vec![solid_rect(0.0, 0.0, 20.0, 20.0, RED), away], ..Group::default() };
    scene.add(group_node(layer));
    assert_px(&scene.render(), 10, 10, [255, 255, 255, 255], 0);
}

#[test]
fn mask_clips_children_to_its_alpha() {
    let mut scene = Scene::new(32, 32);
    let masked = Group { mask: Some(vec![solid_circle(16.0, 16.0, 8.0, BLACK)]), children: vec![solid_rect(0.0, 0.0, 32.0, 32.0, RED)], ..Group::default() };
    scene.add(group_node(masked));
    let (img, stats) = scene.render_stats();
    assert_px(&img, 16, 16, [255, 0, 0, 255], 0);
    assert_px(&img, 4, 4, [255, 255, 255, 255], 0);
    assert_px(&img, 28, 16, [255, 255, 255, 255], 0);
    let ink: f64 = img.premultiplied_rgba.chunks_exact(4).map(|px| f64::from(255 - px[1]) / 255.0).sum();
    let expected = std::f64::consts::PI * 64.0;
    assert!((ink - expected).abs() < expected * 0.02, "masked area {ink} vs {expected}");
    assert_eq!(stats.isolated_groups, 1);
    save("groups_mask_circle", &img);
}

#[test]
fn partially_transparent_masks_scale_alpha() {
    let mut scene = Scene::new(8, 8);
    let masked = Group {
        mask: Some(vec![solid_rect(0.0, 0.0, 8.0, 8.0, Rgba8::new(0, 0, 0, 128))]),
        children: vec![solid_rect(0.0, 0.0, 8.0, 8.0, RED)],
        ..Group::default()
    };
    scene.add(group_node(masked));
    assert_px(&scene.render(), 4, 4, [255, 127, 127, 255], 2);
}

#[test]
fn masks_live_in_the_groups_local_space() {
    let mut scene = Scene::new(40, 10);
    let masked = Group {
        transform: Affine::translate((10.0, 0.0)) * Affine::scale(2.0),
        mask: Some(vec![solid_rect(0.0, 0.0, 5.0, 5.0, BLACK)]),
        children: vec![solid_rect(0.0, 0.0, 20.0, 20.0, RED)],
        ..Group::default()
    };
    scene.add(group_node(masked));
    let img = scene.render();
    assert_eq!(count(&img, |px| px == [255, 0, 0, 255]), 10 * 10, "the mask is scaled and moved with the children");
    assert_px(&img, 10, 0, [255, 0, 0, 255], 0);
    assert_px(&img, 19, 9, [255, 0, 0, 255], 0);
    assert_px(&img, 20, 5, [255, 255, 255, 255], 0);
}

#[test]
fn empty_or_disjoint_masks_hide_everything() {
    for mask in [vec![], vec![solid_rect(100.0, 100.0, 110.0, 110.0, BLACK)]] {
        let mut scene = Scene::new(16, 16);
        scene.add(group_node(Group { mask: Some(mask), children: vec![solid_rect(0.0, 0.0, 16.0, 16.0, RED)], ..Group::default() }));
        assert_eq!(count(&scene.render(), |px| px != [255, 255, 255, 255]), 0);
    }
}

#[test]
fn nested_masks_intersect() {
    let mut scene = Scene::new(32, 16);
    let inner = Group { mask: Some(vec![solid_rect(8.0, 0.0, 32.0, 16.0, BLACK)]), children: vec![solid_rect(0.0, 0.0, 32.0, 16.0, RED)], ..Group::default() };
    let outer = Group { mask: Some(vec![solid_rect(0.0, 0.0, 24.0, 16.0, BLACK)]), children: vec![group_node(inner)], ..Group::default() };
    scene.add(group_node(outer));
    let img = scene.render();
    assert_eq!(count(&img, |px| px == [255, 0, 0, 255]), 16 * 16);
    assert_px(&img, 7, 5, [255, 255, 255, 255], 0);
    assert_px(&img, 8, 5, [255, 0, 0, 255], 0);
    assert_px(&img, 23, 5, [255, 0, 0, 255], 0);
    assert_px(&img, 24, 5, [255, 255, 255, 255], 0);
}

#[test]
fn offscreen_layers_are_limited_to_the_groups_bounds() {
    let mut scene = Scene::new(1000, 1000);
    scene.add(with_color(vec![solid_rect(500.0, 500.0, 510.0, 510.0, RED)], ColorTransform::alpha(0.5)));
    let (img, stats) = scene.render_stats();
    assert_px(&img, 505, 505, [255, 128, 128, 255], 2);
    assert!(stats.peak_layer_bytes <= 12 * 12 * 4, "a 10x10 group must not allocate a full-stage layer: {} bytes", stats.peak_layer_bytes);
}

#[test]
fn groups_far_outside_the_view_are_culled() {
    let mut scene = Scene::new(20, 20);
    scene.add(with_color(vec![solid_rect(500.0, 500.0, 510.0, 510.0, RED)], ColorTransform::alpha(0.5)));
    scene.add(solid_rect(-50.0, -50.0, -40.0, -40.0, RED));
    let (img, stats) = scene.render_stats();
    assert_eq!(count(&img, |px| px != [255, 255, 255, 255]), 0);
    assert_eq!(stats.isolated_groups, 0);
    assert!(stats.culled >= 1);
    assert!(stats.is_clean());
}

#[test]
fn partially_visible_blend_groups_composite_where_they_overlap() {
    let mut scene = Scene::new(20, 20);
    scene.add(solid_rect(0.0, 0.0, 20.0, 20.0, Rgba8::opaque(128, 128, 128)));
    let shifted = Group {
        transform: Affine::translate((-5.0, -5.0)),
        blend: BlendMode::Multiply,
        children: vec![solid_rect(0.0, 0.0, 10.0, 10.0, Rgba8::opaque(128, 128, 128))],
        ..Group::default()
    };
    scene.add(group_node(shifted));
    let img = scene.render();
    assert_px(&img, 2, 2, [64, 64, 64, 255], 2);
    assert_px(&img, 6, 6, [128, 128, 128, 255], 0);
}
