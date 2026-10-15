//! Gradients, bitmap fills and bitmap nodes.

use anim_core::{Affine, Rgba8};
use anim_display::{Bitmap, Fill, FillRule, Gradient, GradientStop, Image, ImageId, Interpolation, Node, Paint, Spread};

use crate::common::*;

/// A 100x10 scene with a full-width rect painted by `gradient`.
fn strip(gradient: Gradient) -> anim_display::Image {
    let mut scene = Scene::new(100, 10);
    scene.add(fill(rect_path(0.0, 0.0, 100.0, 10.0), Paint::Linear(gradient)));
    scene.render()
}

#[test]
fn linear_gradient_endpoints_and_middle() {
    let img = strip(gradient(0.0, 100.0, RED, BLUE));
    assert_px(&img, 0, 5, [254, 0, 1, 255], 3);
    assert_px(&img, 99, 5, [1, 0, 254, 255], 3);
    assert_px(&img, 49, 5, [128, 0, 127, 255], 3);
    assert_px(&img, 25, 5, [190, 0, 65, 255], 3);
    save("paints_linear", &img);
}

#[test]
fn linear_gradient_pads_beyond_its_ends() {
    let img = strip(gradient(20.0, 80.0, RED, BLUE));
    assert_px(&img, 2, 5, [255, 0, 0, 255], 1);
    assert_px(&img, 97, 5, [0, 0, 255, 255], 1);
    assert_px(&img, 49, 5, [133, 0, 122, 255], 4);
}

#[test]
fn linear_gradient_repeats_and_reflects() {
    let mut repeat = gradient(0.0, 50.0, RED, BLUE);
    repeat.spread = Spread::Repeat;
    let img = strip(repeat);
    assert_px(&img, 0, 5, [254, 0, 1, 255], 4);
    assert_px(&img, 49, 5, [3, 0, 252, 255], 6);
    assert_px(&img, 50, 5, [252, 0, 3, 255], 6);
    assert_px(&img, 74, 5, [130, 0, 125, 255], 6);

    let mut reflect = gradient(0.0, 50.0, RED, BLUE);
    reflect.spread = Spread::Reflect;
    let img = strip(reflect);
    assert_px(&img, 49, 5, [3, 0, 252, 255], 6);
    assert_px(&img, 50, 5, [3, 0, 252, 255], 6);
    assert_px(&img, 99, 5, [252, 0, 3, 255], 6);
    save("paints_linear_reflect", &img);
}

#[test]
fn gradient_stops_with_an_alpha_and_a_hard_edge() {
    let mut g = gradient(0.0, 100.0, RED, BLUE);
    g.stops = vec![
        GradientStop { offset: 0.0, color: Rgba8::new(255, 0, 0, 0) },
        GradientStop { offset: 0.5, color: Rgba8::new(255, 0, 0, 255) },
        GradientStop { offset: 0.5, color: Rgba8::new(0, 0, 255, 255) },
        GradientStop { offset: 1.0, color: Rgba8::new(0, 0, 255, 255) },
    ];
    let img = strip(g);
    assert_px(&img, 25, 5, [255, 128, 128, 255], 4);
    assert_px(&img, 48, 5, [255, 10, 10, 255], 14);
    assert_px(&img, 52, 5, [0, 0, 255, 255], 1);
    assert_px(&img, 90, 5, [0, 0, 255, 255], 0);
}

#[test]
fn unsorted_and_out_of_range_stops_do_not_break_the_gradient() {
    let mut g = gradient(0.0, 100.0, RED, BLUE);
    g.stops = vec![GradientStop { offset: 2.0, color: BLUE }, GradientStop { offset: -1.0, color: RED }, GradientStop { offset: f32::NAN, color: GREEN }];
    let img = strip(g);
    // Offsets are clamped to 0..=1 and sorted: a plain red-to-blue gradient; the NaN stop is ignored.
    assert_px(&img, 0, 5, [254, 0, 1, 255], 3);
    assert_px(&img, 99, 5, [1, 0, 254, 255], 3);
    assert_px(&img, 49, 5, [128, 0, 127, 255], 3);
}

#[test]
fn linear_rgb_interpolation_is_brighter_in_the_middle() {
    let mut g = gradient(0.0, 100.0, BLACK, WHITE);
    let srgb = strip(g.clone());
    g.interpolation = Interpolation::LinearRgb;
    let linear = strip(g);
    assert_px(&srgb, 49, 5, [128, 128, 128, 255], 4);
    assert_px(&linear, 49, 5, [187, 187, 187, 255], 6);
    // Exact values: linear light 0.015 encodes to sRGB 33, and 0.985 to 253.
    assert_px(&linear, 1, 5, [33, 33, 33, 255], 4);
    assert_px(&linear, 98, 5, [253, 253, 253, 255], 3);
}

