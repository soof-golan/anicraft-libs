//! Bitmaps, text (including right-to-left) and hostile documents.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::missing_const_for_fn,
    clippy::field_reassign_with_default
)]

use crate::common;

use anim_core::{Affine, Rgba8, kurbo::Shape as _};
use anim_display::{Image, Node, Paint};
use anim_scene::{BitmapInstance, BitmapItem, Document, Element, LibraryItem, MediaStore, TextAlign, TextDirection, TextElement, TextKind, TextRun, TweenType};
use anim_shape::{BitmapFill, EdgeShape, FillStyle, PathInput};
use anim_text::TextEngine;
use anim_timeline::{Evaluator, ImageProvider, PngDecoder, rich_text};
use common::*;
use proptest::prelude::*;

fn png_bytes(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(rgba).unwrap();
    }
    out
}

/// A document with a 2x2 bitmap "pic" (all blue) in `media`, shown scaled by 20 at the origin.
fn bitmap_doc() -> (Document, MediaStore) {
    let mut media = MediaStore::new();
    let id = media.insert(png_bytes(2, 2, &[0, 0, 255, 255].repeat(4))).unwrap();
    let mut d = doc(vec![layer("b", vec![keyframe(0, 1, vec![Element::Bitmap(BitmapInstance::new("pic", Affine::scale(20.0)))])])]);
    let mut item = BitmapItem::new("pic", "id1");
    item.media = Some(id);
    item.size = Some([2.0, 2.0]);
    d.library.push(LibraryItem::Bitmap(item)).unwrap();
    (d, media)
}

#[test]
fn bitmaps_decode_through_the_provider_and_are_cached() {
    let (d, media) = bitmap_doc();
    let mut ev = Evaluator::new(&d).with_media(&media).with_provider(PngDecoder);
    let list = ev.frame(0, 0);
    let mut bitmaps = 0;
    list.visit(&mut |n| bitmaps += usize::from(matches!(n, Node::Bitmap(_))));
    assert_eq!(bitmaps, 1);
    assert_eq!(ev.images().len(), 1);
    let _ = ev.frame(0, 0);
    assert_eq!(ev.images().len(), 1, "decoded once");
    let img = draw(&list, ev.images());
    assert!(is(&img, 10, 10, BLUE));
    assert!(is(&img, 60, 60, WHITE));
}

#[test]
fn provider_is_asked_once_per_bitmap() {
    struct Counting(u32);
    impl ImageProvider for Counting {
        fn decode(&mut self, _name: &str, _bytes: &[u8]) -> Option<Image> {
            self.0 += 1;
            assert_eq!(self.0, 1, "decode called again");
            Image::new(1, 1, vec![0, 255, 0, 255]).ok()
        }
    }
    let (d, media) = bitmap_doc();
    let mut ev = Evaluator::new(&d).with_media(&media).with_provider(Counting(0));
    for f in 0..3 {
        let _ = ev.frame(0, f);
    }
    assert!(is(&draw(&ev.frame(0, 0), ev.images()), 5, 5, GREEN));
}

#[test]
fn preloaded_images_need_no_media() {
    let (d, _) = bitmap_doc();
    let mut ev = Evaluator::new(&d);
    let id = ev.add_image("pic", Image::new(1, 1, vec![255, 0, 0, 255]).unwrap());
    let list = ev.frame(0, 0);
    let mut seen = None;
    list.visit(&mut |n| {
        if let Node::Bitmap(b) = n {
            seen = Some(b.image);
        }
    });
    assert_eq!(seen, Some(id));
    // Replacing the image drops the old one.
    ev.add_image("pic", Image::new(1, 1, vec![0, 255, 0, 255]).unwrap());
    assert_eq!(ev.images().len(), 1);
    ev.clear_caches();
    assert!(ev.images().is_empty());
}

#[test]
fn missing_pixels_become_a_placeholder() {
    let (d, _) = bitmap_doc();
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    let mut fills = 0;
    list.visit(&mut |n| fills += usize::from(matches!(n, Node::Fill(_))));
    assert_eq!(fills, 1, "grey rectangle of the bitmap's size");
    // A bitmap that is not in the library at all draws nothing.
    let d2 = doc(vec![layer("b", vec![keyframe(0, 1, vec![Element::Bitmap(BitmapInstance::new("nope", Affine::IDENTITY))])])]);
    let mut ev2 = Evaluator::new(&d2);
    assert!(ev2.frame(0, 0).nodes.is_empty());
    // Undecodable bytes are also a placeholder.
    let mut media = MediaStore::new();
    let id = media.insert(b"not a png".to_vec()).unwrap();
    let mut d3 = d.clone();
    if let Some(LibraryItem::Bitmap(b)) = d3.library.find_mut("pic") {
        b.media = Some(id);
    }
    let mut ev3 = Evaluator::new(&d3).with_media(&media).with_provider(PngDecoder);
    assert_eq!(ev3.frame(0, 0).nodes.len(), 1);
}

