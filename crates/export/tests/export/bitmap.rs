//! PNG, GIF and sprite sheet exports, decoded back and checked.

// Integration tests may panic and use plain arithmetic; the no-panic rules are for shipped code.
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
    clippy::float_cmp,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::unreachable,
    clippy::manual_is_multiple_of,
    clippy::chunks_exact_to_as_chunks,
    clippy::needless_range_loop
)]

use crate::common;

use std::fs;
use std::io::Cursor;

use anim_export::{
    ExportBackground, ExportError, GifLoop, GifOptions, PngSequenceOptions, RasterOptions, SheetLayout, SheetMetadata, SheetOptions, build_sprite_sheet,
    encode_png, export_gif, export_gif_file, export_png_frame, export_png_sequence, export_sprite_sheet, frame_delays_cs, no_progress, png_frame_bytes,
    render_frame,
};
use common::{RED, WHITE, moving_square};

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn pixel(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn png_frame_has_stage_pixels() {
    let src = moving_square(3);
    let bytes = png_frame_bytes(&src, 1, &RasterOptions::default()).unwrap();
    let (w, h, px) = decode_png(&bytes);
    assert_eq!((w, h), (64, 48));
    assert_eq!(pixel(&px, w, 12, 24), RED.into_array());
    assert_eq!(pixel(&px, w, 2, 2), WHITE.into_array());
}

trait IntoArray {
    fn into_array(self) -> [u8; 4];
}

impl IntoArray for anim_core::Rgba8 {
    fn into_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

#[test]
fn png_frame_scales_and_can_be_transparent() {
    let src = moving_square(1);
    let opts = RasterOptions::default().with_scale(2.0).with_background(ExportBackground::Transparent);
    let (w, h, px) = decode_png(&png_frame_bytes(&src, 0, &opts).unwrap());
    assert_eq!((w, h), (128, 96));
    assert_eq!(pixel(&px, w, 4, 4)[3], 0);
    assert_eq!(pixel(&px, w, 20, 50), RED.into_array());
}

#[test]
fn png_frame_to_file_and_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let src = moving_square(2);
    let path = dir.path().join("nested/one.png");
    let report = export_png_frame(&src, 0, &path, &RasterOptions::default()).unwrap();
    assert_eq!(report.files, vec![path.clone()]);
    assert!(report.bytes > 0);
    assert!(fs::metadata(&path).unwrap().len() > 0);
    assert!(matches!(png_frame_bytes(&src, 2, &RasterOptions::default()), Err(ExportError::FrameOutOfRange { index: 2, count: 2 })));
}

#[test]
fn invalid_settings_are_rejected() {
    let src = moving_square(1);
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let err = render_frame(&src, 0, &RasterOptions::default().with_scale(scale)).unwrap_err();
        assert!(matches!(err, ExportError::InvalidSetting(_)), "{scale}");
    }
    let err = render_frame(&src, 0, &RasterOptions::default().with_scale(1e6)).unwrap_err();
    assert!(matches!(err, ExportError::TooLarge(_)));
}

#[test]
fn png_sequence_names_are_zero_padded_and_pixels_move() {
    let dir = tempfile::tempdir().unwrap();
    let src = moving_square(5);
    let opts = PngSequenceOptions { prefix: "walk_".to_owned(), start_number: 1, min_digits: 3, ..PngSequenceOptions::default() };
    let mut seen = Vec::new();
    let report = export_png_sequence(&src, dir.path(), &opts, &mut |p| {
        seen.push((p.done, p.total));
        true
    })
    .unwrap();
    assert_eq!(report.frames, 5);
    let names: Vec<String> = report.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["walk_001.png", "walk_002.png", "walk_003.png", "walk_004.png", "walk_005.png"]);
    assert_eq!(seen.first(), Some(&(0, 5)));
    assert_eq!(seen.last(), Some(&(5, 5)));
    for (i, file) in report.files.iter().enumerate() {
        let (w, _, px) = decode_png(&fs::read(file).unwrap());
        let x = u32::try_from(i).unwrap() * 8 + 8;
        assert_eq!(pixel(&px, w, x, 24), RED.into_array(), "frame {i}");
    }
}

#[test]
fn png_sequence_range_and_numbering() {
    let dir = tempfile::tempdir().unwrap();
    let src = moving_square(12);
    let opts = PngSequenceOptions { raster: RasterOptions::default().with_frames(8..20), prefix: "f".to_owned(), start_number: 100, min_digits: 1 };
    let report = export_png_sequence(&src, dir.path(), &opts, &mut no_progress).unwrap();
    assert_eq!(report.frames, 4);
    let first = report.files.first().unwrap().file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(first, "f100.png");
}