fn radial_scene(focal: f64, spread: Spread) -> anim_display::Image {
    let mut g = gradient(0.0, 1.0, RED, BLUE);
    // Radius 40 centred on (50, 50): the gradient circle has radius 819.2 in its own space.
    g.transform = Affine::translate((50.0, 50.0)) * Affine::scale(40.0 / anim_display::GRADIENT_EXTENT);
    g.focal = focal;
    g.spread = spread;
    let mut scene = Scene::new(100, 100);
    scene.add(fill(rect_path(0.0, 0.0, 100.0, 100.0), Paint::Radial(g)));
    scene.render()
}

#[test]
fn radial_gradient_centre_and_rim() {
    let img = radial_scene(0.0, Spread::Pad);
    assert_px(&img, 50, 50, [253, 0, 2, 255], 5);
    assert_px(&img, 70, 50, [128, 0, 127, 255], 5);
    assert_px(&img, 50, 30, [128, 0, 127, 255], 5);
    assert_px(&img, 90, 50, [0, 0, 255, 255], 6);
    assert_px(&img, 2, 2, [0, 0, 255, 255], 0);
    save("paints_radial", &img);
}

#[test]
fn focal_point_moves_the_start_colour() {
    let img = radial_scene(0.5, Spread::Pad);
    // The focal point is half way to the rim on the +x side: (70, 50).
    assert_px(&img, 70, 50, [253, 0, 2, 255], 8);
    // Opposite the focal point the ray reaches the rim 60 px away; 40 of them are covered at (30, 50).
    assert_px(&img, 30, 50, [85, 0, 170, 255], 8);
    assert_px(&img, 99, 50, [0, 0, 255, 255], 6);
    save("paints_radial_focal", &img);
}

#[test]
fn radial_gradient_can_repeat() {
    let img = radial_scene(0.0, Spread::Repeat);
    assert_px(&img, 50, 50, [253, 0, 2, 255], 6);
    // 50 px from the centre is 1.25 radii: t wraps to 0.25.
    assert_px(&img, 100 - 1, 50, [188, 0, 66, 255], 12);
}

fn checker_image(store: &mut anim_display::ImageStore) -> ImageId {
    // 2x2: red, green / blue, white.
    #[rustfmt::skip]
    let pixels = vec![
        255, 0, 0, 255,   0, 255, 0, 255,
        0, 0, 255, 255,   255, 255, 255, 255,
    ];
    store.insert(Image::new(2, 2, pixels).unwrap())
}

fn bitmap_fill(image: ImageId, repeat: bool, smoothing: bool, scale: f64) -> Paint {
    Paint::Bitmap { image, transform: Affine::scale(scale), repeat, smoothing }
}

#[test]
fn bitmap_fill_repeats_with_nearest_sampling() {
    let mut scene = Scene::new(16, 16);
    let image = checker_image(&mut scene.images);
    scene.add(fill(rect_path(0.0, 0.0, 16.0, 16.0), bitmap_fill(image, true, false, 4.0)));
    let img = scene.render();
    assert_px(&img, 1, 1, [255, 0, 0, 255], 0);
    assert_px(&img, 5, 1, [0, 255, 0, 255], 0);
    assert_px(&img, 1, 5, [0, 0, 255, 255], 0);
    assert_px(&img, 5, 5, [255, 255, 255, 255], 0);
    assert_px(&img, 9, 1, [255, 0, 0, 255], 0);
    assert_px(&img, 13, 13, [255, 255, 255, 255], 0);
    assert_px(&img, 3, 1, [255, 0, 0, 255], 0);
    assert_px(&img, 4, 1, [0, 255, 0, 255], 0);
    save("paints_bitmap_fill_repeat", &img);
}

#[test]
fn bitmap_fill_without_repeat_clamps_to_the_edge() {
    let mut scene = Scene::new(16, 16);
    let image = checker_image(&mut scene.images);
    scene.add(fill(rect_path(0.0, 0.0, 16.0, 16.0), bitmap_fill(image, false, false, 4.0)));
    let img = scene.render();
    assert_px(&img, 1, 1, [255, 0, 0, 255], 0);
    assert_px(&img, 13, 1, [0, 255, 0, 255], 0);
    assert_px(&img, 1, 13, [0, 0, 255, 255], 0);
    assert_px(&img, 13, 13, [255, 255, 255, 255], 0);
}

