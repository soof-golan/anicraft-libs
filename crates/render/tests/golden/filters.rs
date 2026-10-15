//! Filters on groups: blur, drop shadow, glow, colour matrix, and how they combine with the rest of a group.

use anim_core::{Affine, ColorTransform, Rgba8};
use anim_display::{Filter, Group, Image, Node};

use crate::common::*;

fn filtered(children: Vec<Node>, filters: Vec<Filter>) -> Node {
    Node::Group(Group { filters, children, ..Group::default() })
}

fn blur(x: f64, y: f64, quality: u8) -> Filter {
    Filter::Blur { x, y, quality }
}

fn shadow(color: Rgba8, angle_deg: f64, distance: f64, blur_px: f64) -> Filter {
    Filter::DropShadow { color, angle_deg, distance, blur_x: blur_px, blur_y: blur_px, strength: 1.0, inner: false, knockout: false, hide_object: false }
}

fn ink(img: &Image) -> f64 {
    img.premultiplied_rgba.chunks_exact(4).map(|px| f64::from(255 - px[1]) / 255.0).sum()
}

#[test]
fn blur_spreads_a_square_symmetrically_and_keeps_its_energy() {
    let plain = {
        let mut scene = Scene::new(48, 48);
        scene.add(solid_rect(20.0, 20.0, 28.0, 28.0, BLACK));
        scene.render()
    };
    let mut scene = Scene::new(48, 48);
    scene.add(filtered(vec![solid_rect(20.0, 20.0, 28.0, 28.0, BLACK)], vec![blur(8.0, 8.0, 3)]));
    let (img, stats) = scene.render_stats();
    assert!((ink(&img) - ink(&plain)).abs() < ink(&plain) * 0.01, "blur redistributes ink: {} vs {}", ink(&img), ink(&plain));
    assert_eq!(pixel(&img, 24, 12), pixel(&img, 24, 35), "vertical symmetry");
    assert_eq!(pixel(&img, 12, 24), pixel(&img, 35, 24), "horizontal symmetry");
    assert!(pixel(&img, 24, 24)[0] > 20 && pixel(&img, 24, 24)[0] < 200, "the centre is no longer solid");
    assert!(pixel(&img, 17, 24)[0] < 250, "ink spreads past the square");
    assert_eq!(stats.isolated_groups, 1);
    save("filters_blur", &img);
}

#[test]
fn blur_quality_changes_the_falloff() {
    let render = |quality| {
        let mut scene = Scene::new(64, 16);
        scene.add(filtered(vec![solid_rect(28.0, 0.0, 36.0, 16.0, BLACK)], vec![blur(12.0, 0.0, quality)]));
        scene.render()
    };
    let (one, three) = (render(1), render(3));
    // One box pass is a trapezoid, three passes are bell-shaped: the tail is lighter, the shoulders steeper.
    assert_ne!(pixel(&one, 14, 8)[0], pixel(&three, 14, 8)[0]);
    assert_eq!(pixel(&one, 32, 3), pixel(&one, 32, 12), "x-only blur leaves rows identical");
    save("filters_blur_q3", &three);
}

#[test]
fn blur_scales_with_the_zoom() {
    let scene_at = |zoom: f64| {
        let mut scene = Scene::new(32, 32);
        scene.add(filtered(vec![solid_rect(14.0, 14.0, 18.0, 18.0, BLACK)], vec![blur(4.0, 4.0, 1)]));
        scene.render_with(&anim_render::RenderOptions::new(32 * 2, 32 * 2).with_view(Affine::scale(zoom)))
    };
    let (small, big) = (scene_at(1.0), scene_at(2.0));
    // At 2x the blurred blob covers twice the width: compare how far ink reaches from the centre.
    let reach = |img: &Image, centre: u32| (0..centre).filter(|&d| pixel(img, centre + d, centre)[1] < 250).count();
    assert!(reach(&big, 32) >= reach(&small, 16) * 2 - 1, "{} vs {}", reach(&big, 32), reach(&small, 16));
}

#[test]
fn blur_reaches_in_from_outside_the_view() {
    let mut scene = Scene::new(32, 32);
    scene.add(filtered(vec![solid_rect(-30.0, 8.0, -4.0, 24.0, BLACK)], vec![blur(24.0, 24.0, 2)]));
    let img = scene.render();
    assert!(pixel(&img, 0, 16)[1] < 240, "the blur of an off-screen shape shades the edge: {:?}", pixel(&img, 0, 16));
    assert_eq!(pixel(&img, 31, 16), [255, 255, 255, 255], "but not the far side");
}

