//! Fixture `mesh_folds`: meshes whose triangles are folded over, in both painting orders, plus invalid triangles.

use anim_core::{Affine, Point, Rgba8};
use anim_display::{Mesh, Node, Paint};

use crate::common::{BLACK, BLUE, GREEN, RED, Scene, WHITE, fill, gradient, max_diff, pixel, rect_path};
use crate::kit::{assert_close, finish, save_crop};

const YELLOW: Rgba8 = Rgba8::opaque(255, 255, 0);

fn stripes() -> Vec<Node> {
    [RED, GREEN, BLUE, YELLOW].iter().enumerate().map(|(i, c)| fill(rect_path(10.0 * i as f64, 0.0, 10.0 * i as f64 + 10.0, 40.0), Paint::Solid(*c))).collect()
}

fn pts(list: &[(f64, f64)]) -> Vec<Point> {
    list.iter().map(|&(x, y)| Point::new(x, y)).collect()
}

fn folded(offset_x: f64, left_first: bool, extra: bool) -> Node {
    let source = pts(&[(0.0, 0.0), (20.0, 0.0), (40.0, 0.0), (0.0, 40.0), (20.0, 40.0), (40.0, 40.0)]);
    let target = pts(&[(0.0, 0.0), (50.0, 0.0), (40.0, 0.0), (0.0, 40.0), (50.0, 40.0), (40.0, 40.0)]);
    let left = [[0, 1, 4], [0, 4, 3]];
    let right = [[1, 2, 5], [1, 5, 4]];
    let mut triangles: Vec<[u32; 3]> = if left_first { [left, right].concat() } else { [right, left].concat() };
    if extra {
        triangles.push([0, 1, 1]);
        triangles.push([0, 1, 99]);
    }
    Node::Mesh(Mesh { transform: Affine::translate((offset_x, 15.0)), content: stripes(), source, target, triangles })
}

fn stretched(offset_x: f64) -> Node {
    let source = pts(&[(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)]);
    let target = pts(&[(0.0, 0.0), (80.0, 0.0), (80.0, 40.0), (0.0, 40.0)]);
    let content = vec![fill(rect_path(0.0, 0.0, 40.0, 40.0), Paint::Linear(gradient(0.0, 40.0, BLACK, WHITE)))];
    Node::Mesh(Mesh { transform: Affine::translate((offset_x, 15.0)), content, source, target, triangles: vec![[0, 1, 2], [0, 2, 3]] })
}

fn within(what: &str, image: &anim_display::Image, x: u32, y: u32, expected: [u8; 4], tolerance: u8) {
    let got = pixel(image, x, y);
    assert!(got.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= tolerance), "{what}: pixel ({x}, {y}) is {got:?}, expected {expected:?}");
}

#[test]
fn mesh_folds() {
    let mut scene = Scene::with_background(280, 70, None);
    scene.add(folded(10.0, true, false));
    scene.add(folded(100.0, false, false));
    scene.add(stretched(190.0));
    let image = scene.render_transparent();
    finish("mesh_folds", &image);
    save_crop("mesh_folds-fold-m1-10x", &image, 0, 10, 80, 50, 10);
    save_crop("mesh_folds-fold-m2-10x", &image, 90, 10, 80, 50, 10);

    // P1: left quad first, the mirrored right quad over it.
    within("M1 red", &image, 20, 35, [255, 0, 0, 255], 4);
    within("M1 green", &image, 45, 35, [0, 255, 0, 255], 4);
    within("M1 yellow", &image, 52, 35, [255, 255, 0, 255], 4);
    within("M1 blue", &image, 58, 35, [0, 0, 255, 255], 4);
    within("M1 outside", &image, 65, 35, [0, 0, 0, 0], 0);
    // P2: right quad first, the left quad over it.
    within("M2 red", &image, 110, 35, [255, 0, 0, 255], 4);
    within("M2 green", &image, 135, 35, [0, 255, 0, 255], 4);
    within("M2 green over the fold", &image, 142, 35, [0, 255, 0, 255], 4);
    within("M2 green over the fold 2", &image, 148, 35, [0, 255, 0, 255], 4);
    within("M2 outside", &image, 165, 35, [0, 0, 0, 0], 0);
    // P3: stretched gradient.
    for (target_x, grey) in [(20_u32, 64.0), (40, 128.0), (60, 191.0)] {
        let got = f64::from(pixel(&image, 190 + target_x, 35)[0]);
        assert_close(&format!("M3 grey at {target_x}"), got, grey, 8.0);
    }
    within("M3 beyond", &image, 190 + 85, 35, [0, 0, 0, 0], 0);
    // P5: no leaks.
    within("M1 above", &image, 30, 5, [0, 0, 0, 0], 0);
    within("M1 below", &image, 30, 62, [0, 0, 0, 0], 0);
    within("M1 right of the fold", &image, 62, 35, [0, 0, 0, 0], 0);

    // P4: invalid triangles are ignored.
    let mut clean = Scene::with_background(280, 70, None);
    clean.add(folded(10.0, true, false));
    let mut dirty = Scene::with_background(280, 70, None);
    dirty.add(folded(10.0, true, true));
    let (a, b) = (clean.render_transparent(), dirty.render_transparent());
    assert_eq!(max_diff(&a, &b), 0, "P4 bad triangles must not change the render");
}
