//! Perspective groups: a flat square rotated in 3D, with the corners predicted numerically from the pinhole model.

use anim_core::{Matrix3D, Perspective, Point};
use anim_display::{Group, Node};

use crate::common::*;

/// The screen position of a 3D point under a pinhole at distance `f` from the stage, looking at `vanishing`.
fn pinhole(p: [f64; 3], f: f64, vanishing: Point) -> Point {
    let s = f / (f + p[2]);
    Point::new(vanishing.x + (p[0] - vanishing.x) * s, vanishing.y + (p[1] - vanishing.y) * s)
}

fn area(quad: &[Point; 4]) -> f64 {
    let mut twice = 0.0;
    for i in 0..4 {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        twice += a.x * b.y - b.x * a.y;
    }
    twice.abs() / 2.0
}

#[test]
fn a_square_turned_about_y_lands_on_the_predicted_quad() {
    let perspective = Perspective::for_stage(200.0, 200.0);
    let f = perspective.focal_length();
    let placement = Matrix3D::translation(-50.0, -50.0, 0.0).then(&Matrix3D::rotation_y(40.0)).then(&Matrix3D::translation(100.0, 100.0, 0.0));
    let quad = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)].map(|(x, y)| pinhole(placement.transform_point([x, y, 0.0]), f, perspective.vanishing_point));
    // One edge is nearer the viewer, so it must be taller on screen than the other.
    let left = quad[0].distance(quad[3]);
    let right = quad[1].distance(quad[2]);
    assert!((left - right).abs() > 5.0, "{left} {right}");

    let mut scene = Scene::new(200, 200);
    let group = Group { projection: Some(placement.homography(&perspective)), children: vec![solid_rect(0.0, 0.0, 100.0, 100.0, RED)], ..Group::default() };
    scene.add(Node::Group(group));
    let img = scene.render();
    // Red over white: the missing green is the coverage, so edge pixels count fractionally.
    let mut red = 0.0;
    for y in 0..200 {
        for x in 0..200 {
            red += f64::from(255 - pixel(&img, x, y)[1]) / 255.0;
        }
    }
    let expected = area(&quad);
    assert!((red - expected).abs() / expected < 0.01, "red {red} vs predicted {expected}");
    // The centre of the quad is red; a point past the nearer edge is not.
    let centre = Point::new(quad.iter().map(|p| p.x).sum::<f64>() / 4.0, quad.iter().map(|p| p.y).sum::<f64>() / 4.0);
    assert_px(&img, centre.x as u32, centre.y as u32, [255, 0, 0, 255], 0);
    let min_x = quad.iter().map(|p| p.x).fold(f64::MAX, f64::min);
    assert_px(&img, (min_x - 3.0) as u32, 100, [255, 255, 255, 255], 0);
    save("perspective_rotate_y", &img);
}

#[test]
fn moving_back_in_z_shrinks_toward_the_vanishing_point() {
    let perspective = Perspective::for_stage(200.0, 200.0);
    let f = perspective.focal_length();
    let z = f; // twice the distance: half the size
    let placement = Matrix3D::translation(60.0, 60.0, z);
    let mut scene = Scene::new(200, 200);
    scene.add(Node::Group(Group { projection: Some(placement.homography(&perspective)), children: vec![solid_rect(0.0, 0.0, 40.0, 40.0, BLUE)], ..Group::default() }));
    let img = scene.render();
    // (60,60)..(100,100) shrinks about (100,100) by one half: (80,80)..(100,100).
    assert_eq!(count(&img, |px| px == [0, 0, 255, 255]), 400);
    assert_px(&img, 85, 85, [0, 0, 255, 255], 0);
    assert_px(&img, 70, 70, [255, 255, 255, 255], 0);
}