#[test]
fn drop_shadow_falls_where_the_angle_says() {
    let mut scene = Scene::new(48, 48);
    scene.add(filtered(vec![solid_rect(10.0, 10.0, 22.0, 22.0, RED)], vec![shadow(BLACK, 45.0, 10.0, 0.0)]));
    let img = scene.render();
    // 45 degrees, distance 10: the shadow is the square moved by about (7.07, 7.07).
    assert_px(&img, 15, 15, [255, 0, 0, 255], 0);
    assert_px(&img, 27, 27, [0, 0, 0, 255], 4);
    assert_px(&img, 27, 12, [255, 255, 255, 255], 0);
    assert_px(&img, 12, 27, [255, 255, 255, 255], 0);
    save("filters_drop_shadow", &img);
}

#[test]
fn soft_drop_shadow_fades() {
    let mut scene = Scene::new(64, 64);
    scene.add(filtered(vec![solid_rect(20.0, 20.0, 36.0, 36.0, BLUE)], vec![shadow(BLACK, 90.0, 8.0, 10.0)]));
    let img = scene.render();
    let below = (36..58).map(|y| pixel(&img, 28, y)[1]).collect::<Vec<_>>();
    assert!(below.windows(2).all(|w| w[0] <= w[1]), "shadow fades monotonically with distance: {below:?}");
    assert!(below[0] < 200 && below[below.len() - 1] > 240);
    save("filters_soft_shadow", &img);
}

#[test]
fn drop_shadow_colour_alpha_and_strength() {
    let render = |color: Rgba8, strength: f64| {
        let mut scene = Scene::new(40, 20);
        let filter =
            Filter::DropShadow { color, angle_deg: 0.0, distance: 12.0, blur_x: 6.0, blur_y: 0.0, strength, inner: false, knockout: false, hide_object: false };
        scene.add(filtered(vec![solid_rect(4.0, 6.0, 12.0, 14.0, BLUE)], vec![filter]));
        scene.render()
    };
    let faint = render(Rgba8::new(0, 0, 0, 128), 1.0);
    assert!(pixel(&faint, 20, 10)[1] > 100 && pixel(&faint, 20, 10)[1] < 160, "half-alpha shadow colour: {:?}", pixel(&faint, 20, 10));
    let strong = render(Rgba8::new(0, 0, 0, 255), 4.0);
    assert!(pixel(&strong, 14, 10)[1] < pixel(&render(Rgba8::BLACK, 1.0), 14, 10)[1], "strength darkens the soft edge");
}

#[test]
fn knockout_and_hide_object_show_only_the_shadow() {
    let render = |knockout, hide_object| {
        let mut scene = Scene::new(40, 40);
        let filter =
            Filter::DropShadow { color: BLACK, angle_deg: 0.0, distance: 8.0, blur_x: 0.0, blur_y: 0.0, strength: 1.0, inner: false, knockout, hide_object };
        scene.add(filtered(vec![solid_rect(8.0, 8.0, 20.0, 20.0, RED)], vec![filter]));
        scene.render()
    };
    let normal = render(false, false);
    let knocked = render(true, false);
    let hidden = render(false, true);
    assert_px(&normal, 10, 12, [255, 0, 0, 255], 0);
    assert_px(&knocked, 10, 12, [255, 255, 255, 255], 0);
    assert_px(&knocked, 18, 12, [255, 255, 255, 255], 0);
    assert_px(&knocked, 24, 12, [0, 0, 0, 255], 0);
    assert_px(&hidden, 10, 12, [255, 255, 255, 255], 0);
    assert_px(&hidden, 18, 12, [0, 0, 0, 255], 0);
}

#[test]
fn glow_wraps_the_shape_in_its_colour() {
    let mut scene = Scene::new(48, 48);
    let glow = Filter::Glow { color: Rgba8::opaque(255, 200, 0), blur_x: 10.0, blur_y: 10.0, strength: 2.0, inner: false, knockout: false };
    scene.add(filtered(vec![solid_circle(24.0, 24.0, 8.0, BLUE)], vec![glow]));
    let img = scene.render();
    assert_px(&img, 24, 24, [0, 0, 255, 255], 0);
    let ring = pixel(&img, 24, 12);
    assert!(ring[0] > 200 && ring[1] > 150 && ring[2] < 200, "yellow-orange halo near the rim: {ring:?}");
    assert_eq!(pixel(&img, 1, 1), [255, 255, 255, 255]);
    save("filters_glow", &img);
}