#[test]
fn bitmap_fills_in_shapes_resolve_to_images() {
    let (mut d, media) = bitmap_doc();
    let fill = FillStyle::Bitmap(BitmapFill { path: "pic".to_owned(), matrix: Affine::IDENTITY, clipped: false, smoothing: true });
    let shape = EdgeShape::from_paths(&[PathInput::new(rect_path(0.0, 0.0, 40.0, 40.0), Some(fill), None)]);
    d.scenes[0].layers = vec![layer("s", vec![keyframe(0, 1, vec![Element::Shape(anim_scene::ShapeElement::new(shape))])])];
    let mut ev = Evaluator::new(&d).with_media(&media).with_provider(PngDecoder);
    let list = ev.frame(0, 0);
    let mut bitmap_paint = false;
    list.visit(&mut |n| bitmap_paint |= matches!(n, Node::Fill(f) if matches!(f.paint, Paint::Bitmap { .. })));
    assert!(bitmap_paint);
    // Without the pixels the fill is a plain grey.
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    let mut solid = false;
    list.visit(&mut |n| solid |= matches!(n, Node::Fill(f) if matches!(f.paint, Paint::Solid(_))));
    assert!(solid);
}

fn text_run(characters: &str, face: &str, align: TextAlign, direction: TextDirection) -> TextRun {
    let mut run = TextRun::new(characters, face, 20.0);
    run.alignment = align;
    run.direction = direction;
    run
}

fn text_element(runs: Vec<TextRun>, width: f64) -> TextElement {
    TextElement::new(TextKind::Static, Affine::IDENTITY, width, 60.0, runs)
}

fn bounds_of(nodes: &[Node]) -> Option<anim_core::Rect> {
    let mut out: Option<anim_core::Rect> = None;
    let mut list = anim_display::DisplayList::default();
    list.nodes = nodes.to_vec();
    list.visit(&mut |n| {
        if let Node::Fill(f) = n {
            let b = f.path.bounding_box();
            out = Some(out.map_or(b, |o| o.union(b)));
        }
    });
    out
}

#[test]
fn text_becomes_glyph_fills() {
    let d = doc(vec![layer(
        "t",
        vec![keyframe(0, 1, vec![Element::Text(Box::new(text_element(vec![text_run("Hello", "Arial", TextAlign::Left, TextDirection::Auto)], 200.0)))])],
    )]);
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    if list.nodes.is_empty() {
        eprintln!("no usable fonts on this machine; skipping");
        return;
    }
    let b = bounds_of(&list.nodes).unwrap();
    assert!(b.width() > 10.0 && b.x0 >= 0.0 && b.x1 < 200.0, "{b:?}");
}

#[test]
fn hebrew_text_is_right_aligned_and_right_to_left() {
    let make = |align| {
        let run = text_run("שלום עולם", "Arial", align, TextDirection::RightToLeft);
        doc(vec![layer("t", vec![keyframe(0, 1, vec![Element::Text(Box::new(text_element(vec![run], 200.0)))])])])
    };
    let right = make(TextAlign::Right);
    let left = make(TextAlign::Left);
    let mut ev = Evaluator::new(&right);
    let r = ev.frame(0, 0);
    if r.nodes.is_empty() {
        eprintln!("no Hebrew font on this machine; skipping");
        return;
    }
    let mut ev2 = Evaluator::new(&left);
    let l = ev2.frame(0, 0);
    let (br, bl) = (bounds_of(&r.nodes).unwrap(), bounds_of(&l.nodes).unwrap());
    assert!(br.x1 > 150.0, "right-aligned Hebrew ends near the right edge: {br:?}");
    assert!(bl.x1 < 150.0, "left-aligned Hebrew stays on the left: {bl:?}");
    assert!((br.width() - bl.width()).abs() < 1.0, "same glyphs either way");
}

