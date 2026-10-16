//! Fixtures `stroke_hairline`, `stroke_huge`, `stroke_joins` and `stroke_scale`.

use anim_core::{Affine, BezPath};
use anim_display::{Cap, Group, Join, Node, StrokeScale, StrokeStyle};

use crate::common::{BLACK, Scene, line_path, pixel, rect_path, stroke};
use crate::kit::{assert_close, assert_white, finish, ink, ink_sum, save_crop};

fn style(width: f64, cap: Cap, join: Join, miter_limit: f64, scale: StrokeScale) -> StrokeStyle {
    StrokeStyle { width, cap, join, miter_limit, scale, dashes: Vec::new() }
}

fn polyline(points: &[(f64, f64)]) -> BezPath {
    let mut path = BezPath::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(*p);
        } else {
            path.line_to(*p);
        }
    }
    path
}

#[test]
fn stroke_hairline() {
    let mut scene = Scene::new(160, 100);
    let plain = |y: f64, width: f64| stroke(line_path(10.0, y, 150.0, y), style(width, Cap::None, Join::Round, 3.0, StrokeScale::Normal), BLACK);
    scene.add(plain(10.5, 1.0));
    scene.add(plain(20.0, 1.0));
    scene.add(plain(30.5, 0.0));
    scene.add(plain(40.0, 0.0));
    let scaled = |y: f64, width: f64, mode: StrokeScale| stroke(line_path(2.5, y, 37.5, y), style(width, Cap::None, Join::Round, 3.0, mode), BLACK);
    scene.add(Node::Group(Group {
        transform: Affine::scale(4.0),
        children: vec![
            scaled(13.125, 0.0, StrokeScale::Normal),
            scaled(16.125, 1.0, StrokeScale::None),
            scaled(20.0, 1.0, StrokeScale::Normal),
            scaled(23.125, 0.0, StrokeScale::Normal),
        ],
        ..Group::default()
    }));
    let image = scene.render();
    finish("stroke_hairline", &image);
    save_crop("stroke_hairline-left-12x", &image, 0, 0, 30, 100, 8);

    let sum = |row: u32, below: u32, above: u32| ink_sum(&image, 80..81, row - below..row + above);
    assert_close("P1 coverage", sum(10, 4, 5), 1.0, 0.05);
    assert!(ink(&image, 80, 10) > 0.99 && ink(&image, 80, 9) < 0.01 && ink(&image, 80, 11) < 0.01, "P1 pixel aligned");
    assert_close("P2 coverage", sum(20, 4, 4), 1.0, 0.05);
    for row in [19, 20] {
        let a = pixel(&image, 80, row)[3];
        assert!(a == 255, "background is opaque");
        assert_close("P2 half row", ink(&image, 80, row), 0.5, 0.03);
    }
    assert_close("P3 coverage", sum(30, 4, 5), 1.0, 0.15);
    assert!(ink(&image, 80, 29) < 0.08 && ink(&image, 80, 31) < 0.08, "P3 neighbours: {} {}", ink(&image, 80, 29), ink(&image, 80, 31));
    assert_close("P4 coverage", sum(40, 4, 4), 1.0, 0.15);
    assert_close("P5 coverage", sum(52, 4, 5), 1.0, 0.15);
    assert_close("P6 coverage", sum(64, 4, 5), 1.0, 0.05);
    assert!(ink(&image, 80, 64) > 0.99, "P6 row fully black");
    assert_close("P7 coverage", ink_sum(&image, 80..81, 74..86), 4.0, 0.05);
    for row in 78..82 {
        assert!(ink(&image, 80, row) > 0.99, "P7 row {row}");
    }
    assert!(ink(&image, 80, 77) < 0.01 && ink(&image, 80, 82) < 0.01, "P7 edges");
    assert_close("P8 coverage", sum(92, 4, 4), 1.0, 0.15);
    assert_white("P9 left end", &image, 9, 10);
    assert_white("P9 right end", &image, 150, 10);
}

