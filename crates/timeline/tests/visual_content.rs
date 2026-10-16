//! Visual verification of symbol playback and masks. Predictions: `tests/fixtures/visual/nested_symbols.expect.md`
//! and `masks.expect.md`, written before the first render.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod common;

use anim_core::{Affine, Rgba8};
use anim_display::Image;
use anim_scene::{Document, Element, Layer, LayerKind, LoopMode, SymbolInstance, SymbolKind, TweenType};
use anim_timeline::{EvalOptions, Evaluator};
use common::*;

const BLACK: Rgba8 = Rgba8::opaque(0, 0, 0);
const DOT_COLOURS: [Rgba8; 4] = [RED, GREEN, BLUE, BLACK];

fn dots_layer() -> Layer {
    layer("d", (0..4u32).map(|k| keyframe(k, 1, vec![rect(10.0 * f64::from(k), 0.0, 10.0, 10.0, DOT_COLOURS[k as usize])])).collect())
}

fn graphic(name: &str, matrix: Affine, mode: LoopMode, first: u32) -> Element {
    let mut i = SymbolInstance::new(name, matrix);
    i.loop_mode = Some(mode);
    i.first_frame = first;
    Element::SymbolInstance(Box::new(i))
}

fn clip(name: &str, matrix: Affine, first: u32) -> Element {
    let mut i = SymbolInstance::new(name, matrix);
    i.first_frame = first;
    Element::SymbolInstance(Box::new(i))
}

/// Which of the four dot cells in the row starting at `y` is drawn: `Some(k)` for exactly the cell k with its colour.
fn row_dot(img: &Image, y: u32) -> Option<usize> {
    let cells: Vec<Option<usize>> = (0..4u32)
        .map(|j| {
            let p = pixel(img, 10 * j + 5, y + 5);
            if p == [255, 255, 255, 255] { None } else { DOT_COLOURS.iter().position(|c| p == [c.r, c.g, c.b, 255]).or(Some(99)) }
        })
        .collect();
    let drawn: Vec<(usize, usize)> = cells.iter().enumerate().filter_map(|(j, c)| c.map(|c| (j, c))).collect();
    match drawn.as_slice() {
        [] => None,
        [(j, c)] => {
            assert_eq!(j, c, "cell {j} drawn in the colour of dot {c}");
            Some(*j)
        }
        many => panic!("several cells drawn in row {y}: {many:?}"),
    }
}

fn nested_doc() -> Document {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "dots", SymbolKind::Graphic, vec![dots_layer()]);
    for (name, inner_mode) in [("mid", LoopMode::Loop), ("mid_once", LoopMode::PlayOnce)] {
        add_symbol(
            &mut d,
            name,
            SymbolKind::Graphic,
            vec![layer("a", vec![keyframe(0, 2, vec![]), keyframe(2, 4, vec![graphic("dots", Affine::IDENTITY, inner_mode, 1)])])],
        );
    }
    let row = |name: &str, y: f64, mode: LoopMode, first: u32| graphic(name, Affine::translate((0.0, y)), mode, first);
    d.scenes[0].layers = vec![
        layer("r0", vec![keyframe(0, 14, vec![row("mid", 0.0, LoopMode::Loop, 4)])]),
        layer("r1", vec![keyframe(0, 14, vec![row("mid", 25.0, LoopMode::PlayOnce, 4)])]),
        layer("r2", vec![keyframe(0, 14, vec![row("mid", 50.0, LoopMode::SingleFrame, 3)])]),
        layer("r3", vec![keyframe(0, 14, vec![row("mid_once", 75.0, LoopMode::Loop, 4)])]),
    ];
    d
}

#[test]
fn nested_graphics_follow_the_predicted_frame_table() {
    let d = nested_doc();
    let mut ev = Evaluator::new(&d);
    let row0 = [Some(3), Some(0), None, None, Some(1), Some(2), Some(3), Some(0), None, None, Some(1), Some(2), Some(3), Some(0)];
    let row1 = [Some(3), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0), Some(0)];
    let row2 = [Some(2); 14];
    let row3 = [Some(3), Some(3), None, None, Some(1), Some(2), Some(3), Some(3), None, None, Some(1), Some(2), Some(3), Some(3)];
    let mut frames = Vec::new();
    for f in 0..14u32 {
        let img = draw(&ev.frame(0, f), ev.images());
        let i = f as usize;
        assert_eq!(row_dot(&img, 0), row0[i], "row 0 (loop) at f{f}");
        assert_eq!(row_dot(&img, 25), row1[i], "row 1 (play once) at f{f}");
        assert_eq!(row_dot(&img, 50), row2[i], "row 2 (single frame) at f{f}");
        assert_eq!(row_dot(&img, 75), row3[i], "row 3 (inner play once, outer loop) at f{f}");
        frames.push(zoom(&img, 0, 0, 40, 100, 1));
    }
    let sheet = sheet(&frames, 7, 2);
    save(&sheet, "nested_symbols");
    golden(&sheet, "nested_symbols");
}

