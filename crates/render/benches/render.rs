//! Rasteriser benchmarks on synthetic stress scenes (the hardest verification fixtures, scaled up).
//!
//! With `BENCH_CHECKSUM=1` each scene prints a checksum of its output before timing, so that an optimisation can be
//! checked to leave the pixels unchanged: compare the `checksum` lines of two runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops,
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    reason = "benchmark code over synthetic scenes"
)]

use std::hint::black_box;

use anim_core::kurbo::{Circle, Shape};
use anim_core::{Affine, BezPath, BlendMode, ColorTransform, Point, Rgba8, Size};
use anim_display::{
    Bitmap, DisplayList, Fill, FillRule, Filter, Gradient, GradientStop, Group, Image, ImageStore, Interpolation, Mesh, Node, Paint, Spread, Stroke,
    StrokeStyle,
};
use anim_render::{RenderOptions, Renderer};
use criterion::{Criterion, criterion_group, criterion_main};

const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
const BLUE: Rgba8 = Rgba8::opaque(0, 0, 255);
const GREEN: Rgba8 = Rgba8::opaque(0, 160, 0);

struct Scene {
    list: DisplayList,
    images: ImageStore,
    options: RenderOptions,
}

impl Scene {
    fn new(width: u32, height: u32) -> Self {
        Self {
            list: DisplayList::new(Size::new(f64::from(width), f64::from(height)), Some(Rgba8::WHITE)),
            images: ImageStore::new(),
            options: RenderOptions::new(width, height),
        }
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((x0, y0));
    path.line_to((x1, y0));
    path.line_to((x1, y1));
    path.line_to((x0, y1));
    path.close_path();
    path
}

fn circle(cx: f64, cy: f64, radius: f64) -> BezPath {
    Circle::new((cx, cy), radius).to_path(0.05)
}

fn fill(path: BezPath, paint: Paint, rule: FillRule) -> Node {
    Node::Fill(Fill { transform: Affine::IDENTITY, path, rule, paint })
}

fn stroke(path: BezPath, width: f64, paint: Paint) -> Node {
    Node::Stroke(Stroke { transform: Affine::IDENTITY, path, style: StrokeStyle { width, ..StrokeStyle::default() }, paint })
}

fn gradient(spread: Spread, focal: f64, stops: &[(f32, Rgba8)]) -> Gradient {
    Gradient {
        transform: Affine::translate((960.0, 540.0)) * Affine::scale(0.9),
        stops: stops.iter().map(|(offset, color)| GradientStop { offset: *offset, color: *color }).collect(),
        spread,
        interpolation: Interpolation::Rgb,
        focal,
    }
}

fn group(children: Vec<Node>) -> Group {
    Group { children, ..Group::default() }
}

/// Deterministic noise in 0..1.
fn noise(i: u32) -> f64 {
    let x = i.wrapping_mul(2_654_435_761).rotate_left(13).wrapping_mul(40_503);
    f64::from(x >> 8) / f64::from(1_u32 << 24)
}

/// 400 circles and 400 stroked lines on a Full HD frame.
fn many_shapes() -> Scene {
    let mut scene = Scene::new(1920, 1080);
    for i in 0..400_u32 {
        let x = f64::from(i % 40) * 48.0;
        let y = f64::from(i / 40) * 100.0;
        scene.list.nodes.push(fill(circle(x + 24.0, y + 24.0, 22.0), Paint::Solid(if i % 2 == 0 { RED } else { BLUE }), FillRule::NonZero));
        let mut line = BezPath::new();
        line.move_to((x, y));
        line.line_to((x + 40.0, y + 60.0));
        scene.list.nodes.push(stroke(line, 3.0, Paint::Solid(Rgba8::BLACK)));
    }
    scene
}

/// A spirograph star with thousands of self-intersections, filled even-odd and non-zero, plus a 200 px stroke of it.
fn self_intersecting() -> Scene {
    let mut scene = Scene::new(1920, 1080);
    let mut path = BezPath::new();
    let points = 3001_u32;
    for k in 0..points {
        let angle = f64::from(k) * 2.0 * std::f64::consts::PI * 1000.0 / f64::from(points);
        let p = (960.0 + 500.0 * angle.cos(), 540.0 + 500.0 * angle.sin());
        if k == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.close_path();
    scene.list.nodes.push(fill(path.clone(), Paint::Solid(RED), FillRule::EvenOdd));
    scene.list.nodes.push(stroke(path, 1.5, Paint::Solid(Rgba8::BLACK)));
    scene
}

/// A long round-joined polyline with a 200 px wide stroke, and a hairline version of it.
fn wide_strokes() -> Scene {
    let mut scene = Scene::new(1920, 1080);
    let mut path = BezPath::new();
    path.move_to((0.0, 540.0));
    for k in 1..2000_u32 {
        let t = f64::from(k);
        path.quad_to((t * 0.96 + 10.0, 540.0 + 400.0 * (t / 17.0).sin()), (t * 0.96 + 20.0, 540.0 + 300.0 * (t / 31.0).cos()));
    }
    scene.list.nodes.push(stroke(path.clone(), 200.0, Paint::Solid(Rgba8::new(0, 0, 255, 90))));
    scene.list.nodes.push(stroke(path, 0.0, Paint::Solid(Rgba8::BLACK)));
    scene
}

/// 60 full-frame gradient fills: linear, radial and focal, every spread, out of order stops.
fn gradients() -> Scene {
    let mut scene = Scene::new(1920, 1080);
    let stops = [(0.0, RED), (0.2, GREEN), (0.55, BLUE), (0.7, Rgba8::new(255, 255, 0, 200)), (1.0, Rgba8::BLACK)];
    for i in 0..60_u32 {
        let spread = [Spread::Pad, Spread::Reflect, Spread::Repeat][usize::try_from(i % 3).unwrap()];
        let g = gradient(spread, if i % 4 == 3 { 0.6 } else { 0.0 }, &stops);
        let paint = if i % 2 == 0 { Paint::Linear(g) } else { Paint::Radial(g) };
        let y = f64::from(i % 6) * 180.0;
        scene.list.nodes.push(fill(rect(f64::from(i) * 8.0, y, f64::from(i) * 8.0 + 900.0, y + 220.0), paint, FillRule::NonZero));
    }
    scene
}

/// A 256 px noise bitmap, tiled and rotated, smoothing on and off, drawn small (mip path) and large.
fn bitmaps() -> Scene {
    let mut scene = Scene::new(1920, 1080);
    let size = 256_u32;
    let bytes: Vec<u8> = (0..size * size).flat_map(|i| [(noise(i) * 255.0) as u8, (noise(i + 7) * 255.0) as u8, (noise(i + 99) * 255.0) as u8, 255]).collect();
    let id = scene.images.insert(Image::new(size, size, bytes).unwrap());
    for (i, smoothing) in [true, false, true, false].into_iter().enumerate() {
        let transform = Affine::rotate(0.3 + i as f64) * Affine::scale(if i < 2 { 1.7 } else { 0.31 });
        scene.list.nodes.push(fill(
            rect(0.0, 270.0 * i as f64, 1920.0, 270.0 * (i + 1) as f64),
            Paint::Bitmap { image: id, transform, repeat: true, smoothing },
            FillRule::NonZero,
        ));
    }
    for k in 0..40_u32 {
        let transform = Affine::translate((f64::from(k % 8) * 230.0, f64::from(k / 8) * 210.0)) * Affine::rotate(f64::from(k) * 0.1) * Affine::scale(0.5);
        scene.list.nodes.push(Node::Bitmap(Bitmap { image: id, transform, smoothing: true }));
    }
    scene
}

/// Nested groups with masks, colour transforms and every blend mode, a few levels deep.
fn layers() -> Scene {
    let mut scene = Scene::new(1280, 720);
    let modes = [
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::Invert,
        BlendMode::Alpha,
        BlendMode::Erase,
    ];
    for (i, mode) in modes.into_iter().enumerate() {
        let x = f64::from(u32::try_from(i % 5).unwrap()) * 250.0;
        let y = f64::from(u32::try_from(i / 5).unwrap()) * 170.0;
        let inner = Group {
            children: vec![
                fill(rect(x, y, x + 300.0, y + 200.0), Paint::Solid(RED), FillRule::NonZero),
                fill(circle(x + 150.0, y + 100.0, 90.0), Paint::Solid(BLUE), FillRule::NonZero),
            ],
            color: ColorTransform { mul: [1.0, 0.9, 0.8, 0.8], add: [0.0, 0.0, 0.1, 0.0] },
            mask: Some(vec![fill(circle(x + 140.0, y + 100.0, 120.0), Paint::Solid(Rgba8::BLACK), FillRule::NonZero)]),
            ..Group::default()
        };
        let outer = Group { blend: mode, children: vec![Node::Group(inner)], ..Group::default() };
        scene.list.nodes.push(Node::Group(outer));
    }
    scene
}

/// A blurred, shadowed full-frame group.
fn filters() -> Scene {
    let mut scene = Scene::new(1280, 720);
    let mut g = group(
        (0..30_u32)
            .map(|i| fill(circle(noise(i) * 1280.0, noise(i + 50) * 720.0, 40.0 + noise(i + 90) * 60.0), Paint::Solid(RED), FillRule::NonZero))
            .collect(),
    );
    g.filters = vec![
        Filter::Blur { x: 24.0, y: 24.0, quality: 2 },
        Filter::DropShadow {
            color: Rgba8::BLACK,
            angle_deg: 45.0,
            distance: 12.0,
            blur_x: 10.0,
            blur_y: 10.0,
            strength: 1.0,
            inner: false,
            knockout: false,
            hide_object: false,
        },
    ];
    scene.list.nodes.push(Node::Group(g));
    scene
}

/// A 50 x 50 cell warped mesh (5000 triangles) over a textured square.
fn mesh() -> Scene {
    let mut scene = Scene::new(1100, 1100);
    let cells = 50_u32;
    let (mut source, mut target, mut triangles) = (Vec::new(), Vec::new(), Vec::new());
    for row in 0..=cells {
        for col in 0..=cells {
            let p = Point::new(f64::from(col) * 16.0, f64::from(row) * 16.0);
            source.push(p);
            target.push(Point::new(p.x + 12.0 * (p.y / 90.0).sin() + 100.0, p.y + 12.0 * (p.x / 70.0).cos() + 100.0));
        }
    }
    for row in 0..cells {
        for col in 0..cells {
            let a = row * (cells + 1) + col;
            triangles.push([a, a + 1, a + cells + 1]);
            triangles.push([a + 1, a + cells + 2, a + cells + 1]);
        }
    }
    let content = vec![
        fill(rect(0.0, 0.0, 800.0, 800.0), Paint::Solid(RED), FillRule::NonZero),
        fill(circle(400.0, 400.0, 300.0), Paint::Solid(BLUE), FillRule::NonZero),
    ];
    scene.list.nodes.push(Node::Mesh(Mesh { transform: Affine::IDENTITY, content, source, target, triangles }));
    scene
}

fn checksum(image: &Image) -> u64 {
    image.premultiplied_rgba.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3))
}

