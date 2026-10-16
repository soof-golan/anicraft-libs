//! Fixture `curves`: exact cubics, quadratic circles and the XFL `cubics` attribute round trip.

use anim_core::Point;
use anim_core::kurbo::PathEl;
use anim_shape::{Edge, EdgeShape, FillStyle, NoImages, Segment};

use crate::common::{BLACK, Scene, max_diff};
use crate::kit::{assert_close, finish, ink, ink_sum, save_crop};

fn p(x: f64, y: f64) -> Point {
    Point::new(x * 20.0, y * 20.0)
}

fn shape(segments: Vec<Segment>) -> EdgeShape {
    EdgeShape { fills: vec![FillStyle::Solid(BLACK)], strokes: Vec::new(), edges: vec![Edge::new(0, 1, 0, segments)] }
}

fn cubic_circle(cx: f64, cy: f64, r: f64) -> EdgeShape {
    let k = 0.552_284_749_8 * r;
    let q = |x: f64, y: f64| p(cx + x, cy + y);
    shape(vec![
        Segment::Cubic { from: q(r, 0.0), ctrl1: q(r, k), ctrl2: q(k, r), to: q(0.0, r) },
        Segment::Cubic { from: q(0.0, r), ctrl1: q(-k, r), ctrl2: q(-r, k), to: q(-r, 0.0) },
        Segment::Cubic { from: q(-r, 0.0), ctrl1: q(-r, -k), ctrl2: q(-k, -r), to: q(0.0, -r) },
        Segment::Cubic { from: q(0.0, -r), ctrl1: q(k, -r), ctrl2: q(r, -k), to: q(r, 0.0) },
    ])
}

fn quad_circle(cx: f64, cy: f64, r: f64) -> EdgeShape {
    let control = r / 22.5_f64.to_radians().cos();
    let at = |angle: f64, radius: f64| p(radius.mul_add(angle.to_radians().cos(), cx), radius.mul_add(angle.to_radians().sin(), cy));
    shape(
        (0..8)
            .map(|i| {
                let a = 45.0 * f64::from(i);
                Segment::Quad { from: at(a, r), ctrl: at(a + 22.5, control), to: at(a + 45.0, r) }
            })
            .collect(),
    )
}

fn s_curve(dx: f64) -> EdgeShape {
    let from = p(10.0 + dx, 90.0);
    let to = p(90.0 + dx, 10.0);
    shape(vec![Segment::Cubic { from, ctrl1: p(90.0 + dx, 90.0), ctrl2: p(10.0 + dx, 10.0), to }, Segment::Line { from: to, to: from }])
}

fn render(shapes: &[&EdgeShape]) -> anim_display::Image {
    let mut scene = Scene::new(300, 100);
    for shape in shapes {
        for node in shape.to_display(&NoImages) {
            scene.add(node);
        }
    }
    scene.render()
}

fn check_disc(image: &anim_display::Image, cx: f64, cy: f64, margin: f64, area_tolerance: f64) {
    let (x0, y0) = (cx as u32 - 50, 0);
    for y in y0..y0 + 100 {
        for x in x0..x0 + 100 {
            let d = (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy);
            let v = ink(image, x, y);
            if d < 40.0 - margin {
                assert!(v >= 0.98, "inside pixel ({x}, {y}) at {d:.2} has ink {v:.3}");
            } else if d > 40.0 + margin {
                assert!(v <= 0.02, "outside pixel ({x}, {y}) at {d:.2} has ink {v:.3}");
            }
        }
    }
    let area = ink_sum(image, x0..x0 + 100, 0..100);
    let exact = std::f64::consts::PI * 1600.0;
    assert_close("disc area", area, exact, exact * area_tolerance);
}

#[test]
fn curves() {
    let a = cubic_circle(50.0, 50.0, 40.0);
    let b = quad_circle(150.0, 50.0, 40.0);
    let c = s_curve(200.0);
    let image = render(&[&a, &b, &c]);
    finish("curves", &image);
    save_crop("curves-circle-edge-12x", &image, 80, 5, 30, 20, 12);
    save_crop("curves-s-curve-4x", &image, 200, 0, 100, 100, 6);

    // P1, P2.
    check_disc(&image, 50.0, 50.0, 1.0, 0.0025);
    check_disc(&image, 150.0, 50.0, 1.2, 0.01);
    // P3.
    let mut worst = (0_u8, 0, 0);
    for y in 0..100 {
        for x in 0..100 {
            let (u, v) = (crate::common::pixel(&image, x, y), crate::common::pixel(&image, x + 100, y));
            let d = u.iter().zip(v).map(|(s, t)| s.abs_diff(t)).max().unwrap_or(0);
            if d > worst.0 {
                worst = (d, x, y);
            }
        }
    }
    assert!(
        worst.0 <= 72,
        "P3: circles A and B differ by {} at ({}, {}): A {:?}, B {:?}",
        worst.0,
        worst.1,
        worst.2,
        crate::common::pixel(&image, worst.1, worst.2),
        crate::common::pixel(&image, worst.1 + 100, worst.2)
    );
    // P4: the lobes of the S shape (cell offset 200).
    let ink_at = |x: f64, y: f64| ink(&image, x.floor() as u32, y.floor() as u32);
    assert!(ink_at(242.9, 76.1) > 0.9, "P4 inside lobe 1: {}", ink_at(242.9, 76.1));
    assert!(ink_at(247.1, 78.9) < 0.1, "P4 outside lobe 1: {}", ink_at(247.1, 78.9));
    assert!(ink_at(257.1, 23.9) > 0.9, "P4 inside lobe 2: {}", ink_at(257.1, 23.9));
    assert!(ink_at(252.9, 21.1) < 0.1, "P4 outside lobe 2: {}", ink_at(252.9, 21.1));
    // P5: the assembled path keeps its cubic.
    let assembled = c.assemble();
    let path = &assembled.fills.first().unwrap().path;
    assert!(path.elements().iter().any(|e| matches!(e, PathEl::CurveTo(..))), "P5 cubic kept");
    // P6: XFL round trip and the cubics-less variant.
    let xfl = c.to_xfl_edges();
    assert!(xfl.iter().any(|e| e.cubics.is_some()), "a cubic writes a cubics attribute");
    let back = EdgeShape::from_xfl(c.fills.clone(), Vec::new(), &xfl).unwrap();
    let again = render(&[&a, &b, &back]);
    assert!(max_diff(&image, &again) <= 2, "P6 round trip differs by {}", max_diff(&image, &again));
    let stripped: Vec<_> = xfl
        .iter()
        .cloned()
        .map(|mut e| {
            e.cubics = None;
            e
        })
        .collect();
    let quads_only = EdgeShape::from_xfl(c.fills, Vec::new(), &stripped).unwrap();
    let approx = render(&[&a, &b, &quads_only]);
    assert!(max_diff(&image, &approx) <= 66, "P6 quadratic approximation differs by {}", max_diff(&image, &approx));
    let moved = image
        .premultiplied_rgba
        .chunks_exact(4)
        .zip(approx.premultiplied_rgba.chunks_exact(4))
        .filter(|(s, t)| s.iter().zip(*t).any(|(a, b)| a.abs_diff(*b) > 10))
        .count();
    assert!(moved < 60, "P6 only edge pixels differ, {moved} did");
}
