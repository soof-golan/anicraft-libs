//! Writes every export format for a small built-in animation, so a person can look at the results.
//!
//! `cargo run -p anim-export --example export_demo -- OUT_DIR`
//!
//! Produces `frame_0001.png`..., `demo.gif`, `sheet.png` + `sheet.json`, `demo.svg` (frame 12), `demo_animated.svg`
//! (SMIL) and `demo_animated_css.svg`.

// Examples may panic and use plain arithmetic; the no-panic rules are for shipped code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::print_stdout,
    clippy::many_single_char_names,
    clippy::too_many_lines
)]

use std::path::PathBuf;

use anim_core::{Affine, BezPath, BlendMode, ColorTransform, Rgba8, Size};
use anim_display::{
    DisplayList, Fill, FillRule, Filter, Gradient, GradientStop, Group, ImageStore, Interpolation, Node, Paint, Spread, Stroke, StrokeStyle,
};
use anim_export::{
    AnimatedSvgOptions, FrameSource, GifOptions, PngSequenceOptions, RasterOptions, SheetOptions, SvgAnimation, SvgOptions, VecFrameSource, export_animated_svg_file,
    export_gif_file, export_png_sequence, export_sprite_sheet, export_svg_file, no_progress,
};

fn circle(r: f64) -> BezPath {
    // Four cubic arcs.
    let k = 0.552_284_749_8 * r;
    let mut p = BezPath::new();
    p.move_to((r, 0.0));
    p.curve_to((r, k), (k, r), (0.0, r));
    p.curve_to((-k, r), (-r, k), (-r, 0.0));
    p.curve_to((-r, -k), (-k, -r), (0.0, -r));
    p.curve_to((k, -r), (r, -k), (r, 0.0));
    p.close_path();
    p
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x, y));
    p.line_to((x + w, y));
    p.line_to((x + w, y + h));
    p.line_to((x, y + h));
    p.close_path();
    p
}

fn gradient(stops: &[(f32, Rgba8)], transform: Affine) -> Gradient {
    Gradient {
        transform,
        stops: stops.iter().map(|&(offset, color)| GradientStop { offset, color }).collect(),
        spread: Spread::Pad,
        interpolation: Interpolation::Rgb,
        focal: 0.0,
    }
}

fn frame(i: u32, count: u32) -> DisplayList {
    let t = f64::from(i) / f64::from(count);
    let mut list = DisplayList::new(Size::new(320.0, 180.0), Some(Rgba8::opaque(24, 28, 44)));
    // Sky gradient.
    list.nodes.push(Node::Fill(Fill {
        transform: Affine::IDENTITY,
        path: rect(0.0, 0.0, 320.0, 180.0),
        rule: FillRule::NonZero,
        paint: Paint::Linear(gradient(
            &[(0.0, Rgba8::opaque(30, 40, 90)), (1.0, Rgba8::opaque(240, 130, 90))],
            Affine::new([0.0, 0.1, -0.1, 0.0, 160.0, 90.0]) * Affine::scale(1.0),
        )),
    }));
    // Ground.
    list.nodes.push(Node::Fill(Fill {
        transform: Affine::IDENTITY,
        path: rect(0.0, 140.0, 320.0, 40.0),
        rule: FillRule::NonZero,
        paint: Paint::Solid(Rgba8::opaque(20, 60, 40)),
    }));
    // A bouncing ball with a radial highlight, a drop shadow and a squash.
    let x = t.mul_add(240.0, 40.0);
    let bounce = (t * std::f64::consts::TAU * 2.0).sin().abs();
    let y = bounce.mul_add(-80.0, 130.0);
    let squash = (1.0 - bounce).mul_add(0.25, 1.0);
    list.nodes.push(Node::Group(Group {
        transform: Affine::translate((x, y)) * Affine::scale_non_uniform(squash, 2.0 - squash),
        filters: vec![Filter::DropShadow {
            color: Rgba8::new(0, 0, 0, 150),
            angle_deg: 60.0,
            distance: 6.0,
            blur_x: 6.0,
            blur_y: 6.0,
            strength: 1.0,
            inner: false,
            knockout: false,
            hide_object: false,
        }],
        children: vec![
            Node::Fill(Fill {
                transform: Affine::IDENTITY,
                path: circle(18.0),
                rule: FillRule::NonZero,
                paint: Paint::Radial(Gradient {
                    focal: -0.4,
                    ..gradient(&[(0.0, Rgba8::opaque(255, 240, 200)), (1.0, Rgba8::opaque(220, 50, 60))], Affine::scale(18.0 / anim_display::GRADIENT_EXTENT))
                }),
            }),
            Node::Stroke(Stroke {
                transform: Affine::IDENTITY,
                path: circle(18.0),
                style: StrokeStyle { width: 2.0, ..StrokeStyle::default() },
                paint: Paint::Solid(Rgba8::opaque(120, 20, 30)),
            }),
        ],
        ..Group::default()
    }));
    // A translucent multiply-blended band, rotating.
    list.nodes.push(Node::Group(Group {
        transform: Affine::translate((160.0, 60.0)) * Affine::rotate(t * std::f64::consts::TAU),
        color: ColorTransform::alpha(0.6),
        blend: BlendMode::Multiply,
        children: vec![Node::Fill(Fill {
            transform: Affine::IDENTITY,
            path: rect(-50.0, -6.0, 100.0, 12.0),
            rule: FillRule::NonZero,
            paint: Paint::Solid(Rgba8::opaque(250, 210, 60)),
        })],
        ..Group::default()
    }));
    list
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("usage: export_demo OUT_DIR"));
    let count = 24;
    let source = VecFrameSource::new(Size::new(320.0, 180.0), 24.0, (0..count).map(|i| frame(i, count)).collect(), ImageStore::new());
    let raster = RasterOptions::default().with_scale(2.0);

    let png = export_png_sequence(&source, &out.join("png"), &PngSequenceOptions { raster,..PngSequenceOptions::default() }, &mut no_progress).unwrap();
    println!("png sequence: {} files, {} bytes", png.files.len(), png.bytes);

    let gif = export_gif_file(&source, &out.join("demo.gif"), &GifOptions { raster: RasterOptions::default(), ..GifOptions::default() }, &mut no_progress).unwrap();
    println!("gif: {} frames, {} bytes", gif.frames, gif.bytes);

    let sheet_opts = SheetOptions { raster: RasterOptions::default().with_scale(0.5), padding: 2, ..SheetOptions::default() };
    let sheet = export_sprite_sheet(&source, &out.join("sheet.png"), &out.join("sheet.json"), &sheet_opts, &mut no_progress).unwrap();
    println!("sprite sheet: {} frames, {} bytes", sheet.frames, sheet.bytes);

    export_svg_file(&source, 6, &out.join("demo.svg"), &SvgOptions::default()).unwrap();
    export_animated_svg_file(&source, &out.join("demo_animated.svg"), &AnimatedSvgOptions::default(), &mut no_progress).unwrap();
    export_animated_svg_file(
        &source,
        &out.join("demo_animated_css.svg"),
        &AnimatedSvgOptions { animation: SvgAnimation::Css, ..AnimatedSvgOptions::default() },
        &mut no_progress,
    )
    .unwrap();
    println!("svg written; {} frames at {} fps", source.frame_count(), source.fps());
}