#[test]
fn stroke_huge() {
    let mut scene = Scene::new(400, 440);
    scene.add(stroke(line_path(100.0, 100.0, 300.0, 100.0), style(200.0, Cap::Round, Join::Round, 3.0, StrokeScale::Normal), BLACK));
    scene.add(stroke(line_path(100.0, 330.0, 300.0, 330.0), style(200.0, Cap::None, Join::Round, 3.0, StrokeScale::Normal), BLACK));
    let image = scene.render();
    finish("stroke_huge", &image);
    save_crop("stroke_huge-cap-corner-6x", &image, 0, 0, 60, 60, 8);

    for (x, y) in [(100, 100), (200, 5), (200, 195), (5, 100), (395, 100), (60, 40), (30, 100)] {
        assert!(ink(&image, x, y) > 0.99, "P1 black at ({x}, {y})");
    }
    for (x, y) in [(10, 10), (390, 10), (10, 190), (390, 190), (3, 60)] {
        assert!(ink(&image, x, y) < 0.01, "P1/P3 white at ({x}, {y})");
    }
    for (x, y) in [(200, 235), (200, 425), (101, 330), (299, 330)] {
        assert!(ink(&image, x, y) > 0.99, "P2 black at ({x}, {y})");
    }
    for (x, y) in [(99, 330), (301, 330), (50, 330), (200, 229), (200, 431), (200, 215)] {
        assert!(ink(&image, x, y) < 0.01, "P2/P4 white at ({x}, {y})");
    }
}

#[test]
fn stroke_joins() {
    let mut scene = Scene::new(520, 200);
    let arm = 80.0;
    let (dx, dy) = (arm * 10.0_f64.to_radians().cos(), arm * 10.0_f64.to_radians().sin());
    let chevron = |k: u32, join: Join, limit: f64| {
        let ax = f64::from(130 * k + 30);
        stroke(polyline(&[(ax + dx, 50.0 - dy), (ax, 50.0), (ax + dx, 50.0 + dy)]), style(10.0, Cap::None, join, limit, StrokeScale::Normal), BLACK)
    };
    scene.add(chevron(0, Join::Miter, 5.5));
    scene.add(chevron(1, Join::Miter, 6.0));
    scene.add(chevron(2, Join::Round, 3.0));
    scene.add(chevron(3, Join::Bevel, 3.0));
    let row2 = |points: &[(f64, f64)], join: Join, limit: f64| stroke(polyline(points), style(10.0, Cap::None, join, limit, StrokeScale::Normal), BLACK);
    scene.add(row2(&[(10.0, 150.0), (60.0, 150.0), (110.0, 152.0)], Join::Miter, 3.0));
    scene.add(row2(&[(140.0, 150.0), (190.0, 150.0), (140.0, 150.0)], Join::Round, 3.0));
    scene.add(row2(&[(270.0, 150.0), (320.0, 150.0), (270.0, 150.0)], Join::Miter, 3.0));
    scene.add(row2(&[(420.0, 110.0), (420.0, 170.0), (480.0, 170.0)], Join::Miter, 2.0));
    let image = scene.render();
    finish("stroke_joins", &image);
    for k in 0..4_u32 {
        save_crop(&format!("stroke_joins-chevron{k}-8x"), &image, 130 * k, 30, 60, 40, 10);
    }
    save_crop("stroke_joins-bend-8x", &image, 40, 135, 40, 30, 12);
    save_crop("stroke_joins-hairpins-8x", &image, 130, 135, 210, 30, 4);
    save_crop("stroke_joins-right-angle-12x", &image, 405, 150, 40, 30, 12);

    let tip = |k: u32, dxp: i32| {
        let ax = 130 * k + 30;
        let x = u32::try_from(i32::try_from(ax).unwrap() + dxp).unwrap();
        (ink(&image, x, 49), ink(&image, x, 50))
    };
    // P1.
    let (a, b) = tip(1, -20);
    assert!(a > 0.78 && b > 0.78, "P1 miter tip pixels {a} {b}");
    for k in [0, 2, 3] {
        let (a, b) = tip(k, -20);
        assert!(a < 0.02 && b < 0.02, "P1 no miter for chevron {k}: {a} {b}");
    }
    // P2.
    for k in [0, 3] {
        assert!(tip(k, -2).0 < 0.02, "P2 bevel is flat {k}");
        assert!(tip(k, 1).0 > 0.9, "P2 body {k}");
    }
    // P3.
    assert!(tip(2, -4).0 > 0.9 && tip(2, -6).0 < 0.02, "P3 round");
    // P4.
    for x in [59, 60, 61] {
        assert_close("P4 bend coverage", ink_sum(&image, x..x + 1, 140..160), 10.0, 0.15);
    }
    assert!(ink(&image, 60, 150) > 0.99, "P4 centre");
    // P5, P6.
    assert!(ink(&image, 193, 149) > 0.9 && ink(&image, 196, 149) < 0.02, "P5 round hairpin tip");
    assert!(ink(&image, 138, 149) < 0.02 && ink(&image, 142, 149) > 0.9, "P5 flat start");
    assert!(ink(&image, 318, 149) > 0.9 && ink(&image, 322, 149) < 0.02, "P6 bevelled hairpin");
    // P7.
    assert!(ink(&image, 415, 174) > 0.94, "P7 outer corner {}", ink(&image, 415, 174));
    assert!(ink(&image, 425, 164) < 0.02, "P7 inner corner");
}