#[test]
fn inner_glow_stays_inside_the_shape() {
    let mut scene = Scene::new(48, 48);
    let glow = Filter::Glow { color: Rgba8::opaque(255, 255, 0), blur_x: 8.0, blur_y: 8.0, strength: 2.0, inner: true, knockout: false };
    scene.add(filtered(vec![solid_circle(24.0, 24.0, 14.0, BLUE)], vec![glow]));
    let img = scene.render();
    assert_px(&img, 24, 24, [0, 0, 255, 255], 0);
    assert!(pixel(&img, 24, 11)[0] > 100, "inner glow tints the rim: {:?}", pixel(&img, 24, 11));
    assert_px(&img, 24, 6, [255, 255, 255, 255], 0);
    save("filters_inner_glow", &img);
}

#[test]
fn colour_matrix_greyscale() {
    let mut scene = Scene::new(16, 16);
    // Luma weights in the three colour rows, alpha untouched.
    #[rustfmt::skip]
    let grey = [
        0.3, 0.59, 0.11, 0.0, 0.0,
        0.3, 0.59, 0.11, 0.0, 0.0,
        0.3, 0.59, 0.11, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    scene.add(filtered(vec![solid_rect(0.0, 0.0, 16.0, 16.0, Rgba8::opaque(200, 100, 50))], vec![Filter::ColorMatrix(grey)]));
    assert_px(&scene.render(), 8, 8, [125, 125, 125, 255], 2);
}

#[test]
fn colour_matrix_keeps_transparent_pixels_transparent() {
    let mut scene = Scene::with_background(16, 16, None);
    #[rustfmt::skip]
    let lift = [
        1.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 128.0,
    ];
    scene.add(filtered(vec![solid_rect(4.0, 4.0, 8.0, 8.0, RED)], vec![Filter::ColorMatrix(lift)]));
    let img = scene.render();
    assert_eq!(pixel(&img, 0, 0), [0, 0, 0, 0]);
    assert_px(&img, 5, 5, [255, 0, 0, 255], 0);
}

#[test]
fn filters_run_in_order() {
    let render = |filters: Vec<Filter>| {
        let mut scene = Scene::new(48, 24);
        scene.add(filtered(vec![solid_rect(18.0, 8.0, 24.0, 16.0, RED)], filters));
        scene.render()
    };
    #[rustfmt::skip]
    let to_blue = [
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    let blur_then_matrix = render(vec![blur(6.0, 6.0, 2), Filter::ColorMatrix(to_blue)]);
    let matrix_then_blur = render(vec![Filter::ColorMatrix(to_blue), blur(6.0, 6.0, 2)]);
    // The colour matrix is applied to straight colour, so the faded blur edge keeps the full blue either way;
    // what differs is the premultiplied maths at partial alpha, which must still be valid and symmetric.
    for img in [&blur_then_matrix, &matrix_then_blur] {
        assert_eq!(pixel(img, 21, 3), pixel(img, 21, 20));
        assert!(pixel(img, 21, 12)[0] < 120, "red is gone: {:?}", pixel(img, 21, 12));
    }
}

#[test]
fn filters_apply_before_the_colour_transform_and_after_the_mask() {
    let mut scene = Scene::new(40, 20);
    let group = Group {
        color: ColorTransform::alpha(0.5),
        mask: Some(vec![solid_rect(0.0, 0.0, 20.0, 20.0, BLACK)]),
        filters: vec![shadow(BLACK, 0.0, 10.0, 0.0)],
        children: vec![solid_rect(2.0, 6.0, 10.0, 14.0, RED)],
        ..Group::default()
    };
    scene.add(Node::Group(group));
    let img = scene.render();
    // The mask clips the red square before the shadow is made, so the shadow falls on x 12..20 even though
    // nothing of the mask covers x >= 20; the colour transform then halves object and shadow alike.
    assert_px(&img, 5, 10, [255, 128, 128, 255], 2);
    assert_px(&img, 15, 10, [128, 128, 128, 255], 2);
    assert_px(&img, 25, 10, [255, 255, 255, 255], 0);
}

#[test]
fn huge_filter_values_stay_bounded() {
    let mut scene = Scene::new(32, 32);
    scene.add(filtered(vec![solid_rect(8.0, 8.0, 24.0, 24.0, BLACK)], vec![blur(1e12, f64::NAN, 255), shadow(BLACK, f64::INFINITY, 1e300, 1e9)]));
    let (img, stats) = scene.render_stats();
    assert_eq!((img.width, img.height), (32, 32));
    assert!(stats.peak_layer_bytes < 200_000_000, "padding is capped: {}", stats.peak_layer_bytes);
}