#[test]
fn arabic_joins_and_mixed_direction_lay_out() {
    let run = text_run("مرحبا abc שלום", "Arial", TextAlign::Left, TextDirection::Auto);
    let d = doc(vec![layer("t", vec![keyframe(0, 1, vec![Element::Text(Box::new(text_element(vec![run], 300.0)))])])]);
    let mut ev = Evaluator::new(&d);
    let list = ev.frame(0, 0);
    // Whatever fonts exist, evaluation must finish and stay inside sane bounds.
    if let Some(b) = bounds_of(&list.nodes) {
        assert!(b.x0 > -50.0 && b.x1 < 400.0, "{b:?}");
    }
}

#[test]
fn text_runs_split_into_paragraphs_with_per_paragraph_style() {
    let mut engine = TextEngine::without_system_fonts();
    let element = text_element(
        vec![
            text_run("one\rtwo\r\nthree", "Arial", TextAlign::Center, TextDirection::LeftToRight),
            text_run(" more\n", "Arial", TextAlign::Right, TextDirection::RightToLeft),
            text_run("last", "Arial", TextAlign::Right, TextDirection::RightToLeft),
        ],
        100.0,
    );
    let rich = rich_text(&mut engine, &element);
    let texts: Vec<_> = rich.paragraphs.iter().map(anim_text::Paragraph::text).collect();
    assert_eq!(texts, vec!["one", "two", "three more", "last"]);
    assert_eq!(rich.paragraphs[0].alignment, anim_text::Alignment::Center);
    assert_eq!(rich.paragraphs[3].alignment, anim_text::Alignment::Right);
    assert_eq!(rich.paragraphs[3].direction, anim_text::Direction::Rtl);
    assert_eq!(rich.paragraphs[2].alignment, anim_text::Alignment::Center, "a paragraph takes the style of its first run");
}

#[test]
fn run_styles_map_to_anim_text() {
    let mut engine = TextEngine::without_system_fonts();
    let mut run = TextRun::new("x", "Arial-BoldMT", 18.0);
    run.fill_color = Rgba8::opaque(10, 20, 30);
    run.alpha = 0.5;
    run.letter_spacing = 2.0;
    let rich = rich_text(&mut engine, &text_element(vec![run], 50.0));
    let r = &rich.paragraphs[0].runs[0];
    assert_eq!(r.size, 18.0);
    assert!(r.bold, "the face name asks for bold");
    assert_eq!((r.color.r, r.color.g, r.color.b), (10, 20, 30));
    assert!((i32::from(r.color.a) - 128).abs() <= 1);
    assert_eq!(r.letter_spacing, 2.0);
}

#[test]
fn empty_text_draws_nothing() {
    let d = doc(vec![layer("t", vec![keyframe(0, 1, vec![Element::Text(Box::new(text_element(vec![], 100.0)))])])]);
    let mut ev = Evaluator::new(&d);
    assert!(ev.frame(0, 0).nodes.is_empty());
}

fn hostile_doc(spec: &[(u32, u32, u8)], frame_probe: u32) -> (Document, u32) {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "s", anim_scene::SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 2, vec![rect(0.0, 0.0, 5.0, 5.0, RED)])])]);
    let frames = spec
        .iter()
        .map(|&(index, duration, kind)| {
            let mut f = keyframe(index, duration, vec![instance("s", Affine::translate((f64::from(index), 0.0))), rect(0.0, 0.0, 3.0, 3.0, BLUE)]);
            f.tween_type = match kind % 4 {
                0 => TweenType::None,
                1 => TweenType::Classic,
                2 => TweenType::Shape,
                _ => TweenType::MotionObject,
            };
            f.acceleration = i32::from(kind) - 128;
            f
        })
        .collect();
    d.scenes[0].layers = vec![layer("x", frames)];
    (d, frame_probe)
}

proptest! {
    #[test]
    fn hostile_keyframes_never_panic(
        spec in proptest::collection::vec((0u32..40, 0u32..40, any::<u8>()), 0..8),
        raw_probe in 0u32..70,
    ) {
        // Unsorted, overlapping and zero-length spans are all fair game for a hostile file; so are frames near the
        // top of the u32 range.
        let probe = if raw_probe >= 60 { u32::MAX - (raw_probe - 60) } else { raw_probe };
        let (d, probe) = hostile_doc(&spec, probe);
        let mut ev = Evaluator::new(&d);
        let list = ev.frame(0, probe);
        prop_assert!(list.nodes.len() < 100);
        let _ = ev.onion(0, probe, 2, 2);
        let _ = ev.frame_of_symbol("s", probe);
    }
}
