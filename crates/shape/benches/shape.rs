//! Edge-shape benchmarks: a synthetic stress shape (a 100 x 100 grid of cells with shared, curved boundaries and
//! four fills) and, when `ANICRAFT_SAMPLES` points at the sample folder, every shape of the Adobe sample FLAs.
//!
//! With `BENCH_CHECKSUM=1` the benches print a checksum of what they produce, to compare two runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::panic,
    reason = "benchmark code over trusted local samples"
)]

use std::hint::black_box;
use std::io::Read;
use std::path::Path;

use anim_core::Point;
use anim_core::kurbo::{PathEl, Shape};
use anim_shape::{Edge, EdgeShape, FillStyle, NoImages, Segment, StrokeStyle, XflEdge, encode_edges, parse_edges};
use criterion::{Criterion, criterion_group, criterion_main};

const CELLS: u32 = 100;
/// Cell size in twips.
const CELL: f64 = 200.0;

fn fill_of(i: u32, j: u32) -> u32 {
    1 + (i * 7 + j * 3) % 4
}

/// A grid of cells; every boundary is one curved edge listed once, with the cell on each side as its fills.
fn grid_shape() -> EdgeShape {
    let styles = vec![FillStyle::default(); 4];
    let mut edges = Vec::new();
    let pt = |i: u32, j: u32| Point::new(f64::from(i) * CELL, f64::from(j) * CELL);
    let side = |i: u32, j: u32| if i < CELLS && j < CELLS { fill_of(i, j) } else { 0 };
    for j in 0..=CELLS {
        for i in 0..CELLS {
            let (from, to) = (pt(i, j), pt(i + 1, j));
            let ctrl = Point::new(from.x + CELL / 2.0, from.y + if (i + j) % 2 == 0 { 40.0 } else { -40.0 });
            let above = if j > 0 { side(i, j - 1) } else { 0 };
            edges.push(Edge::new(above, side(i, j), 0, vec![Segment::Quad { from, ctrl, to }]));
        }
    }
    for i in 0..=CELLS {
        for j in 0..CELLS {
            let (from, to) = (pt(i, j), pt(i, j + 1));
            let ctrl = Point::new(from.x + if (i + j) % 2 == 0 { 40.0 } else { -40.0 }, from.y + CELL / 2.0);
            let west = if i > 0 { side(i - 1, j) } else { 0 };
            edges.push(Edge::new(side(i, j), west, 0, vec![Segment::Line { from, to: ctrl }, Segment::Line { from: ctrl, to }]));
        }
    }
    EdgeShape { fills: styles, strokes: Vec::new(), edges }
}

fn hash(h: u64, value: f64) -> u64 {
    (h ^ value.to_bits()).wrapping_mul(0x100_0000_01b3)
}

fn nodes_checksum(shape: &EdgeShape) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for fill in shape.assemble().fills {
        for el in fill.path.elements() {
            let points: &[Point] = match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => &[*p],
                PathEl::QuadTo(a, b) => &[*a, *b],
                PathEl::CurveTo(a, b, c) => &[*a, *b, *c],
                PathEl::ClosePath => &[],
            };
            for p in points {
                h = hash(hash(h, p.x), p.y);
            }
        }
        h = hash(h, fill.path.bounding_box().area());
    }
    h
}

fn le(bytes: &[u8], at: usize, len: usize) -> Option<usize> {
    let b = bytes.get(at..at + len)?;
    Some(b.iter().rev().fold(0_usize, |acc, x| (acc << 8) | usize::from(*x)))
}

/// The XML entries of a FLA, read by walking local file headers (the central directory of these samples is damaged).
fn xml_entries(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).expect("sample readable");
    let mut out = Vec::new();
    let mut pos = 0;
    while bytes.get(pos..pos + 4) == Some(&[0x50, 0x4B, 0x03, 0x04]) {
        let method = le(&bytes, pos + 8, 2).unwrap();
        let compressed = le(&bytes, pos + 18, 4).unwrap();
        let name_len = le(&bytes, pos + 26, 2).unwrap();
        let extra_len = le(&bytes, pos + 28, 2).unwrap();
        let name = String::from_utf8_lossy(bytes.get(pos + 30..pos + 30 + name_len).unwrap()).into_owned();
        let start = pos + 30 + name_len + extra_len;
        let data = bytes.get(start..start + compressed).unwrap();
        pos = start + compressed;
        if !name.to_ascii_lowercase().ends_with(".xml") || name.starts_with("META-INF") || name.starts_with("Publish") {
            continue;
        }
        let text = if method == 8 {
            let mut inflated = Vec::new();
            flate2::read::DeflateDecoder::new(data).read_to_end(&mut inflated).expect("inflate");
            inflated
        } else {
            data.to_vec()
        };
        out.push(String::from_utf8_lossy(&text).into_owned());
    }
    out
}

