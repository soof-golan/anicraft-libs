//! Predict / render / inspect fixtures for the bitmap exporters (animated SVG, GIF, sprite sheets, PNG); see
//! `tests/fixtures/visual/{svg-animated,gif-alpha-edges,sheet-layouts,png-alpha-scale}.expect.md`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::suboptimal_flops,
    clippy::missing_const_for_fn,
    clippy::chunks_exact_to_as_chunks,
    clippy::redundant_clone
)]

use crate::common;

use std::collections::BTreeSet;

use anim_core::kurbo::{Circle, Shape};
use anim_core::{Affine, Rgba8, Size};
use anim_display::{DisplayList, Fill, FillRule, Gradient, GradientStop, ImageStore, Interpolation, Node, Paint, Spread};
use anim_export::{
    AnimatedSvgOptions, ExportBackground, ExportError, GifLoop, GifOptions, PngSequenceOptions, RasterOptions, RgbaFrame, SheetLayout, SheetOptions,
    SvgAnimation, VecFrameSource, build_sprite_sheet, export_animated_svg, export_gif_file, export_png_sequence, frame_delays_cs, no_progress, render_frame,
};
use common::vis::{chrome_raster, compare, crop_zoom, ffmpeg_frames, flatten, miss, out_dir, px, save};
use common::{rect_path, solid_rect};

// ---------------------------------------------------------------------------------------------------------------------
// Animated SVG

const X: [u32; 8] = [0, 8, 16, 24, 24, 40, 16, 56];

fn animated_source() -> VecFrameSource {
    let size = Size::new(80.0, 40.0);
    let frames = X
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let mut list = DisplayList::new(size, Some(Rgba8::WHITE));
            list.nodes.push(solid_rect(f64::from(x), 5.0, 10.0, 10.0, Rgba8::opaque(255, 0, 0)));
            if i == 0 || i == 7 {
                list.nodes.push(solid_rect(70.0, 30.0, 6.0, 6.0, Rgba8::opaque(0, 0, 255)));
            }
            list
        })
        .collect();
    VecFrameSource::new(size, 10.0, frames, ImageStore::new())
}

fn is_red(p: [u8; 4]) -> bool {
    p[0] > 230 && p[1] < 30 && p[2] < 30
}

fn is_white(p: [u8; 4]) -> bool {
    p[0] > 240 && p[1] > 240 && p[2] > 240
}

