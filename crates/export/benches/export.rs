//! Export benchmarks on synthetic stress inputs (the hardest verification fixtures, scaled up).
//!
//! The crate has no document parsers, so there are no sample-file benchmarks here; the sample-driven export paths are
//! measured in the app repositories. With `BENCH_CHECKSUM=1` each case prints a checksum of its output before timing,
//! so that an optimisation can be checked to leave the bytes unchanged: compare the `checksum` lines of two runs.
//! With `BENCH_FIXED=<runs> BENCH_ONLY=<case>` only that case runs, a fixed number of times (for `perf record`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::suboptimal_flops,
    clippy::missing_const_for_fn,
    reason = "benchmark code over synthetic scenes"
)]

use std::hint::black_box;

use anim_core::kurbo::{Circle, Shape};
use anim_core::{Affine, BezPath, Point, Rgba8, Size};
use anim_display::{
    DisplayList, Fill, FillRule, Gradient, GradientStop, Group, Image, ImageStore, Interpolation, Mesh, Node, Paint, Spread, Stroke, StrokeStyle,
};
use anim_export::{
    AnimatedSvgOptions, GifOptions, RasterOptions, SheetLayout, SheetOptions, SvgOptions, VecFrameSource, build_sprite_sheet, display_list_to_svg, encode_png,
    export_animated_svg, export_gif, render_frame,
};
use criterion::{Criterion, criterion_group, criterion_main};

const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
const BLUE: Rgba8 = Rgba8::opaque(0, 0, 255);
const GREEN: Rgba8 = Rgba8::opaque(0, 160, 0);

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

fn gradient(stops: &[(f32, Rgba8)]) -> Gradient {
    Gradient {
        transform: Affine::translate((320.0, 180.0)) * Affine::scale(0.4),
        stops: stops.iter().map(|(offset, color)| GradientStop { offset: *offset, color: *color }).collect(),
        spread: Spread::Reflect,
        interpolation: Interpolation::Rgb,
        focal: 0.0,
    }
}

/// Deterministic noise in 0..1.
fn noise(i: u32) -> f64 {
    let x = i.wrapping_mul(2_654_435_761).rotate_left(13).wrapping_mul(40_503);
    f64::from(x >> 8) / f64::from(1_u32 << 24)
}

/// Many circles, strokes and gradient fills in nested groups: the SVG emitter's stress case.
fn busy_list(circles: u32) -> DisplayList {
    let mut list = DisplayList::new(Size::new(1920.0, 1080.0), Some(Rgba8::WHITE));
    let stops = [(0.0, RED), (0.4, GREEN), (1.0, Rgba8::new(0, 0, 255, 128))];
    let mut children = Vec::new();
    for i in 0..circles {
        let x = f64::from(i % 60) * 32.0;
        let y = f64::from(i / 60 % 36) * 30.0;
        let paint = match i % 3 {
            0 => Paint::Solid(if i % 2 == 0 { RED } else { BLUE }),
            1 => Paint::Linear(gradient(&stops)),
            _ => Paint::Radial(gradient(&stops)),
        };
        children.push(fill(circle(x + 16.0, y + 16.0, 14.0), paint, if i % 5 == 0 { FillRule::EvenOdd } else { FillRule::NonZero }));
        let mut line = BezPath::new();
        line.move_to((x, y));
        line.curve_to((x + 10.0, y + 30.0), (x + 20.0, y - 10.0), (x + 30.0, y + 20.0));
        children.push(stroke(line, 2.5, Paint::Solid(Rgba8::BLACK)));
        if children.len() >= 40 {
            list.nodes.push(Node::Group(Group { children: std::mem::take(&mut children), ..Group::default() }));
        }
    }
    list.nodes.push(Node::Group(Group { children, ..Group::default() }));
    list
}

fn mesh_list() -> DisplayList {
    let mut list = DisplayList::new(Size::new(1100.0, 1100.0), Some(Rgba8::WHITE));
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
    list.nodes.push(Node::Mesh(Mesh { transform: Affine::IDENTITY, content, source, target, triangles }));
    list
}

fn bitmap_list(images: &mut ImageStore) -> DisplayList {
    let mut list = DisplayList::new(Size::new(640.0, 360.0), None);
    let bytes: Vec<u8> =
        (0..128_u32 * 128).flat_map(|i| [(noise(i) * 255.0) as u8, (noise(i + 7) * 255.0) as u8, (noise(i + 99) * 255.0) as u8, 255]).collect();
    let id = images.insert(Image::new(128, 128, bytes).unwrap());
    for k in 0..24_u32 {
        let transform = Affine::translate((f64::from(k % 6) * 100.0, f64::from(k / 6) * 90.0)) * Affine::rotate(f64::from(k) * 0.1);
        list.nodes.push(Node::Bitmap(anim_display::Bitmap { image: id, transform, smoothing: true }));
    }
    list
}

