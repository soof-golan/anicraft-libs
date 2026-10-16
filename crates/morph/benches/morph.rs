//! Shape-tween benchmarks on synthetic stress shapes (the verification fixtures, scaled up): a many-vertex circle
//! to star, a square to a ring with a growing hole, a grid of hundreds of contours, and hinted tweens.
//!
//! The XFL samples cannot be read from this crate (the importer lives in the app repo), so there is no sample-driven
//! bench here; `anim-timeline`'s bench covers the same code through whole documents.
//!
//! With `BENCH_CHECKSUM=1` each case prints a checksum of its in-betweens before timing, so that an optimisation can
//! be checked to leave the output unchanged: compare the `checksum` lines of two runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "benchmark code over synthetic shapes"
)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::hint::black_box;

use anim_core::kurbo::{Circle, Shape};
use anim_core::{BezPath, Point, Rgba8};
use anim_morph::{MorphMode, MorphOptions, MorphPlan, ShapeHint};
use anim_shape::{EdgeShape, FillStyle, NoImages, PathInput};
use criterion::{Criterion, criterion_group, criterion_main};

fn shape_of(paths: Vec<(BezPath, Rgba8)>) -> EdgeShape {
    let inputs: Vec<PathInput> = paths.into_iter().map(|(p, c)| PathInput::new(p, Some(FillStyle::Solid(c)), None)).collect();
    EdgeShape::from_paths(&inputs)
}

fn polygon(points: impl Iterator<Item = Point>) -> BezPath {
    let mut path = BezPath::new();
    for (i, p) in points.enumerate() {
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.close_path();
    path
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    polygon([Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)].into_iter())
}

/// A star with `n` spikes around `(cx, cy)`.
fn star(cx: f64, cy: f64, outer: f64, inner: f64, n: usize) -> BezPath {
    polygon((0..n * 2).map(|i| {
        let r = if i % 2 == 0 { outer } else { inner };
        let a = i as f64 * std::f64::consts::PI / n as f64;
        Point::new(cx + r * a.cos(), cy + r * a.sin())
    }))
}

fn circle(cx: f64, cy: f64, r: f64) -> BezPath {
    Circle::new(Point::new(cx, cy), r).to_path(0.01)
}

fn checksum(shape: &EdgeShape) -> u64 {
    let mut h = DefaultHasher::new();
    format!("{:?}", shape.to_display(&NoImages)).hash(&mut h);
    h.finish()
}

struct Case {
    name: &'static str,
    start: EdgeShape,
    end: EdgeShape,
    options: MorphOptions,
}

fn cases() -> Vec<Case> {
    let red = Rgba8::opaque(220, 40, 40);
    let blue = Rgba8::opaque(40, 40, 220);
    let mut out = Vec::new();

    // One contour with hundreds of vertices at each end.
    out.push(Case {
        name: "circle_to_star_200",
        start: shape_of(vec![(circle(100.0, 100.0, 80.0), red)]),
        end: shape_of(vec![(star(100.0, 100.0, 90.0, 40.0, 200), blue)]),
        options: MorphOptions::default(),
    });
    out.push(Case {
        name: "circle_to_star_200_angular",
        start: shape_of(vec![(circle(100.0, 100.0, 80.0), red)]),
        end: shape_of(vec![(star(100.0, 100.0, 90.0, 40.0, 200), blue)]),
        options: MorphOptions { mode: MorphMode::Angular, hints: Vec::new() },
    });

    // A square becomes a ring: the hole appears.
    let mut ring = star(100.0, 100.0, 90.0, 85.0, 120);
    ring.extend(star(100.0, 100.0, 40.0, 38.0, 120).elements().iter().copied());
    out.push(Case {
        name: "square_to_ring",
        start: shape_of(vec![(rect(10.0, 10.0, 180.0, 180.0), red)]),
        end: shape_of(vec![(ring, blue)]),
        options: MorphOptions::default(),
    });

    // Hundreds of separate contours: the pairing step dominates.
    let grid = |f: &dyn Fn(f64, f64) -> BezPath, color: Rgba8| {
        let mut paths = Vec::new();
        for i in 0..20 {
            for j in 0..20 {
                paths.push((f(f64::from(i) * 20.0 + 10.0, f64::from(j) * 20.0 + 10.0), color));
            }
        }
        shape_of(paths)
    };
    out.push(Case {
        name: "grid_400_squares_to_circles",
        start: grid(&|x, y| rect(x - 6.0, y - 6.0, 12.0, 12.0), red),
        end: grid(&|x, y| circle(x, y, 7.0), blue),
        options: MorphOptions::default(),
    });

    // Hinted: four pinned points on a square becoming a many-sided star.
    out.push(Case {
        name: "square_to_star_4_hints",
        start: shape_of(vec![(rect(10.0, 10.0, 180.0, 180.0), red)]),
        end: shape_of(vec![(star(100.0, 100.0, 95.0, 50.0, 60), blue)]),
        options: MorphOptions {
            mode: MorphMode::Distributive,
            hints: vec![
                ShapeHint::new(Point::new(10.0, 10.0), Point::new(195.0, 100.0)),
                ShapeHint::new(Point::new(190.0, 10.0), Point::new(100.0, 195.0)),
                ShapeHint::new(Point::new(190.0, 190.0), Point::new(5.0, 100.0)),
                ShapeHint::new(Point::new(10.0, 190.0), Point::new(100.0, 5.0)),
            ],
        },
    });
    out
}

fn bench(c: &mut Criterion) {
    let cases = cases();
    let check = std::env::var_os("BENCH_CHECKSUM").is_some();
    for case in &cases {
        let plan = MorphPlan::new(&case.start, &case.end, &case.options);
        if check {
            let h = [0.1, 0.5, 0.9].iter().fold(0_u64, |h, t| h ^ checksum(&plan.at(*t)));
            eprintln!("checksum {} {h:016x} (fill contours {})", case.name, plan.fill_contours());
        }
        c.bench_function(&format!("plan/{}", case.name), |b| b.iter(|| MorphPlan::new(black_box(&case.start), black_box(&case.end), black_box(&case.options))));
        c.bench_function(&format!("at/{}", case.name), |b| b.iter(|| plan.at(black_box(0.5))));
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