#[test]
fn animated_svg_shows_the_right_frame_at_each_time() {
    let source = animated_source();
    for animation in [SvgAnimation::Smil, SvgAnimation::Css] {
        let name = format!("svg-animated-{animation:?}").to_lowercase();
        let options = AnimatedSvgOptions { animation, ..AnimatedSvgOptions::default() };
        let svg = export_animated_svg(&source, &options, &mut no_progress).unwrap();
        std::fs::write(out_dir().join(format!("{name}.svg")), &svg).unwrap();
        let doc = roxmltree::Document::parse(&svg).unwrap();
        assert_eq!(doc.root_element().attribute("viewBox"), Some("0 0 80 40"));
        assert_eq!(svg.matches("id=\"frame").count(), 7, "{name}: hold merged, non-adjacent repeat kept");
        let total = if animation == SvgAnimation::Smil { "dur=\"0.8s\"" } else { " 0.8s step-end" };
        assert!(svg.contains(total), "{name}: total duration 0.8 s");

        let mut shots = Vec::new();
        for i in 0..8u32 {
            let ms = i * 100 + 50;
            let Some(shot) = chrome_raster(&format!("{name}-{i}"), &svg, 80, 40, Some(ms)) else { return };
            let mut problems = Vec::new();
            if !is_red(px(&shot, X[i as usize] + 5, 10)) {
                problems.push(format!("frame {i} at {ms} ms: red square missing at x={}: {:?}", X[i as usize] + 5, px(&shot, X[i as usize] + 5, 10)));
            }
            for (j, &xj) in X.iter().enumerate() {
                if xj.abs_diff(X[i as usize]) >= 10 && !is_white(px(&shot, xj + 5, 10)) {
                    problems.push(format!("frame {i} at {ms} ms: stray ink where frame {j} would be (x={}): {:?}", xj + 5, px(&shot, xj + 5, 10)));
                }
            }
            let blue = px(&shot, 72, 32);
            let want_blue = i == 0 || i == 7;
            if want_blue != (blue[2] > 200 && blue[0] < 60) || (!want_blue && !is_white(blue)) {
                problems.push(format!("frame {i}: blue square {blue:?}, want visible={want_blue}"));
            }
            assert!(problems.is_empty(), "{name}: {}", problems.join("\n"));
            shots.push(shot);
        }
        // A contact sheet for the eye.
        let mut sheet = RgbaFrame { width: 80 * 4, height: 40 * 2, rgba: vec![255; 80 * 4 * 40 * 2 * 4] };
        for (i, shot) in shots.iter().enumerate() {
            for y in 0..40 {
                for x in 0..80 {
                    let (dx, dy) = ((i % 4) as u32 * 80 + x, (i / 4) as u32 * 40 + y);
                    let o = ((dy * sheet.width + dx) * 4) as usize;
                    sheet.rgba[o..o + 4].copy_from_slice(&px(shot, x, y));
                }
            }
        }
        save(&format!("{name}-contact"), &sheet);

        // Played once: the last frame is held after the end.
        let once = AnimatedSvgOptions { animation, repeat: false, ..AnimatedSvgOptions::default() };
        let svg = export_animated_svg(&source, &once, &mut no_progress).unwrap();
        let shot = chrome_raster(&format!("{name}-once"), &svg, 80, 40, Some(1500)).unwrap();
        assert!(is_red(px(&shot, 61, 10)), "{name}: final frame held (red at x=61), got {:?}", px(&shot, 61, 10));
        assert!(!is_white(px(&shot, 72, 32)), "{name}: final frame held (blue square)");
        assert!(is_white(px(&shot, 5, 10)), "{name}: first frame is gone after the end");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// GIF

fn disc_source(count: u32, fps: f64) -> VecFrameSource {
    let size = Size::new(40.0, 40.0);
    let frames = (0..count)
        .map(|i| {
            let mut list = DisplayList::new(size, None);
            let path = Circle::new((14.0 + 2.0 * f64::from(i), 20.0), 10.5).to_path(0.01);
            list.nodes.push(Node::Fill(Fill { transform: Affine::IDENTITY, path, rule: FillRule::NonZero, paint: Paint::Solid(Rgba8::opaque(255, 0, 0)) }));
            list
        })
        .collect();
    VecFrameSource::new(size, fps, frames, ImageStore::new())
}

fn decode_gif(bytes: &[u8]) -> (Vec<(Vec<u8>, u16, Vec<u8>, Option<u8>)>, u16, u16) {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(bytes).unwrap();
    let (w, h) = (decoder.width(), decoder.height());
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        assert_eq!((frame.width, frame.height, frame.left, frame.top), (w, h, 0, 0), "frames cover the canvas");
        frames.push((frame.buffer.to_vec(), frame.delay, frame.palette.clone().unwrap_or_default(), frame.transparent));
    }
    (frames, w, h)
}

#[test]
fn gif_alpha_edges() {
    let source = disc_source(6, 24.0);
    let path = out_dir().join("gif-alpha-edges.gif");
    export_gif_file(&source, &path, &GifOptions::default(), &mut no_progress).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "loop forever by default");
    let (frames, w, h) = decode_gif(&bytes);
    assert_eq!((frames.len(), w, h), (6, 40, 40));
    assert_eq!(frames.iter().map(|f| f.1).collect::<Vec<_>>(), vec![4, 4, 5, 4, 4, 4], "cumulative rounding of 100/24 cs");
    assert_eq!(frame_delays_cs(6, 24.0, 2).unwrap(), vec![4, 4, 5, 4, 4, 4]);

    let mut partial = 0;
    for (i, (buffer, _, palette, transparent)) in frames.iter().enumerate() {
        let direct = render_frame(&source, i as u32, &RasterOptions::new()).unwrap();
        let mut bad = Vec::new();
        for y in 0..40u32 {
            for x in 0..40u32 {
                let g = [0, 1, 2, 3].map(|c| buffer[((y * 40 + x) * 4) as usize + c]);
                let a = px(&direct, x, y)[3];
                if a > 0 && a < 255 {
                    partial += 1;
                }
                if (a < 128) != (g[3] == 0) {
                    bad.push(format!("({x},{y}) direct alpha {a} but gif alpha {}", g[3]));
                } else if g[3] != 0 && g[..3] != [255, 0, 0] {
                    bad.push(format!("({x},{y}) direct alpha {a}, gif colour {:?} is not pure red", &g[..3]));
                }
            }
        }
        assert!(bad.is_empty(), "frame {i}: {} wrong pixels, e.g. {:?}", bad.len(), &bad[..bad.len().min(6)]);
        let colours: BTreeSet<[u8; 3]> =
            palette.chunks_exact(3).enumerate().filter(|(n, _)| Some(*n as u8) != *transparent).map(|(_, c)| [c[0], c[1], c[2]]).collect();
        assert!(
            colours.is_empty() || colours == BTreeSet::from([[255, 0, 0]]),
            "frame {i}: palette (without the transparent index) is exactly red, got {colours:?}"
        );
    }
    assert!(partial > 60, "the fixture really has anti-aliased edge pixels ({partial})");
    // Frame 0 centre and corner.
    assert_eq!(&frames[0].0[((20 * 40 + 14) * 4) as usize..][..4], &[255, 0, 0, 255]);
    assert_eq!(frames[0].0[3], 0);

    // As a real decoder composites it (disposal applied): no trail of earlier discs.
    if let Some(composited) = ffmpeg_frames("gif-alpha-edges", &path) {
        assert_eq!(composited.len(), 6);
        let f3 = &composited[3];
        save("gif-alpha-edges-frame3", f3);
        save("gif-alpha-edges-frame3-x8", &crop_zoom(f3, 0, 8, 36, 24, 8));
        assert_eq!(px(f3, 20, 20)[3], 255, "frame 3 disc centre (20,20)");
        assert_eq!(px(f3, 4, 20)[3], 0, "frame 0's leftmost disc pixels do not linger in frame 3");
        let direct3 = render_frame(&source, 3, &RasterOptions::new()).unwrap();
        let mut wrong = 0;
        for y in 0..40 {
            for x in 0..40 {
                wrong += u32::from((px(&direct3, x, y)[3] < 128) != (px(f3, x, y)[3] == 0));
            }
        }
        assert_eq!(wrong, 0, "composited frame 3 differs from the thresholded direct render");
    }

    // Loop modes.
    let once = out_dir().join("gif-once.gif");
    export_gif_file(&source, &once, &GifOptions { loop_mode: GifLoop::Once, ..GifOptions::default() }, &mut no_progress).unwrap();
    assert!(!std::fs::read(&once).unwrap().windows(11).any(|w| w == b"NETSCAPE2.0"));

    // 60 fps cannot be represented: every delay is the 2 cs minimum.
    assert!(frame_delays_cs(30, 60.0, 2).unwrap().iter().all(|&d| d == 2));
    assert_eq!(frame_delays_cs(50, 50.0, 2).unwrap().iter().map(|&d| u32::from(d)).sum::<u32>(), 100);
    assert_eq!(frame_delays_cs(24, 24.0, 2).unwrap().iter().map(|&d| u32::from(d)).sum::<u32>(), 100);
    assert_eq!(frame_delays_cs(30, 29.97, 2).unwrap().iter().map(|&d| u32::from(d)).sum::<u32>(), 100);
}