#[test]
fn png_sequence_cancel_stops_and_keeps_written_files() {
    let dir = tempfile::tempdir().unwrap();
    let src = moving_square(10);
    let mut calls = 0;
    let err = export_png_sequence(&src, dir.path(), &PngSequenceOptions::default(), &mut |_| {
        calls += 1;
        calls <= 3
    })
    .unwrap_err();
    assert!(matches!(err, ExportError::Cancelled));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
fn bad_prefix_and_empty_range_fail() {
    let dir = tempfile::tempdir().unwrap();
    let src = moving_square(2);
    let opts = PngSequenceOptions { prefix: "../evil".to_owned(), ..PngSequenceOptions::default() };
    assert!(matches!(export_png_sequence(&src, dir.path(), &opts, &mut no_progress), Err(ExportError::InvalidSetting(_))));
    let opts = PngSequenceOptions { raster: RasterOptions::default().with_frames(5..9), ..PngSequenceOptions::default() };
    assert!(matches!(export_png_sequence(&src, dir.path(), &opts, &mut no_progress), Err(ExportError::FrameOutOfRange { .. })));
    let empty = anim_export::VecFrameSource::new(anim_core::Size::new(4.0, 4.0), 24.0, Vec::new(), anim_display::ImageStore::new());
    assert!(matches!(export_png_sequence(&empty, dir.path(), &PngSequenceOptions::default(), &mut no_progress), Err(ExportError::NoFrames)));
}

fn decode_gif(bytes: &[u8]) -> (gif::Repeat, u16, u16, Vec<(Vec<u8>, u16)>) {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = options.read_info(Cursor::new(bytes)).unwrap();
    let mut frames = Vec::new();
    while let Some(f) = dec.read_next_frame().unwrap() {
        frames.push((f.buffer.to_vec(), f.delay));
    }
    (dec.repeat(), dec.width(), dec.height(), frames)
}

#[test]
fn gif_has_frames_pixels_delays_and_loops() {
    let src = moving_square(6);
    let mut bytes = Vec::new();
    let report = export_gif(&src, &mut bytes, &GifOptions::default(), &mut no_progress).unwrap();
    assert_eq!(report.frames, 6);
    let (repeat, w, h, frames) = decode_gif(&bytes);
    assert_eq!((w, h), (64, 48));
    assert_eq!(frames.len(), 6);
    assert!(matches!(repeat, gif::Repeat::Infinite));
    for (i, (px, _)) in frames.iter().enumerate() {
        let x = u32::try_from(i).unwrap() * 8 + 8;
        assert_eq!(pixel(px, 64, x, 24), RED.into_array(), "frame {i}");
        assert_eq!(pixel(px, 64, 1, 1), WHITE.into_array());
    }
    let total: u32 = frames.iter().map(|f| u32::from(f.1)).sum();
    assert_eq!(total, 25, "6 frames at 24 fps is 0.25 s");
}

#[test]
fn gif_delays_track_the_frame_rate() {
    let d = frame_delays_cs(24, 24.0, 2).unwrap();
    assert_eq!(d.iter().map(|v| u32::from(*v)).sum::<u32>(), 100);
    assert!(d.iter().all(|v| *v == 4 || *v == 5));
    let d = frame_delays_cs(10, 60.0, 2).unwrap();
    assert!(d.iter().all(|v| *v >= 2));
    assert!(frame_delays_cs(3, 0.0, 2).is_err());
    assert!(frame_delays_cs(3, f64::NAN, 2).is_err());
}

#[test]
fn gif_loop_modes_and_transparency() {
    let src = moving_square(2);
    let once = GifOptions { loop_mode: GifLoop::Once, ..GifOptions::default() };
    let mut bytes = Vec::new();
    export_gif(&src, &mut bytes, &once, &mut no_progress).unwrap();
    assert!(!bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "play-once writes no loop extension");

    let finite = GifOptions { loop_mode: GifLoop::Repeat(3), ..GifOptions::default() };
    let mut bytes = Vec::new();
    export_gif(&src, &mut bytes, &finite, &mut no_progress).unwrap();
    assert!(matches!(decode_gif(&bytes).0, gif::Repeat::Finite(3)));

    let transparent = GifOptions { raster: RasterOptions::default().with_background(ExportBackground::Transparent), ..GifOptions::default() };
    let mut bytes = Vec::new();
    export_gif(&src, &mut bytes, &transparent, &mut no_progress).unwrap();
    let (_, _, _, frames) = decode_gif(&bytes);
    assert_eq!(pixel(&frames[0].0, 64, 1, 1)[3], 0);
    assert_eq!(pixel(&frames[0].0, 64, 12, 24), RED.into_array());
}

#[test]
fn gif_file_cancel_removes_partial_output() {
    let dir = tempfile::tempdir().unwrap();
    let src = moving_square(8);
    let path = dir.path().join("a/out.gif");
    let mut n = 0;
    let err = export_gif_file(&src, &path, &GifOptions::default(), &mut |_| {
        n += 1;
        n < 4
    })
    .unwrap_err();
    assert!(matches!(err, ExportError::Cancelled));
    assert!(!path.exists());

    let report = export_gif_file(&src, &path, &GifOptions::default(), &mut no_progress).unwrap();
    assert_eq!(report.files, vec![path.clone()]);
    assert_eq!(report.bytes, fs::metadata(&path).unwrap().len());
    assert_eq!(decode_gif(&fs::read(&path).unwrap()).3.len(), 8);
}

#[test]
fn gif_rejects_oversized_and_bad_fps() {
    let mut src = moving_square(2);
    src.fps = 0.0;
    assert!(matches!(export_gif(&src, Vec::new(), &GifOptions::default(), &mut no_progress), Err(ExportError::InvalidSetting(_))));
    let src = moving_square(1);
    let huge = GifOptions { raster: RasterOptions::default().with_scale(2000.0), ..GifOptions::default() };
    assert!(matches!(export_gif(&src, Vec::new(), &huge, &mut no_progress), Err(ExportError::TooLarge(_))));
}

fn no_overlap(meta: &SheetMetadata) {
    let rects: Vec<_> = meta.frames.values().map(|f| f.frame).collect();
    for (i, a) in rects.iter().enumerate() {
        assert!(a.x + a.w <= meta.meta.size.w && a.y + a.h <= meta.meta.size.h, "{a:?} outside sheet");
        for b in &rects[i + 1..] {
            let apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
            assert!(apart, "{a:?} overlaps {b:?}");
        }
    }
}

fn check_sheet_pixels(src: &anim_export::VecFrameSource, sheet: &anim_export::SpriteSheet) {
    for (i, name) in sheet.metadata.animations["all"].iter().enumerate() {
        let entry = &sheet.metadata.frames[name];
        let frame = render_frame(src, u32::try_from(i).unwrap(), &RasterOptions::default()).unwrap();
        // Compare every pixel of the placed (possibly trimmed) region with the full frame.
        for y in 0..entry.frame.h {
            for x in 0..entry.frame.w {
                let on_sheet = pixel(&sheet.image.rgba, sheet.image.width, entry.frame.x + x, entry.frame.y + y);
                let original = pixel(&frame.rgba, frame.width, entry.sprite_source_size.x + x, entry.sprite_source_size.y + y);
                assert_eq!(on_sheet, original, "{name} at {x},{y}");
            }
        }
    }
}

#[test]
fn sprite_sheet_grid_places_frames_in_rows() {
    let src = moving_square(7);
    let opts = SheetOptions { layout: SheetLayout::Grid { columns: Some(3) }, padding: 2, ..SheetOptions::default() };
    let sheet = build_sprite_sheet(&src, &opts, &mut no_progress).unwrap();
    assert_eq!(sheet.metadata.frames.len(), 7);
    assert_eq!((sheet.image.width, sheet.image.height), (3 * 64 + 2 * 2, 3 * 48 + 2 * 2));
    let second = &sheet.metadata.frames["frame_0002.png"];
    assert_eq!((second.frame.x, second.frame.y), (66, 0));
    assert_eq!(sheet.metadata.frames["frame_0004.png"].frame.y, 50);
    no_overlap(&sheet.metadata);
    check_sheet_pixels(&src, &sheet);
}

#[test]
fn sprite_sheet_auto_grid_is_compact() {
    let src = moving_square(8);
    let opts = SheetOptions { layout: SheetLayout::Grid { columns: None }, padding: 0, ..SheetOptions::default() };
    let sheet = build_sprite_sheet(&src, &opts, &mut no_progress).unwrap();
    let area = u64::from(sheet.image.width) * u64::from(sheet.image.height);
    assert_eq!(area, 8 * 64 * 48, "8 cells tile exactly");
    no_overlap(&sheet.metadata);
}

#[test]
fn sprite_sheet_max_rects_trims_and_round_trips_json() {
    let src = moving_square(10);
    let opts = SheetOptions { trim: true, padding: 1, power_of_two: true, ..SheetOptions::default() };
    let sheet = build_sprite_sheet(&src, &opts, &mut no_progress).unwrap();
    // Backgrounds are opaque white here, so trimming keeps everything; the packer must still be exact.
    no_overlap(&sheet.metadata);
    check_sheet_pixels(&src, &sheet);
    assert!(sheet.image.width.is_power_of_two() && sheet.image.height.is_power_of_two());

    let json = sheet.json().unwrap();
    let back: SheetMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sheet.metadata);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value["frames"]["frame_0001.png"]["spriteSourceSize"].is_object());
    assert_eq!(value["frames"]["frame_0001.png"]["sourceSize"]["w"], 64);
    assert_eq!(value["meta"]["format"], "RGBA8888");
    assert_eq!(value["animations"]["all"].as_array().unwrap().len(), 10);
}