#[test]
fn stroke_scale() {
    let mut scene = Scene::new(340, 120);
    let cells = [(0.0, 0.0, StrokeScale::Normal), (170.0, 0.0, StrokeScale::None), (0.0, 60.0, StrokeScale::Horizontal), (170.0, 60.0, StrokeScale::Vertical)];
    for (cx, cy, mode) in cells {
        scene.add(Node::Group(Group {
            transform: Affine::translate((cx, cy)) * Affine::scale_non_uniform(3.0, 0.5),
            children: vec![stroke(rect_path(4.0, 20.0, 48.0, 100.0), style(6.0, Cap::None, Join::Miter, 4.0, mode), BLACK)],
            ..Group::default()
        }));
    }
    let image = scene.render();
    finish("stroke_scale", &image);
    save_crop("stroke_scale-corners-8x", &image, 0, 0, 40, 30, 10);

    // (vertical bar width, horizontal bar height)
    let expected = [(18.0, 3.0), (6.0, 6.0), (6.0, 3.0), (18.0, 6.0)];
    for ((cx, cy, mode), (v_expected, h_expected)) in cells.into_iter().zip(expected) {
        let (cx, cy) = (cx as u32, cy as u32);
        let v = ink_sum(&image, cx..cx + 30, cy + 30..cy + 31);
        let h = ink_sum(&image, cx + 80..cx + 81, cy..cy + 30);
        assert_close(&format!("{mode:?} vertical bar width"), v, v_expected, 0.25);
        assert_close(&format!("{mode:?} horizontal bar height"), h, h_expected, 0.25);
        // P1: the centre lines are in the same place whatever the mode.
        let weighted_x: f64 = (cx..cx + 30).map(|x| ink(&image, x, cy + 30) * (f64::from(x) + 0.5 - f64::from(cx))).sum();
        assert_close(&format!("{mode:?} left bar centre"), weighted_x / v, 12.0, 0.1);
        let weighted_y: f64 = (cy..cy + 30).map(|y| ink(&image, cx + 80, y) * (f64::from(y) + 0.5 - f64::from(cy))).sum();
        assert_close(&format!("{mode:?} top bar centre"), weighted_y / h, 10.0, 0.1);
    }
    // P2: mitred corner.
    assert!(ink(&image, 3, 9) > 0.99, "P2 sharp corner");
}