#[test]
fn gif_gradient_palette_and_transparent_corner() {
    let size = Size::new(64.0, 64.0);
    let mut list = DisplayList::new(size, None);
    let across = Gradient {
        transform: Affine::translate((32.0, 32.0)) * Affine::scale(64.0 / 1638.4),
        stops: vec![
            GradientStop { offset: 0.0, color: Rgba8::opaque(255, 0, 0) },
            GradientStop { offset: 0.5, color: Rgba8::opaque(0, 255, 0) },
            GradientStop { offset: 1.0, color: Rgba8::opaque(0, 0, 255) },
        ],
        spread: Spread::Pad,
        interpolation: Interpolation::Rgb,
        focal: 0.0,
    };
    // Vertical overlay: a rotated gradient from transparent white to opaque black.
    let down = Gradient {
        transform: Affine::translate((32.0, 32.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale(64.0 / 1638.4),
        stops: vec![GradientStop { offset: 0.0, color: Rgba8::new(255, 255, 255, 0) }, GradientStop { offset: 1.0, color: Rgba8::new(0, 0, 0, 200) }],
        spread: Spread::Pad,
        interpolation: Interpolation::Rgb,
        focal: 0.0,
    };
    for (x, y, w, h) in [(16.0, 0.0, 48.0, 64.0), (0.0, 16.0, 16.0, 48.0)] {
        for g in [&across, &down] {
            list.nodes.push(Node::Fill(Fill {
                transform: Affine::IDENTITY,
                path: rect_path(x, y, w, h),
                rule: FillRule::NonZero,
                paint: Paint::Linear(g.clone()),
            }));
        }
    }
    let source = VecFrameSource::new(size, 10.0, vec![list], ImageStore::new());
    for speed in [1u8, 3, 10, 30] {
        let probe = out_dir().join("gif-gradient-speed.gif");
        export_gif_file(&source, &probe, &GifOptions { speed, ..GifOptions::default() }, &mut no_progress).unwrap();
        let (frames, _, _) = decode_gif(&std::fs::read(&probe).unwrap());
        let direct = render_frame(&source, 0, &RasterOptions::new()).unwrap();
        let (mut sum, mut max, mut n) = (0u64, 0u8, 0u64);
        for y in 16..64 {
            for x in 16..64 {
                let (d, g) = (px(&direct, x, y), [0, 1, 2].map(|c| frames[0].0[((y * 64 + x) * 4) as usize + c]));
                for c in 0..3 {
                    let e = d[c].abs_diff(g[c]);
                    sum += u64::from(e);
                    max = max.max(e);
                    n += 1;
                }
            }
        }
        eprintln!("gif speed {speed}: mean {:.2} max {max}", sum as f64 / n as f64);
    }
    let path = out_dir().join("gif-gradient.gif");
    export_gif_file(&source, &path, &GifOptions::default(), &mut no_progress).unwrap();
    let (frames, _, _) = decode_gif(&std::fs::read(&path).unwrap());
    let direct = render_frame(&source, 0, &RasterOptions::new()).unwrap();
    let gif = RgbaFrame { width: 64, height: 64, rgba: frames[0].0.clone() };
    save("gif-gradient-direct", &direct);
    save("gif-gradient-gif", &gif);
    let distinct: BTreeSet<[u8; 3]> = direct.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect();
    assert!(distinct.len() > 256, "fixture has more than 256 colours ({})", distinct.len());
    let (mut sum, mut max, mut n) = (0u64, 0u8, 0u64);
    for y in 0..64 {
        for x in 0..64 {
            let (d, g) = (px(&direct, x, y), px(&gif, x, y));
            if x < 16 && y < 16 {
                assert_eq!(g[3], 0, "transparent corner stays transparent at ({x},{y})");
                continue;
            }
            assert_eq!(g[3], 255, "opaque everywhere else at ({x},{y})");
            for c in 0..3 {
                let e = d[c].abs_diff(g[c]);
                sum += u64::from(e);
                max = max.max(e);
                n += 1;
            }
        }
    }
    let mean = sum as f64 / n as f64;
    eprintln!("gif gradient quantisation error: mean {mean:.2}, max {max}");
    // 256 palette entries over a two-dimensional colour surface of about 360 units across leave cells of about 22 units, so the
    // best possible mean error is near 4; NeuQuant at the default speed measures 8 (speed 1: 5). See the expect file.
    assert!(mean <= 10.0 && max <= 64, "quantisation too coarse: mean {mean:.2}, max {max}");
}

// ---------------------------------------------------------------------------------------------------------------------
// Sprite sheets

const STAGE: (u32, u32) = (24, 16);
const FRAME_RECTS: [Option<(u32, u32, u32, u32, [u8; 3])>; 7] = [
    Some((0, 0, 24, 16, [255, 0, 0])),
    Some((4, 3, 10, 6, [0, 255, 0])),
    Some((20, 12, 4, 4, [0, 0, 255])),
    None,
    Some((11, 7, 1, 1, [255, 255, 0])),
    Some((2, 2, 20, 3, [255, 0, 255])),
    Some((0, 10, 3, 6, [0, 255, 255])),
];

fn sheet_source() -> VecFrameSource {
    let size = Size::new(f64::from(STAGE.0), f64::from(STAGE.1));
    let frames = FRAME_RECTS
        .iter()
        .map(|r| {
            let mut list = DisplayList::new(size, None);
            if let Some((x, y, w, h, c)) = r {
                list.nodes.push(solid_rect(f64::from(*x), f64::from(*y), f64::from(*w), f64::from(*h), Rgba8::opaque(c[0], c[1], c[2])));
            }
            list
        })
        .collect();
    VecFrameSource::new(size, 12.0, frames, ImageStore::new())
}

fn key(n: u32) -> String {
    format!("frame_{n:04}.png")
}

fn check_sheet(label: &str, options: &SheetOptions) -> anim_export::SpriteSheet {
    let source = sheet_source();
    let sheet = build_sprite_sheet(&source, options, &mut no_progress).unwrap();
    save(&format!("sheet-{label}"), &sheet.image);
    let meta = &sheet.metadata;
    assert_eq!((meta.meta.size.w, meta.meta.size.h), (sheet.image.width, sheet.image.height), "{label}: meta.size");
    assert_eq!(meta.meta.fps, 12.0);
    let names: Vec<String> = (1..=7).map(key).collect();
    assert_eq!(meta.frames.keys().cloned().collect::<Vec<_>>(), names, "{label}: names");
    assert_eq!(meta.animations.get("all"), Some(&names), "{label}: animation order");
    let mut covered = vec![false; (sheet.image.width * sheet.image.height) as usize];
    let rects: Vec<_> = names.iter().map(|n| meta.frames[n].frame).collect();
    for (i, name) in names.iter().enumerate() {
        let f = &meta.frames[name];
        assert_eq!((f.source_size.w, f.source_size.h), STAGE, "{label}: sourceSize of {name}");
        assert!(!f.rotated);
        let r = f.frame;
        assert!(r.x + r.w <= sheet.image.width && r.y + r.h <= sheet.image.height, "{label}: {name} inside the sheet");
        for (j, o) in rects.iter().enumerate().skip(i + 1) {
            let gap = options.padding;
            let apart = r.x + r.w + gap <= o.x || o.x + o.w + gap <= r.x || r.y + r.h + gap <= o.y || o.y + o.h + gap <= r.y;
            assert!(apart, "{label}: frames {i} and {j} are closer than the padding {gap}: {r:?} {o:?}");
        }
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                covered[(y * sheet.image.width + x) as usize] = true;
            }
        }
        // Rebuild the original frame from the sheet and compare with the direct render.
        let s = f.sprite_source_size;
        assert_eq!((s.w, s.h), (r.w, r.h), "{label}: sprite source size matches the cell");
        let mut rebuilt = RgbaFrame { width: STAGE.0, height: STAGE.1, rgba: vec![0; (STAGE.0 * STAGE.1 * 4) as usize] };
        for y in 0..r.h {
            for x in 0..r.w {
                let o = (((s.y + y) * STAGE.0 + s.x + x) * 4) as usize;
                rebuilt.rgba[o..o + 4].copy_from_slice(&px(&sheet.image, r.x + x, r.y + y));
            }
        }
        let direct = render_frame(&source, i as u32, &RasterOptions::new()).unwrap();
        assert_eq!(rebuilt, direct, "{label}: frame {i} rebuilt from the sheet differs from the direct render");
    }
    for y in 0..sheet.image.height {
        for x in 0..sheet.image.width {
            if !covered[(y * sheet.image.width + x) as usize] {
                assert_eq!(px(&sheet.image, x, y)[3], 0, "{label}: ink outside every cell at ({x},{y})");
            }
        }
    }
    sheet
}