/// Every `<DOMShape>` of the sample FLAs as its `<Edge>` attributes.
fn sample_shapes() -> Vec<Vec<XflEdge>> {
    let Some(dir) = std::env::var_os("ANICRAFT_SAMPLES") else { return Vec::new() };
    let mut shapes = Vec::new();
    for name in ["unicorn_4AE.fla", "derwood_green_choreo_short.fla"] {
        let path = Path::new(&dir).join(name);
        if !path.exists() {
            continue;
        }
        for xml in xml_entries(&path) {
            let Ok(doc) = roxmltree::Document::parse_with_options(&xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }) else { continue };
            for shape in doc.descendants().filter(|n| n.has_tag_name("DOMShape")) {
                let index = |n: roxmltree::Node, a: &str| n.attribute(a).map_or(0, |v| v.parse::<u32>().unwrap_or(0));
                let edges: Vec<XflEdge> = shape
                    .descendants()
                    .filter(|n| n.has_tag_name("Edge"))
                    .map(|e| XflEdge {
                        fill_style0: index(e, "fillStyle0"),
                        fill_style1: index(e, "fillStyle1"),
                        stroke_style: index(e, "strokeStyle"),
                        edges: e.attribute("edges").map(str::to_owned),
                        cubics: e.attribute("cubics").map(str::to_owned),
                    })
                    .collect();
                shapes.push(edges);
            }
        }
    }
    shapes
}

fn dummy_styles(edges: &[XflEdge]) -> (Vec<FillStyle>, Vec<StrokeStyle>) {
    let fills = edges.iter().map(|e| e.fill_style0.max(e.fill_style1)).max().unwrap_or(0);
    let strokes = edges.iter().map(|e| e.stroke_style).max().unwrap_or(0);
    (vec![FillStyle::default(); fills as usize], vec![StrokeStyle::default(); strokes as usize])
}

fn shape_benches(c: &mut Criterion) {
    let grid = grid_shape();
    let segments: Vec<Segment> = grid.edges.iter().flat_map(|e| e.segments.iter().copied()).collect();
    let text = encode_edges(&segments);
    if std::env::var_os("BENCH_CHECKSUM").is_some() {
        eprintln!("checksum grid {:016x}", nodes_checksum(&grid));
    }
    let mut group = c.benchmark_group("shape");
    group.sample_size(20);
    group.bench_function("grid_encode", |b| b.iter(|| encode_edges(black_box(&segments))));
    group.bench_function("grid_parse", |b| b.iter(|| parse_edges(black_box(&text)).unwrap()));
    group.bench_function("grid_assemble", |b| b.iter(|| black_box(&grid).assemble()));
    group.bench_function("grid_to_display", |b| b.iter(|| black_box(&grid).to_display(&NoImages)));
    group.bench_function("grid_bounds_hit", |b| b.iter(|| (black_box(&grid).bounds(), grid.hit_test(Point::new(10_050.0 / 20.0, 9_950.0 / 20.0), 3.0))));

    let samples = sample_shapes();
    if !samples.is_empty() {
        let built: Vec<EdgeShape> = samples
            .iter()
            .map(|edges| {
                let (fills, strokes) = dummy_styles(edges);
                EdgeShape::from_xfl_lenient(fills, strokes, edges).0
            })
            .collect();
        if std::env::var_os("BENCH_CHECKSUM").is_some() {
            eprintln!("checksum samples {:016x}", built.iter().fold(0_u64, |h, s| h ^ nodes_checksum(s)));
        }
        group.bench_function("samples_from_xfl", |b| {
            b.iter(|| {
                for edges in &samples {
                    let (fills, strokes) = dummy_styles(edges);
                    black_box(EdgeShape::from_xfl_lenient(fills, strokes, edges));
                }
            });
        });
        group.bench_function("samples_to_display", |b| {
            b.iter(|| {
                for shape in &built {
                    black_box(shape.to_display(&NoImages));
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, shape_benches);
criterion_main!(benches);