#[test]
fn sprite_sheet_trims_transparent_borders() {
    let src = moving_square(4);
    let opts =
        SheetOptions { raster: RasterOptions::default().with_background(ExportBackground::Transparent), trim: true, padding: 0, ..SheetOptions::default() };
    let sheet = build_sprite_sheet(&src, &opts, &mut no_progress).unwrap();
    for (name, f) in &sheet.metadata.frames {
        assert!(f.trimmed, "{name}");
        assert_eq!((f.frame.w, f.frame.h), (16, 16));
        assert_eq!((f.source_size.w, f.source_size.h), (64, 48));
        assert_eq!(f.sprite_source_size.y, 16);
    }
    assert_eq!(sheet.metadata.frames["frame_0003.png"].sprite_source_size.x, 16);
    no_overlap(&sheet.metadata);
    check_sheet_pixels(&src, &sheet);
    assert!(u64::from(sheet.image.width) * u64::from(sheet.image.height) < 4 * 64 * 48 / 4);
}

#[test]
fn sprite_sheet_too_small_and_files() {
    let src = moving_square(9);
    let tiny = SheetOptions { max_size: 100, ..SheetOptions::default() };
    assert!(matches!(build_sprite_sheet(&src, &tiny, &mut no_progress), Err(ExportError::SheetTooSmall { frames: 9, max: 100 })));

    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("out/walk.png");
    let json_path = dir.path().join("out/walk.json");
    let report = export_sprite_sheet(&src, &png_path, &json_path, &SheetOptions::default(), &mut no_progress).unwrap();
    assert_eq!(report.files.len(), 2);
    let meta: SheetMetadata = serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
    assert_eq!(meta.meta.image, "walk.png");
    let (w, h, _) = decode_png(&fs::read(&png_path).unwrap());
    assert_eq!((w, h), (meta.meta.size.w, meta.meta.size.h));
}