#[test]
fn sheet_layouts() {
    let base = SheetOptions::default();
    // (a) grid, automatic columns, padding 2, no trim.
    let a = check_sheet("a-grid-auto", &SheetOptions { layout: SheetLayout::Grid { columns: None }, padding: 2, trim: false, ..base.clone() });
    assert_eq!((a.image.width, a.image.height), (180, 16), "smallest grid for 7 cells is one row");
    for i in 0..7u32 {
        let f = &a.metadata.frames[&key(i + 1)];
        assert_eq!((f.frame.x, f.frame.y, f.trimmed), (i * 26, 0, false));
    }
    // (b) three columns, no padding.
    let b = check_sheet("b-grid-3", &SheetOptions { layout: SheetLayout::Grid { columns: Some(3) }, padding: 0, trim: false, ..base.clone() });
    assert_eq!((b.image.width, b.image.height), (72, 48));
    assert_eq!((b.metadata.frames[&key(5)].frame.x, b.metadata.frames[&key(5)].frame.y), (24, 16));
    assert_eq!((b.metadata.frames[&key(7)].frame.x, b.metadata.frames[&key(7)].frame.y), (0, 32));
    // (c) max-rects with trimming.
    let c = check_sheet("c-maxrects-trim", &SheetOptions { layout: SheetLayout::MaxRects, padding: 1, trim: true, ..base.clone() });
    let trimmed: Vec<bool> = (1..=7).map(|n| c.metadata.frames[&key(n)].trimmed).collect();
    assert_eq!(trimmed, vec![false, true, true, true, true, true, true]);
    let empty = &c.metadata.frames[&key(4)];
    assert_eq!((empty.frame.w, empty.frame.h, empty.sprite_source_size.x, empty.sprite_source_size.y), (1, 1, 0, 0));
    let one = &c.metadata.frames[&key(5)];
    assert_eq!((one.sprite_source_size.x, one.sprite_source_size.y, one.frame.w, one.frame.h), (11, 7, 1, 1));
    let corner = &c.metadata.frames[&key(3)];
    assert_eq!((corner.sprite_source_size.x, corner.sprite_source_size.y, corner.frame.w, corner.frame.h), (20, 12, 4, 4));
    // (d) max-rects, no padding, power of two.
    let d = check_sheet("d-maxrects-pot", &SheetOptions { layout: SheetLayout::MaxRects, padding: 0, trim: true, power_of_two: true, ..base.clone() });
    assert!(d.image.width.is_power_of_two() && d.image.height.is_power_of_two(), "{}x{}", d.image.width, d.image.height);
    // (e), (f) do not fit.
    let source = sheet_source();
    let e = build_sprite_sheet(&source, &SheetOptions { layout: SheetLayout::MaxRects, trim: true, max_size: 12, ..base.clone() }, &mut no_progress);
    assert!(matches!(e, Err(ExportError::SheetTooSmall { .. })), "{e:?}");
    let f = build_sprite_sheet(&source, &SheetOptions { layout: SheetLayout::Grid { columns: None }, max_size: 20, ..base.clone() }, &mut no_progress);
    assert!(matches!(f, Err(ExportError::SheetTooSmall { .. })), "{f:?}");
}

