//! Fixture `color_alpha`.

use anim_core::{ColorTransform, Rgba8};
use anim_display::{Group, Node, Paint};

use crate::common::{BLUE, Scene, fill, rect_path};
use crate::kit::{assert_rgb, assert_white, finish};

fn cell_rect(col: u32, row: u32, x0: f64, y0: f64, x1: f64, y1: f64, color: Rgba8) -> Node {
    let (ox, oy) = (f64::from(col) * 60.0, f64::from(row) * 60.0);
    fill(rect_path(ox + x0, oy + y0, ox + x1, oy + y1), Paint::Solid(color))
}

fn wrap(color: ColorTransform, children: Vec<Node>) -> Node {
    Node::Group(Group { color, children, ..Group::default() })
}

fn ct(mul: [f32; 4], add: [f32; 4]) -> ColorTransform {
    ColorTransform { mul, add }
}

#[test]
fn color_alpha() {
    let mut scene = Scene::new(180, 120);
    // 1.
    let b = wrap(ct([0.5, 0.5, 0.5, 1.0], [0.2, 0.0, 0.0, 0.0]), vec![cell_rect(0, 0, 10.0, 10.0, 50.0, 50.0, Rgba8::opaque(200, 100, 50))]);
    scene.add(wrap(ColorTransform::alpha(0.5), vec![b]));
    // 2.
    let b = wrap(ct([1.0; 4], [0.0, 0.0, 0.0, 0.25]), vec![cell_rect(1, 0, 10.0, 10.0, 50.0, 50.0, Rgba8::new(200, 100, 50, 128))]);
    scene.add(wrap(ColorTransform::IDENTITY, vec![b]));
    // 3.
    let red_half = Rgba8::new(255, 0, 0, 128);
    scene.add(wrap(ColorTransform::alpha(0.5), vec![cell_rect(2, 0, 10.0, 10.0, 40.0, 40.0, red_half), cell_rect(2, 0, 25.0, 25.0, 55.0, 55.0, red_half)]));
    // 4.
    scene.add(wrap(ct([1.0; 4], [0.0, 0.0, 0.0, 1.0]), vec![cell_rect(0, 1, 15.0, 15.0, 45.0, 45.0, Rgba8::new(200, 100, 50, 64))]));
    // 5.
    scene.add(wrap(ct([0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0]), vec![cell_rect(1, 1, 10.0, 10.0, 50.0, 50.0, BLUE)]));
    // 6.
    scene.add(wrap(ct([2.0, 2.0, 2.0, 1.0], [0.5, 0.5, 0.5, 0.0]), vec![cell_rect(2, 1, 10.0, 10.0, 50.0, 50.0, Rgba8::opaque(200, 100, 50))]));
    let image = scene.render();
    finish("color_alpha", &image);

    assert_rgb("P1 nested colour transforms", &image, 30, 30, [203, 152, 140], 3);
    assert_rgb("P2 alpha offset on a half-transparent fill", &image, 90, 30, [214, 138, 101], 3);
    assert_rgb("P3 overlap is composited as a unit", &image, 120 + 32, 32, [255, 159, 159], 3);
    assert_rgb("P3 first rectangle only", &image, 120 + 15, 15, [255, 191, 191], 3);
    assert_rgb("P3 second rectangle only", &image, 120 + 50, 50, [255, 191, 191], 3);
    assert_rgb("P4 opaque after alpha offset", &image, 30, 90, [200, 100, 50], 4);
    assert_white("P4 transparent stays transparent", &image, 2, 62);
    assert_rgb("P5 tint to red", &image, 90, 90, [255, 0, 0], 2);
    assert_rgb("P6 clamping", &image, 150, 90, [255, 255, 227], 3);
}