/// A frame of a moving, semi-transparent scene on a transparent stage.
fn moving_frame(t: u32, width: f64, height: f64) -> DisplayList {
    let mut list = DisplayList::new(Size::new(width, height), None);
    let stops = [(0.0, RED), (0.5, GREEN), (1.0, Rgba8::new(0, 0, 255, 100))];
    list.nodes.push(fill(rect(0.0, 0.0, width, height), Paint::Linear(gradient(&stops)), FillRule::NonZero));
    for i in 0..12_u32 {
        let phase = f64::from(t) * 0.2 + f64::from(i);
        let (x, y) = (width / 2.0 + phase.cos() * width * 0.35, height / 2.0 + (phase * 1.3).sin() * height * 0.35);
        list.nodes.push(fill(circle(x, y, 18.0 + f64::from(i)), Paint::Solid(Rgba8::new(255, (i * 20) as u8, 0, 180)), FillRule::NonZero));
    }
    list
}

fn moving_source(frames: u32, width: f64, height: f64) -> VecFrameSource {
    VecFrameSource::new(Size::new(width, height), 24.0, (0..frames).map(|t| moving_frame(t, width, height)).collect(), ImageStore::new())
}

/// Frames of very different sizes on a transparent stage, so trimming and packing have real work to do.
fn varied_source(frames: u32) -> VecFrameSource {
    let list = |t: u32| {
        let mut list = DisplayList::new(Size::new(128.0, 128.0), None);
        let r = 6.0 + noise(t) * 50.0;
        list.nodes.push(fill(circle(64.0, 64.0, r), Paint::Solid(Rgba8::new(255, 128, 0, 200)), FillRule::NonZero));
        list
    };
    VecFrameSource::new(Size::new(128.0, 128.0), 24.0, (0..frames).map(list).collect(), ImageStore::new())
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3))
}

/// One timed operation; it returns its output bytes so a checksum can be taken.
type Runner = Box<dyn Fn() -> Vec<u8>>;

/// A case builds its input only when it is selected, so profiling one case does not pay for the others' setup.
struct Case {
    name: &'static str,
    build: fn() -> Runner,
}

fn svg_of(list: DisplayList, images: ImageStore) -> Runner {
    Box::new(move || display_list_to_svg(&list, &images, &SvgOptions::default()).unwrap().into_bytes())
}

fn noisy_rgba() -> Vec<u8> {
    (0..1920_u32 * 1080).flat_map(|i| [(noise(i) * 255.0) as u8, (noise(i + 7) * 255.0) as u8, (noise(i + 99) * 255.0) as u8, 255]).collect()
}

const CASES: [Case; 8] = [
    Case { name: "svg_busy_2400_shapes", build: || svg_of(busy_list(2400), ImageStore::new()) },
    Case { name: "svg_mesh_5000_triangles", build: || svg_of(mesh_list(), ImageStore::new()) },
    Case {
        name: "svg_bitmaps_embedded",
        build: || {
            let mut images = ImageStore::new();
            let list = bitmap_list(&mut images);
            svg_of(list, images)
        },
    },
    Case {
        name: "svg_animated_smil_48_frames",
        build: || {
            let source = moving_source(48, 320.0, 180.0);
            Box::new(move || export_animated_svg(&source, &AnimatedSvgOptions::default(), &mut |_| true).unwrap().into_bytes())
        },
    },
    Case {
        name: "gif_24_frames_480x270",
        build: || {
            let source = moving_source(24, 480.0, 270.0);
            Box::new(move || {
                let mut out = Vec::new();
                export_gif(&source, &mut out, &GifOptions::default(), &mut |_| true).unwrap();
                out
            })
        },
    },
    Case {
        name: "png_encode_1080p_smooth",
        build: || {
            let big = render_frame(&moving_source(1, 1920.0, 1080.0), 0, &RasterOptions::new()).unwrap();
            Box::new(move || encode_png(big.width, big.height, &big.rgba).unwrap())
        },
    },
    Case {
        name: "png_encode_1080p_noise",
        build: || {
            let noisy = noisy_rgba();
            Box::new(move || encode_png(1920, 1080, &noisy).unwrap())
        },
    },
    Case {
        name: "sheet_300_frames_trim_maxrects",
        build: || {
            let source = varied_source(300);
            Box::new(move || {
                let options = SheetOptions { trim: true, layout: SheetLayout::MaxRects, max_size: 4096, ..SheetOptions::default() };
                let sheet = build_sprite_sheet(&source, &options, &mut |_| true).unwrap();
                let mut bytes = sheet.json().unwrap().into_bytes();
                bytes.extend_from_slice(&sheet.image.rgba);
                bytes
            })
        },
    },
];

fn export_benches(c: &mut Criterion) {
    let fixed = std::env::var("BENCH_FIXED").ok().and_then(|v| v.parse::<u32>().ok());
    let only = std::env::var("BENCH_ONLY").ok();
    let mut group = c.benchmark_group("export");
    group.sample_size(10);
    for case in &CASES {
        if fixed.is_some() && !only.as_deref().is_some_and(|only| case.name.contains(only)) {
            continue;
        }
        let run = (case.build)();
        if std::env::var_os("BENCH_CHECKSUM").is_some() {
            eprintln!("checksum {} {:016x}", case.name, hash(&run()));
        }
        if let Some(count) = fixed {
            for _ in 0..count {
                black_box(run());
            }
            continue;
        }
        group.bench_function(case.name, |b| b.iter(|| black_box(run())));
    }
    group.finish();
}

criterion_group!(benches, export_benches);
criterion_main!(benches);
