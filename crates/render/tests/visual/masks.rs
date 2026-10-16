//! Fixture `masks_nested`.

use anim_core::Affine;
use anim_display::{Group, Node};

use crate::common::{GREEN, RED, Scene, circle_path, count, fill, pixel, rect_path};
use crate::kit::{assert_color, assert_white, finish, save_crop};

#[test]
fn masks_nested() {
    let disc_mask = Group {
        transform: Affine::translate((10.0, 0.0)),
        children: vec![fill(circle_path(90.0, 100.0, 60.0), anim_display::Paint::Solid(RED))],
        ..Group::default()
    };
    let inner_mask = vec![fill(rect_path(20.0, 0.0, 100.0, 200.0), anim_display::Paint::Solid(RED))];
    let g2 = Group {
        transform: Affine::translate((30.0, 0.0)),
        mask: Some(inner_mask),
        children: vec![fill(rect_path(0.0, 0.0, 200.0, 200.0), anim_display::Paint::Solid(RED))],
        ..Group::default()
    };
    let g1 = Group { mask: Some(vec![Node::Group(disc_mask)]), children: vec![Node::Group(g2)], ..Group::default() };
    let half = Group {
        mask: Some(vec![fill(rect_path(10.0, 150.0, 40.0, 190.0), anim_display::Paint::Solid(anim_core::Rgba8::new(0, 0, 0, 128)))]),
        children: vec![fill(rect_path(0.0, 140.0, 60.0, 200.0), anim_display::Paint::Solid(GREEN))],
        ..Group::default()
    };
    let mut scene = Scene::new(200, 200);
    scene.add(Node::Group(g1));
    scene.add(Node::Group(half));
    let image = scene.render();
    finish("masks_nested", &image);
    save_crop("masks_nested-disc-edge-8x", &image, 40, 30, 40, 40, 12);
    save_crop("masks_nested-half-alpha-8x", &image, 0, 140, 60, 60, 8);

    // P1.
    assert_color("centre", &image, 100, 100, [255, 0, 0]);
    assert_color("left of centre", &image, 60, 100, [255, 0, 0]);
    assert_color("right of centre", &image, 125, 100, [255, 0, 0]);
    // P2, P3.
    assert_white("outside inner mask, left", &image, 45, 100);
    assert_white("outside inner mask, right", &image, 135, 100);
    // P4.
    assert_color("disc top", &image, 100, 45, [255, 0, 0]);
    assert_white("above disc", &image, 100, 38);
    assert_white("below disc", &image, 100, 160);
    assert_color("disc bottom", &image, 100, 157, [255, 0, 0]);
    // P5.
    assert_white("outside disc, inside strip", &image, 52, 50);
    assert_color("inside both", &image, 128, 50, [255, 0, 0]);
    assert_white("just outside disc", &image, 128, 45);
    // P6.
    let soft = count(&image, |p| p[3] == 255 && p[0] == 255 && p[1] > 40 && p[1] < 215);
    assert!(soft >= 30, "P6 soft edge pixels: {soft}");
    // P7.
    assert_white("cut x=50 outside", &image, 49, 100);
    assert_color("cut x=50 inside", &image, 50, 100, [255, 0, 0]);
    assert_color("cut x=130 inside", &image, 129, 100, [255, 0, 0]);
    assert_white("cut x=130 outside", &image, 130, 100);
    // P8.
    assert_white("far corner", &image, 5, 5);
    assert_white("far corner 2", &image, 195, 195);
    // Half-transparent mask: record what the renderer does (see the verdicts).
    let half_px = pixel(&image, 25, 170);
    assert!(half_px == [127, 255, 127, 255] || half_px == [128, 255, 128, 255] || half_px == [0, 255, 0, 255], "half alpha mask pixel {half_px:?}");
}