fn render_benches(c: &mut Criterion) {
    let scenes: [(&str, Scene); 8] = [
        ("many_shapes_1080p", many_shapes()),
        ("self_intersecting_even_odd", self_intersecting()),
        ("wide_strokes_200px", wide_strokes()),
        ("gradients_60_fullframe", gradients()),
        ("bitmaps_tiled_rotated", bitmaps()),
        ("layers_masks_blends", layers()),
        ("filters_blur_shadow", filters()),
        ("mesh_5000_triangles", mesh()),
    ];
    let mut group = c.benchmark_group("render");
    group.sample_size(20);
    for (name, scene) in &scenes {
        let mut renderer = Renderer::new();
        if std::env::var_os("BENCH_CHECKSUM").is_some() {
            let first = renderer.render(&scene.list, &scene.images, &scene.options).unwrap();
            eprintln!("checksum {name} {:016x}", checksum(&first));
            renderer.recycle(first);
        }
        // BENCH_FIXED=<renders> BENCH_ONLY=<scene>: a fixed amount of work for `perf stat`, which counts cycles and
        // instructions and so is steadier than wall time on a busy machine.
        if let Some(count) = std::env::var("BENCH_FIXED").ok().and_then(|v| v.parse::<u32>().ok()) {
            if std::env::var("BENCH_ONLY").is_ok_and(|only| name.contains(only.as_str())) {
                for _ in 0..count {
                    let image = renderer.render(black_box(&scene.list), &scene.images, &scene.options).unwrap();
                    renderer.recycle(black_box(image));
                }
            }
            continue;
        }
        group.bench_function(*name, |b| {
            b.iter(|| {
                let image = renderer.render(black_box(&scene.list), &scene.images, &scene.options).unwrap();
                renderer.recycle(black_box(image));
            });
        });
    }
    group.finish();
}

criterion_group!(benches, render_benches);
criterion_main!(benches);