fn clip_doc() -> Document {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "dots", SymbolKind::Graphic, vec![dots_layer()]);
    add_symbol(&mut d, "clip", SymbolKind::MovieClip, vec![dots_layer()]);
    for (name, inner_first) in [("clipwrap", 0), ("clipwrap1", 1)] {
        add_symbol(
            &mut d,
            name,
            SymbolKind::MovieClip,
            vec![layer("a", vec![keyframe(0, 4, vec![graphic("dots", Affine::IDENTITY, LoopMode::Loop, inner_first)])])],
        );
    }
    d.scenes[0].layers = vec![
        layer("c0", vec![keyframe(0, 10, vec![clip("clip", Affine::IDENTITY, 2)])]),
        layer("c1", vec![keyframe(0, 10, vec![clip("clipwrap", Affine::translate((0.0, 25.0)), 2)])]),
        layer("c2", vec![keyframe(0, 10, vec![clip("clipwrap1", Affine::translate((0.0, 50.0)), 2)])]),
    ];
    d
}

#[test]
fn movie_clips_authoring_and_advancing() {
    let d = clip_doc();
    let mut ev = Evaluator::new(&d);
    for f in 0..10u32 {
        let img = draw(&ev.frame(0, f), ev.images());
        assert_eq!((row_dot(&img, 0), row_dot(&img, 25), row_dot(&img, 50)), (Some(2), Some(2), Some(3)), "authoring f{f}");
    }
    ev.set_options(EvalOptions { advance_movie_clips: true, ..EvalOptions::default() });
    let mut frames = Vec::new();
    for f in 0..10u32 {
        let img = draw(&ev.frame(0, f), ev.images());
        let local = ((2 + f) % 4) as usize;
        assert_eq!(row_dot(&img, 0), Some(local), "clip advancing f{f}");
        assert_eq!(row_dot(&img, 25), Some(local), "clipwrap advancing f{f}");
        assert_eq!(row_dot(&img, 50), Some((1 + local) % 4), "clipwrap1 advancing f{f}");
        frames.push(zoom(&img, 0, 0, 40, 60, 1));
    }
    save(&sheet(&frames, 5, 2), "movie_clips_advancing");
}

// ---- masks ----

fn stripes() -> Vec<Element> {
    (0..10u32).map(|i| rect(10.0 * f64::from(i), 0.0, 10.0, 100.0, if i % 2 == 0 { RED } else { GREEN })).collect()
}

fn tweened(index: u32, dur: u32, from: Affine, name: &str) -> Vec<anim_scene::Frame> {
    let mut start = keyframe(index, dur, vec![instance(name, from)]);
    start.tween_type = TweenType::Classic;
    vec![start]
}

fn mask_doc() -> Document {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "m30", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 35.0, 30.0, 30.0, BLACK)])])]);
    add_symbol(&mut d, "barh", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 100.0, 20.0, BLUE)])])]);
    let mut mask_frames = tweened(0, 10, Affine::IDENTITY, "m30");
    mask_frames.push(keyframe(10, 1, vec![instance("m30", Affine::translate((70.0, 0.0)))]));
    let mut bar_frames = tweened(0, 10, Affine::IDENTITY, "barh");
    bar_frames.push(keyframe(10, 1, vec![instance("barh", Affine::translate((0.0, 80.0)))]));
    d.scenes[0].layers = vec![
        layer_kind("m", LayerKind::Mask, None, mask_frames),
        layer_kind("bar", LayerKind::Normal, Some(0), bar_frames),
        layer_kind("content", LayerKind::Normal, Some(0), vec![keyframe(0, 11, stripes())]),
    ];
    d
}

