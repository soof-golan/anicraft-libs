//! Fixtures `gradients_linear` and `gradients_radial`.

use anim_core::{Affine, Rgba8};
use anim_display::{GRADIENT_EXTENT, Gradient, GradientStop, Interpolation, Paint, Spread};
use anim_shape::{Edge, EdgeShape, FillStyle, GradientFill, NoImages, parse_edges};

use crate::common::{BLACK, BLUE, RED, Scene, WHITE, fill, rect_path};
use crate::kit::{assert_rgb, finish, save_crop};

fn stop(offset: f32, color: Rgba8) -> anim_shape::GradientStop {
    anim_shape::GradientStop { offset, color }
}

fn rect_edge(style: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> Edge {
    let t = |v: f64| v * 20.0;
    let text = format!("!{} {}|{} {}|{} {}|{} {}|{} {}", t(x0), t(y0), t(x1), t(y0), t(x1), t(y1), t(x0), t(y1), t(x0), t(y0));
    Edge::new(0, style, 0, parse_edges(&text).unwrap())
}

#[test]
fn gradients_linear() {
    let matrix = Affine::translate((80.0, 0.0)) * Affine::scale_non_uniform(80.0 / (2.0 * GRADIENT_EXTENT), 1.0);
    let bw = vec![stop(0.0, BLACK), stop(1.0, WHITE)];
    let make = |stops: Vec<anim_shape::GradientStop>, spread: Spread, interpolation: Interpolation| {
        FillStyle::LinearGradient(GradientFill { matrix, stops, spread, interpolation, focal: 0.0 })
    };
    let fills = vec![
        make(bw.clone(), Spread::Pad, Interpolation::Rgb),
        make(bw.clone(), Spread::Reflect, Interpolation::Rgb),
        make(bw.clone(), Spread::Repeat, Interpolation::Rgb),
        make(vec![stop(1.0, WHITE), stop(0.0, BLACK), stop(0.5, RED)], Spread::Pad, Interpolation::Rgb),
        make(vec![stop(0.0, BLACK), stop(0.5, RED), stop(0.5, BLUE), stop(1.0, WHITE)], Spread::Pad, Interpolation::Rgb),
        make(vec![stop(0.25, BLACK), stop(0.75, WHITE)], Spread::Pad, Interpolation::Rgb),
        make(bw.clone(), Spread::Pad, Interpolation::LinearRgb),
        make(bw, Spread::Reflect, Interpolation::Rgb),
    ];
    let edges = (0..8_u32).map(|r| rect_edge(r + 1, 0.0, f64::from(r) * 16.0, 160.0, f64::from(r) * 16.0 + 16.0)).collect();
    let shape = EdgeShape { fills, strokes: Vec::new(), edges };
    let mut scene = Scene::new(160, 128);
    for node in shape.to_display(&NoImages) {
        scene.add(node);
    }
    let image = scene.render();
    finish("gradients_linear", &image);
    save_crop("gradients_linear-hard-stop-12x", &image, 70, 64, 20, 16, 12);

    let g = |v: f64| {
        let v = (v * 255.0).round() as u8;
        [v, v, v]
    };
    let at = |row: u32, x: u32| (x, row * 16 + 8);
    let check = |what: &str, row: u32, x: u32, expected: [u8; 3], tolerance: u8| {
        let (x, y) = at(row, x);
        assert_rgb(&format!("{what} (row {row}, x {x})"), &image, x, y, expected, tolerance);
    };
    // P0 pad.
    check("pad start", 0, 10, g(0.0), 3);
    check("pad end", 0, 150, g(1.0), 3);
    check("pad mid", 0, 80, g(0.50625), 3);
    // P1 reflect.
    check("reflect", 1, 0, g(0.49375), 3);
    check("reflect", 1, 150, g(0.61875), 3);
    check("reflect", 1, 130, g(0.86875), 3);
    // P2 repeat.
    check("repeat", 2, 0, g(0.50625), 3);
    check("repeat", 2, 150, g(0.38125), 3);
    check("repeat", 2, 130, g(0.13125), 3);
    // P3 sorted stops.
    check("sorted stops", 3, 60, [131, 0, 0], 3);
    check("sorted stops", 3, 100, [255, 131, 131], 3);
    // P4 hard stop.
    check("hard stop", 4, 76, [233, 0, 0], 3);
    check("hard stop", 4, 84, [29, 29, 255], 3);
    check("before the step", 4, 79, [250, 0, 5], 12);
    check("after the step", 4, 80, [5, 5, 255], 12);
    // P5 partial stops.
    check("pad before first stop", 5, 50, [0, 0, 0], 3);
    check("pad after last stop", 5, 110, [255, 255, 255], 3);
    check("partial mid", 5, 80, g(0.5125), 3);
    // P6 linear RGB.
    check("linear RGB mid", 6, 80, g(0.7395), 4);
    // P7.
    check("reflect 2", 7, 20, g(0.24375), 3);
}

#[test]
fn gradients_radial() {
    let red_blue = vec![GradientStop { offset: 0.0, color: RED }, GradientStop { offset: 1.0, color: BLUE }];
    let cell = |col: u32, row: u32, matrix: Affine, spread: Spread, focal: f64| {
        let (x0, y0) = (f64::from(col) * 100.0, f64::from(row) * 100.0);
        let gradient = Gradient {
            transform: Affine::translate((x0 + 50.0, y0 + 50.0)) * matrix,
            stops: red_blue.clone(),
            spread,
            interpolation: Interpolation::Rgb,
            focal,
        };
        fill(rect_path(x0, y0, x0 + 100.0, y0 + 100.0), Paint::Radial(gradient))
    };
    let r = |radius: f64| Affine::scale(radius / GRADIENT_EXTENT);
    let ellipse = Affine::scale_non_uniform(40.0 / GRADIENT_EXTENT, 20.0 / GRADIENT_EXTENT);
    let mut scene = Scene::new(300, 200);
    scene.add(cell(0, 0, r(25.0), Spread::Pad, 0.0));
    scene.add(cell(1, 0, r(25.0), Spread::Reflect, 0.0));
    scene.add(cell(2, 0, r(25.0), Spread::Repeat, 0.0));
    scene.add(cell(0, 1, r(40.0), Spread::Pad, 0.5));
    scene.add(cell(1, 1, ellipse, Spread::Pad, 0.0));
    scene.add(cell(2, 1, Affine::rotate(std::f64::consts::FRAC_PI_2) * ellipse, Spread::Pad, 0.0));
    let image = scene.render();
    finish("gradients_radial", &image);
    save_crop("gradients_radial-focal-6x", &image, 0, 100, 100, 100, 6);

    let at = |what: &str, x: u32, y: u32, t: f64, tolerance: u8| {
        let expected = [((1.0 - t) * 255.0).round() as u8, 0, (t * 255.0).round() as u8];
        assert_rgb(what, &image, x, y, expected, tolerance);
    };
    // P1..P3.
    at("pad t=0.50", 62, 50, 0.50, 8);
    at("pad beyond", 90, 50, 1.0, 3);
    at("pad centre", 50, 50, 0.02, 8);
    at("reflect", 190, 50, 0.38, 8);
    at("repeat", 290, 50, 0.62, 8);
    // P4 focal.
    let (r_focal, _, _) = (crate::common::pixel(&image, 70, 150)[0], 0, 0);
    assert!(r_focal > 240, "focal point is the first stop, got red {r_focal}");
    at("focal right", 80, 150, 0.525, 8);
    at("focal left", 40, 150, 0.49, 8);
    let (a, b) = (crate::common::pixel(&image, 80, 150)[0], crate::common::pixel(&image, 40, 150)[0]);
    assert!(a.abs_diff(b) < 15, "equal progress from the focus gives equal colours: {a} vs {b}");
    // P5 ellipse.
    at("ellipse y", 150, 160, 0.525, 8);
    at("ellipse x", 170, 150, 0.5125, 8);
    at("ellipse beyond", 150, 175, 1.0, 3);
    // P6 rotated ellipse.
    at("rotated x", 260, 150, 0.525, 8);
    at("rotated y", 250, 170, 0.5125, 8);
    at("rotated beyond y", 250, 195, 1.0, 3);
    at("rotated beyond x", 275, 150, 1.0, 3);
}