#[test]
fn smoothing_blends_texel_boundaries() {
    let build = |smoothing| {
        let mut scene = Scene::new(16, 16);
        let image = checker_image(&mut scene.images);
        scene.add(fill(rect_path(0.0, 0.0, 16.0, 16.0), bitmap_fill(image, true, smoothing, 4.0)));
        scene.render()
    };
    let sharp = build(false);
    let smooth = build(true);
    assert_px(&sharp, 3, 1, [255, 0, 0, 255], 0);
    let edge = pixel(&smooth, 3, 1);
    assert!(edge[0] > 120 && edge[0] < 255 && edge[1] > 40 && edge[1] < 200, "bilinear mixes red and green at the boundary: {edge:?}");
    save("paints_bitmap_fill_smooth", &smooth);
}

fn bitmap_node(image: ImageId, transform: Affine, smoothing: bool) -> Node {
    Node::Bitmap(Bitmap { transform, image, smoothing })
}

#[test]
fn bitmap_node_scales_with_nearest_sampling() {
    let mut scene = Scene::new(16, 16);
    let image = checker_image(&mut scene.images);
    scene.add(bitmap_node(image, Affine::translate((2.0, 2.0)) * Affine::scale(4.0), false));
    let img = scene.render();
    assert_px(&img, 2, 2, [255, 0, 0, 255], 0);
    assert_px(&img, 5, 5, [255, 0, 0, 255], 0);
    assert_px(&img, 6, 2, [0, 255, 0, 255], 0);
    assert_px(&img, 2, 6, [0, 0, 255, 255], 0);
    assert_px(&img, 9, 9, [255, 255, 255, 255], 0);
    assert_px(&img, 1, 2, [255, 255, 255, 255], 0);
    assert_px(&img, 10, 10, [255, 255, 255, 255], 0);
    assert_eq!(count(&img, |px| px != [255, 255, 255, 255]), 8 * 8 - 16, "white texel is invisible on white");
    save("paints_bitmap_node", &img);
}

#[test]
fn bitmap_node_at_one_to_one_is_a_copy() {
    let mut scene = Scene::with_background(4, 4, Some(BLACK));
    let pixels: Vec<u8> = (0..16_u8).flat_map(|i| [i * 10, 255 - i * 10, i, 255]).collect();
    let id = scene.images.insert(Image::new(4, 4, pixels.clone()).unwrap());
    scene.add(bitmap_node(id, Affine::IDENTITY, true));
    assert_eq!(scene.render().premultiplied_rgba, pixels, "smoothing makes no difference at an integer translation");
}

#[test]
fn bitmap_with_alpha_composites_premultiplied() {
    let mut scene = Scene::new(2, 2);
    // Premultiplied 50% blue.
    let id = scene.images.insert(Image::new(1, 1, vec![0, 0, 128, 128]).unwrap());
    scene.add(bitmap_node(id, Affine::scale_non_uniform(2.0, 2.0), false));
    assert_px(&scene.render(), 1, 1, [127, 127, 255, 255], 2);
}

#[test]
fn rotated_bitmap_has_smooth_edges() {
    let mut scene = Scene::new(32, 32);
    let id = scene.images.insert(Image::new(1, 1, vec![255, 0, 0, 255]).unwrap());
    scene.add(bitmap_node(id, Affine::translate((16.0, 6.0)) * Affine::rotate(std::f64::consts::FRAC_PI_4) * Affine::scale(14.0), true));
    let img = scene.render();
    assert_px(&img, 16, 16, [255, 0, 0, 255], 0);
    assert!(count(&img, |px| px[1] > 20 && px[1] < 235) > 10, "anti-aliased diagonal edges");
    save("paints_bitmap_rotated", &img);
}