#[test]
fn sprite_sheet_cancel() {
    let src = moving_square(6);
    let mut n = 0;
    let err = build_sprite_sheet(&src, &SheetOptions::default(), &mut |_| {
        n += 1;
        n < 3
    })
    .unwrap_err();
    assert!(matches!(err, ExportError::Cancelled));
}

#[test]
fn encode_png_rejects_wrong_buffer() {
    assert!(encode_png(2, 2, &[0; 4]).is_err());
    assert!(encode_png(1, 1, &[1, 2, 3, 4]).is_ok());
}

mod packer_props {
    use super::*;
    use anim_core::Size;
    use anim_display::{DisplayList, ImageStore};
    use anim_export::VecFrameSource;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        #[test]
        fn random_frame_sizes_never_overlap(sizes in prop::collection::vec((1u32..40, 1u32..40), 1..14), pad in 0u32..4, grid in any::<bool>()) {
            // A different stage size per frame is not possible, so vary what is drawn and trim instead.
            let stage = Size::new(48.0, 48.0);
            let frames: Vec<DisplayList> = sizes.iter().map(|&(w, h)| {
                let mut l = DisplayList::new(stage, None);
                l.nodes.push(common::solid_rect(3.0, 4.0, f64::from(w), f64::from(h), RED));
                l
            }).collect();
            let src = VecFrameSource::new(stage, 12.0, frames, ImageStore::new());
            let opts = SheetOptions {
                trim: true,
                padding: pad,
                layout: if grid { SheetLayout::Grid { columns: None } } else { SheetLayout::MaxRects },
                ..SheetOptions::default()
            };
            let sheet = build_sprite_sheet(&src, &opts, &mut no_progress).unwrap();
            prop_assert_eq!(sheet.metadata.frames.len(), sizes.len());
            no_overlap(&sheet.metadata);
        }
    }
}
