//! Coarse timing guards: not benchmarks, just protection against accidentally quadratic or pathological paths.

use std::time::{Duration, Instant};

use anim_core::{Affine, ColorTransform};
use anim_display::{Filter, Group, Node};
use anim_render::{RenderOptions, Renderer};

use crate::common::*;

fn timed(label: &str, mut run: impl FnMut()) -> Duration {
    run();
    let start = Instant::now();
    run();
    let elapsed = start.elapsed();
    println!("{label}: {elapsed:?}");
    elapsed
}

#[test]
fn full_hd_frame_with_many_shapes_is_quick() {
    let mut scene = Scene::new(1920, 1080);
    for i in 0..400 {
        let x = f64::from(i % 40) * 48.0;
        let y = f64::from(i / 40) * 100.0;
        scene.add(solid_circle(x + 24.0, y + 24.0, 22.0, if i % 2 == 0 { RED } else { BLUE }));
        scene.add(stroke(line_path(x, y, x + 40.0, y + 60.0), anim_display::StrokeStyle { width: 3.0, ..anim_display::StrokeStyle::default() }, BLACK));
    }
    let mut renderer = Renderer::new();
    let options = RenderOptions::new(1920, 1080);
    let elapsed = timed("1920x1080, 800 shapes", || {
        let img = renderer.render(&scene.list, &scene.images, &options).unwrap();
        renderer.recycle(img);
    });
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
}

#[test]
fn full_hd_blur_and_colour_transform_are_quick() {
    let mut scene = Scene::new(1920, 1080);
    let content = vec![solid_rect(200.0, 200.0, 1700.0, 900.0, RED), solid_circle(960.0, 540.0, 300.0, BLUE)];
    scene.add(Node::Group(Group {
        filters: vec![Filter::Blur { x: 24.0, y: 24.0, quality: 3 }],
        color: ColorTransform::alpha(0.8),
        children: content,
        ..Group::default()
    }));
    let mut renderer = Renderer::new();
    let options = RenderOptions::new(1920, 1080);
    let elapsed = timed("1920x1080, blur 24 x3 + colour transform", || {
        let img = renderer.render(&scene.list, &scene.images, &options).unwrap();
        renderer.recycle(img);
    });
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
}

#[test]
fn deeply_nested_isolated_groups_do_not_blow_up_quadratically() {
    // 40 nested colour-transformed groups around 200 shapes: bounds are recomputed per level, so this is
    // O(depth * nodes) and must stay fast.
    let leaves: Vec<Node> = (0..200).map(|i| solid_circle(f64::from(i % 20) * 30.0 + 15.0, f64::from(i / 20) * 30.0 + 15.0, 12.0, RED)).collect();
    let mut node = Node::Group(Group { children: leaves, ..Group::default() });
    for _ in 0..40 {
        node = Node::Group(Group { color: ColorTransform::alpha(0.98), children: vec![node], ..Group::default() });
    }
    let mut scene = Scene::new(640, 360);
    scene.add(node);
    let elapsed = timed("40 nested isolated groups x 200 shapes", || {
        let _ = scene.render();
    });
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
}

#[test]
fn zoomed_out_view_of_a_large_stage_is_quick() {
    let mut scene = Scene::new(16_000, 16_000);
    scene.add(solid_rect(0.0, 0.0, 16_000.0, 16_000.0, RED));
    scene.add(Node::Group(Group {
        filters: vec![Filter::Blur { x: 200.0, y: 200.0, quality: 2 }],
        children: vec![solid_circle(8000.0, 8000.0, 3000.0, BLUE)],
        ..Group::default()
    }));
    let options = RenderOptions::fit_stage(scene.list.size, 800, 800);
    let elapsed = timed("16000x16000 stage at 800x800", || {
        let _ = scene.render_with(&options);
    });
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    let _ = Affine::IDENTITY;
}