#[test]
fn mask_tween_clips_animated_content() {
    let d = mask_doc();
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for f in 0..=10u32 {
        frames.push(draw(&ev.frame(0, f), ev.images()));
    }
    let at = |f: usize, x: u32, y: u32, c: Rgba8| assert!(is(&frames[f], x, y, c), "f{f} ({x}, {y}) is {:?}, want {c:?}", pixel(&frames[f], x, y));
    // Frame 5: mask x 35..65, y 35..65; bar y 40..60.
    at(5, 45, 50, BLUE);
    at(5, 45, 38, RED);
    at(5, 37, 38, GREEN);
    at(5, 20, 50, WHITE);
    at(5, 45, 70, WHITE);
    at(5, 45, 30, WHITE);
    at(5, 35, 50, BLUE);
    at(5, 34, 50, WHITE);
    at(5, 64, 50, BLUE);
    at(5, 65, 50, WHITE);
    at(5, 35, 62, GREEN);
    at(5, 34, 62, WHITE);
    at(5, 50, 64, GREEN);
    at(5, 50, 65, WHITE);
    // Frame 0: mask x 0..30; bar at y 0..20 is outside the mask.
    at(0, 5, 50, RED);
    at(0, 15, 50, GREEN);
    at(0, 25, 50, RED);
    at(0, 35, 50, WHITE);
    at(0, 5, 10, WHITE);
    // Frame 10: mask x 70..100; bar y 80..100 is outside the mask.
    at(10, 75, 50, GREEN);
    at(10, 95, 50, GREEN);
    at(10, 85, 50, RED);
    at(10, 65, 50, WHITE);
    // Every frame: the mask square's own pixels are never painted in a mask colour.
    for (f, img) in frames.iter().enumerate() {
        let x0 = 7 * f as u32;
        assert!(pixel(img, x0 + 1, 50) != [0, 0, 0, 255], "f{f}: the mask is not drawn itself");
        assert!(x0 + 31 >= 100 || is(img, x0 + 31, 50, WHITE), "f{f}: right of the mask is empty");
        assert!(x0 == 0 || is(img, x0 - 1, 50, WHITE), "f{f}: left of the mask is empty");
    }
    save(&sheet(&frames, 4, 2), "mask_tween");
    golden(&sheet(&frames[..4], 4, 1), "mask_tween");
}

#[test]
fn blank_mask_keyframes_hide_the_masked_layers() {
    let mut d = doc(vec![]);
    d.scenes[0].layers = vec![
        layer_kind(
            "m",
            LayerKind::Mask,
            None,
            vec![keyframe(0, 3, vec![rect(20.0, 35.0, 40.0, 30.0, BLACK)]), keyframe(3, 3, vec![]), keyframe(6, 3, vec![rect(20.0, 35.0, 40.0, 30.0, BLACK)])],
        ),
        layer_kind("content", LayerKind::Normal, Some(0), vec![keyframe(0, 9, stripes())]),
    ];
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for f in 0..9u32 {
        let img = draw(&ev.frame(0, f), ev.images());
        if (3..6).contains(&f) {
            assert!(img.premultiplied_rgba.chunks_exact(4).all(|p| p == [255, 255, 255, 255]), "f{f}: a blank mask hides everything");
        } else {
            assert!(is(&img, 25, 50, RED) && is(&img, 35, 50, GREEN) && is(&img, 65, 50, WHITE) && is(&img, 25, 30, WHITE), "f{f}");
        }
        frames.push(img);
    }
    save(&sheet(&frames, 5, 1), "mask_blank");
}

#[test]
fn mask_inside_mask_intersects() {
    let mut d = doc(vec![]);
    add_symbol(&mut d, "msq40", SymbolKind::Graphic, vec![layer("l", vec![keyframe(0, 1, vec![rect(0.0, 0.0, 40.0, 40.0, BLACK)])])]);
    let mut inner_mask = tweened(0, 4, Affine::IDENTITY, "msq40");
    inner_mask.push(keyframe(4, 1, vec![instance("msq40", Affine::translate((40.0, 0.0)))]));
    add_symbol(
        &mut d,
        "inner",
        SymbolKind::Graphic,
        vec![
            layer_kind("im", LayerKind::Mask, None, inner_mask),
            layer_kind("ic", LayerKind::Normal, Some(0), vec![keyframe(0, 5, vec![rect(0.0, 0.0, 100.0, 100.0, GREEN)])]),
        ],
    );
    d.scenes[0].layers = vec![
        layer_kind("om", LayerKind::Mask, None, vec![keyframe(0, 10, vec![rect(20.0, 0.0, 60.0, 100.0, BLACK)])]),
        layer_kind("oc", LayerKind::Normal, Some(0), vec![keyframe(0, 10, vec![graphic("inner", Affine::IDENTITY, LoopMode::Loop, 0)])]),
    ];
    let mut ev = Evaluator::new(&d);
    let mut frames = Vec::new();
    for f in 0..10u32 {
        let img = draw(&ev.frame(0, f), ev.images());
        let l = f % 5;
        let (lo, hi) = ((10 * l).max(20), (10 * l + 40).min(80));
        for x in 0..100u32 {
            for y in [20u32, 45] {
                let want = if (lo..hi).contains(&x) && y < 40 { GREEN } else { WHITE };
                assert!(is(&img, x, y, want), "f{f} ({x}, {y}): {:?}, want {want:?} (visible x {lo}..{hi})", pixel(&img, x, y));
            }
        }
        frames.push(img);
    }
    save(&sheet(&frames, 5, 1), "mask_in_mask");
}