// ---------------------------------------------------------------------------------------------------------------------
// PNG

fn alpha_rect_source(background: Option<Rgba8>) -> VecFrameSource {
    let size = Size::new(7.0, 5.0);
    let mut list = DisplayList::new(size, background);
    list.nodes.push(solid_rect(1.0, 1.0, 3.0, 3.0, Rgba8::new(200, 100, 50, 128)));
    VecFrameSource::new(size, 24.0, vec![list], ImageStore::new())
}

#[test]
fn png_alpha_and_scale() {
    let close = |p: [u8; 4], want: [u8; 4], tol: u8, what: &str| {
        assert!((0..4).all(|c| p[c].abs_diff(want[c]) <= tol), "{what}: {p:?} want {want:?} +-{tol}");
    };
    // 1. Scale 1, transparent.
    let source = alpha_rect_source(None);
    let f = render_frame(&source, 0, &RasterOptions::new()).unwrap();
    assert_eq!((f.width, f.height), (7, 5));
    close(px(&f, 2, 2), [200, 100, 50, 128], 3, "straight colour, alpha 128");
    assert_eq!(px(&f, 0, 0), [0, 0, 0, 0]);
    let png = common::vis::decode_png(&f.to_png().unwrap());
    assert_eq!(png, f, "PNG round trip is lossless");
    // 2. Scale 1.5.
    let f = render_frame(&source, 0, &RasterOptions::new().with_scale(1.5)).unwrap();
    assert_eq!((f.width, f.height), (11, 8));
    close(px(&f, 3, 3), [200, 100, 50, 128], 3, "inside at 1.5");
    close(px(&f, 1, 1), [200, 100, 50, 32], 8, "quarter covered pixel at 1.5 has alpha 32");
    // 3. Solid background.
    let f = render_frame(&source, 0, &RasterOptions::new().with_scale(1.5).with_background(ExportBackground::Solid(Rgba8::opaque(10, 20, 30)))).unwrap();
    save("png-solid-bg", &f);
    close(px(&f, 3, 3), [105, 60, 40, 255], 2, "blended over solid");
    close(px(&f, 10, 7), [10, 20, 30, 255], 0, "last column and row past the scaled stage edge are background too");
    close(px(&f, 10, 0), [10, 20, 30, 255], 0, "last column");
    // 4. Stage background white.
    let f = render_frame(&alpha_rect_source(Some(Rgba8::WHITE)), 0, &RasterOptions::new().with_scale(1.5)).unwrap();
    save("png-stage-bg", &f);
    for (x, y) in [(10, 0), (10, 7), (0, 7), (5, 7), (10, 4)] {
        assert_eq!(px(&f, x, y), [255, 255, 255, 255], "stage background reaches the output edge at ({x},{y})");
    }
    // 5. Naming.
    let o = |prefix: &str, start, digits| PngSequenceOptions {
        prefix: prefix.to_owned(),
        start_number: start,
        min_digits: digits,
        ..PngSequenceOptions::default()
    };
    assert_eq!(o("walk_", 1, 2).file_name(0, 12), "walk_01.png");
    assert_eq!(o("walk_", 1, 2).file_name(11, 12), "walk_12.png");
    assert_eq!(o("f", 0, 1).file_name(0, 120), "f000.png");
    assert_eq!(o("f", 0, 1).file_name(119, 120), "f119.png");
    assert_eq!((0..3).map(|i| o("s", 98, 3).file_name(i, 3)).collect::<Vec<_>>(), vec!["s098.png".to_owned(), "s099.png".to_owned(), "s100.png".to_owned()]);
    // 6. Ranges.
    let moving = common::moving_square(10);
    let dir = tempfile::tempdir().unwrap();
    let opts = PngSequenceOptions { raster: RasterOptions::new().with_frames(5..99), ..PngSequenceOptions::default() };
    let report = export_png_sequence(&moving, dir.path(), &opts, &mut no_progress).unwrap();
    assert_eq!(report.frames, 5);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 5);
    let bad = PngSequenceOptions { raster: RasterOptions::new().with_frames(12..20), ..PngSequenceOptions::default() };
    assert!(matches!(export_png_sequence(&moving, dir.path(), &bad, &mut no_progress), Err(ExportError::FrameOutOfRange { .. })));
    let _ = (compare, flatten, miss, chrome_raster);
}
