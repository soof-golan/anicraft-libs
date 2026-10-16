//! ARAP benchmarks on the meshes puppets use: a finely meshed body with a block of pins moved rigidly (a welded
//! head) and a coarse mesh pulled by a few far handles.
//!
//! With `BENCH_CHECKSUM=1` each scene prints a checksum of the exact bits of its solutions before timing, so that an
//! optimisation can be checked to leave the output unchanged: compare the `checksum` lines of two runs.
#![allow(
    clippy::unwrap_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::print_stderr,
    clippy::suboptimal_flops,
    reason = "benchmark code over synthetic scenes"
)]

use std::hash::{DefaultHasher, Hash, Hasher};
use std::hint::black_box;

use anim_core::{Point, Vec2};
use anim_rig::{ArapSolver, Mesh, MeshOptions, Outline};
use criterion::{Criterion, criterion_group, criterion_main};

fn rect_mesh(width: f64, height: f64, edge: f64) -> Mesh {
    let outline = Outline { outer: vec![Point::new(0.0, 0.0), Point::new(width, 0.0), Point::new(width, height), Point::new(0.0, height)], holes: vec![] };
    Mesh::from_outline(&outline, &MeshOptions { max_edge: edge }).unwrap()
}

fn ids_where(mesh: &Mesh, keep: impl Fn(Point) -> bool) -> Vec<u32> {
    mesh.vertices.iter().enumerate().filter(|(_, p)| keep(**p)).map(|(i, _)| u32::try_from(i).unwrap()).collect()
}

fn checksum(solutions: &[Vec<Point>]) -> u64 {
    let mut h = DefaultHasher::new();
    for s in solutions {
        for p in s {
            p.x.to_bits().hash(&mut h);
            p.y.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

/// A body mesh: the top pinned block rotated and shifted like a welded head, the bottom row held.
fn body_scene(edge: f64) -> (ArapSolver, Vec<Vec<Point>>) {
    let mesh = rect_mesh(400.0, 600.0, edge);
    let top = ids_where(&mesh, |p| p.y < 40.0);
    let bottom = ids_where(&mesh, |p| p.y > 590.0);
    let pins: Vec<u32> = top.iter().chain(&bottom).copied().collect();
    let solver = ArapSolver::new(&mesh, &pins).unwrap();
    let targets = [0.0_f64, 0.3, -0.5, 0.8]
        .iter()
        .map(|angle| {
            let (sn, cs) = angle.sin_cos();
            let c = Point::new(200.0, 40.0);
            solver
                .pinned()
                .iter()
                .map(|&i| {
                    let p = mesh.vertices[i as usize];
                    if p.y < 40.0 {
                        let v = p - c;
                        c + Vec2::new(cs * v.x - sn * v.y, sn * v.x + cs * v.y) + Vec2::new(30.0 * angle, -40.0)
                    } else {
                        p
                    }
                })
                .collect()
        })
        .collect();
    (solver, targets)
}

/// A coarse mesh with four pins pulled far away, past anything the mesh can follow rigidly.
fn pulled_scene() -> (ArapSolver, Vec<Vec<Point>>) {
    let mesh = rect_mesh(300.0, 300.0, 12.0);
    let corners = [Point::new(0.0, 0.0), Point::new(300.0, 0.0), Point::new(0.0, 300.0), Point::new(300.0, 300.0)];
    let pins: Vec<u32> = corners
        .iter()
        .map(|c| ids_where(&mesh, |p| p.distance(*c) < 1.0).first().copied().unwrap())
        .collect();
    let solver = ArapSolver::new(&mesh, &pins).unwrap();
    let targets = [(300.0, -250.0), (-900.0, 700.0), (50.0, 2000.0)]
        .iter()
        .map(|&(dx, dy)| {
            solver
                .pinned()
                .iter()
                .enumerate()
                .map(|(k, &i)| {
                    let p = mesh.vertices[i as usize];
                    if k == 3 { p + Vec2::new(dx, dy) } else { p }
                })
                .collect()
        })
        .collect();
    (solver, targets)
}

fn bench_arap(c: &mut Criterion) {
    let checks = std::env::var_os("BENCH_CHECKSUM").is_some();
    let mut g = c.benchmark_group("arap");
    g.sample_size(20);
    for (name, edge) in [("body_edge_15", 15.0), ("body_edge_10", 10.0)] {
        g.bench_function(format!("solve_{name}"), |b| {
            let (solver, targets) = body_scene(edge);
            if checks {
                let all: Vec<_> = targets.iter().map(|t| solver.solve(t)).collect();
                eprintln!("checksum {name} {:016x}", checksum(&all));
            }
            b.iter(|| {
                for t in &targets {
                    black_box(solver.solve(black_box(t)));
                }
            });
        });
        g.bench_function(format!("build_{name}"), |b| {
            let mesh = rect_mesh(400.0, 600.0, edge);
            let pins = ids_where(&mesh, |p| p.y < 40.0 || p.y > 590.0);
            b.iter(|| black_box(ArapSolver::new(black_box(&mesh), &pins).unwrap()));
        });
    }
    g.bench_function("solve_pulled_far", |b| {
        let (solver, targets) = pulled_scene();
        if checks {
            let all: Vec<_> = targets.iter().map(|t| solver.solve(t)).collect();
            eprintln!("checksum pulled {:016x}", checksum(&all));
        }
        b.iter(|| {
            for t in &targets {
                black_box(solver.solve(black_box(t)));
            }
        });
    });
    g.finish();
}

criterion_group!(benches, bench_arap);
criterion_main!(benches);