/// A `size` x `size` image of one-pixel black and white checks, the worst case for aliasing.
fn fine_checker(store: &mut anim_display::ImageStore, size: u32) -> ImageId {
    let mut pixels = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let v = if (x + y) % 2 == 0 { 255 } else { 0 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    store.insert(Image::new(size, size, pixels).unwrap())
}

#[test]
fn heavily_minified_smooth_bitmaps_average_instead_of_aliasing() {
    for scale in [0.25, 0.125, 0.1, 1.0 / 3.0] {
        let mut scene = Scene::new(64, 64);
        let id = fine_checker(&mut scene.images, 240);
        scene.add(bitmap_node(id, Affine::scale(scale), true));
        let img = scene.render();
        let side = ((240.0 * scale).floor() as u32).min(62);
        for y in 1..side - 1 {
            for x in 1..side - 1 {
                assert_px(&img, x, y, [127, 127, 127, 255], 14);
            }
        }
        if scale == 0.125 {
            save("paints_minified_checker", &img);
        }
    }
}

#[test]
fn minified_bitmap_keeps_its_exact_extent() {
    let mut scene = Scene::with_background(40, 40, Some(RED));
    let id = fine_checker(&mut scene.images, 250);
    scene.add(bitmap_node(id, Affine::translate((5.0, 5.0)) * Affine::scale(0.1), true));
    let img = scene.render();
    // 250 * 0.1 = 25 pixels exactly: the edge pixels are fully covered, the ones beyond untouched.
    assert_px(&img, 5, 20, [127, 127, 127, 255], 20);
    assert_px(&img, 29, 20, [127, 127, 127, 255], 20);
    assert_px(&img, 4, 20, [255, 0, 0, 255], 0);
    assert_px(&img, 30, 20, [255, 0, 0, 255], 0);
    assert_px(&img, 20, 4, [255, 0, 0, 255], 0);
    assert_px(&img, 20, 30, [255, 0, 0, 255], 0);
}

#[test]
fn minified_bitmap_fill_and_rotated_bitmap_also_average() {
    let mut scene = Scene::new(40, 40);
    let id = fine_checker(&mut scene.images, 64);
    scene.add(fill(rect_path(0.0, 0.0, 40.0, 40.0), Paint::Bitmap { image: id, transform: Affine::scale(0.2), repeat: true, smoothing: true }));
    let img = scene.render();
    for (x, y) in [(3, 3), (20, 20), (37, 12), (11, 30)] {
        assert_px(&img, x, y, [127, 127, 127, 255], 14);
    }
    let mut rotated = Scene::new(40, 40);
    let id = fine_checker(&mut rotated.images, 160);
    rotated.add(bitmap_node(id, Affine::translate((20.0, 4.0)) * Affine::rotate(std::f64::consts::FRAC_PI_4) * Affine::scale(0.15), true));
    let img = rotated.render();
    assert_px(&img, 20, 18, [127, 127, 127, 255], 14);
    assert_px(&img, 14, 24, [127, 127, 127, 255], 14);
}

#[test]
fn unsmoothed_bitmaps_stay_pixel_exact_when_minified() {
    let mut scene = Scene::new(8, 8);
    let id = fine_checker(&mut scene.images, 16);
    scene.add(bitmap_node(id, Affine::scale(0.5), false));
    let img = scene.render();
    // Nearest sampling: each output pixel is one source pixel, never an average.
    assert_eq!(count(&img, |px| px[0] != 0 && px[0] != 255), 0);
}

#[test]
fn stroke_with_gradient_paint() {
    let mut scene = Scene::new(100, 10);
    let node = Node::Stroke(anim_display::Stroke {
        transform: Affine::IDENTITY,
        path: line_path(0.0, 5.0, 100.0, 5.0),
        style: anim_display::StrokeStyle { width: 6.0, cap: anim_display::Cap::None, ..anim_display::StrokeStyle::default() },
        paint: Paint::Linear(gradient(0.0, 100.0, RED, BLUE)),
    });
    scene.add(node);
    let img = scene.render();
    assert_px(&img, 2, 5, [250, 0, 5, 255], 6);
    assert_px(&img, 97, 5, [5, 0, 250, 255], 6);
    assert_px(&img, 50, 1, [255, 255, 255, 255], 0);
}

#[test]
fn gradient_fill_uses_the_nodes_own_transform() {
    let mut scene = Scene::new(100, 10);
    let node = Fill {
        transform: Affine::translate((50.0, 0.0)),
        path: rect_path(0.0, 0.0, 50.0, 10.0),
        rule: FillRule::NonZero,
        paint: Paint::Linear(gradient(0.0, 50.0, RED, BLUE)),
    };
    scene.add(Node::Fill(node));
    let img = scene.render();
    assert_px(&img, 52, 5, [244, 0, 11, 255], 8);
    assert_px(&img, 97, 5, [5, 0, 250, 255], 8);
    assert_px(&img, 10, 5, [255, 255, 255, 255], 0);
}
