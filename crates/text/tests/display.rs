//! Glyph outlines as display-list nodes.

mod common;

use anim_core::kurbo::Shape;
use anim_core::{Affine, Point, Rgba8};
use anim_display::{DisplayList, FillRule, Node, Paint};
use anim_text::{Paragraph, RichText, Run, Script};
use common::{ARABIC, HEBREW, engine, engine_for, one, run};

fn fills(nodes: &[Node]) -> Vec<&anim_display::Fill> {
    nodes.iter().filter_map(|n| if let Node::Fill(f) = n { Some(f) } else { None }).collect()
}

#[test]
fn latin_text_becomes_one_filled_path_per_run_inside_the_layout_bounds() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("Hello"), None);
    let nodes = engine.to_display(&layout);
    let fills = fills(&nodes);
    assert_eq!(fills.len(), 1);
    let fill = fills[0];
    assert_eq!(fill.rule, FillRule::NonZero);
    assert_eq!(fill.paint, Paint::Solid(Rgba8::BLACK));
    assert_eq!(fill.transform, Affine::IDENTITY);
    let bbox = fill.path.bounding_box();
    assert!(bbox.width() > 30.0, "{bbox:?}");
    assert!(bbox.x0 >= -1.0 && bbox.x1 <= layout.bounds.x1 + 1.0, "{bbox:?} vs {:?}", layout.bounds);
    assert!(bbox.y0 >= 0.0 && bbox.y1 <= layout.bounds.y1 + 0.5, "{bbox:?} vs {:?}", layout.bounds);
    // "Hello" has 4 closed shapes with holes or not: H, e, l, l, o -> at least 5 subpaths.
    let subpaths = fill.path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count();
    assert!(subpaths >= 5, "{subpaths}");
}

#[test]
fn glyphs_are_drawn_y_down_with_capitals_above_the_baseline() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("H"), None);
    let nodes = engine.to_display(&layout);
    let bbox = fills(&nodes)[0].path.bounding_box();
    let baseline = layout.lines[0].baseline;
    assert!(bbox.y1 <= baseline + 0.6 && bbox.y1 >= baseline - 0.6, "bottom of H on the baseline: {bbox:?} vs {baseline}");
    assert!(bbox.y0 < baseline - 10.0, "H is taller than half the 20 px em: {bbox:?}");
}

#[test]
fn colours_split_into_separate_fills_and_transparent_text_is_skipped() {
    let Some(mut engine) = engine() else { return };
    let red = Rgba8::opaque(255, 0, 0);
    let green = Rgba8::opaque(0, 255, 0);
    let text = RichText::new(vec![Paragraph::new(vec![
        Run { color: red, ..run("red") },
        Run { color: green, ..run("green") },
        Run { color: Rgba8::TRANSPARENT, ..run("ghost") },
    ])]);
    let layout = engine.layout(&text, None);
    let nodes = engine.to_display(&layout);
    let paints: Vec<_> = fills(&nodes).iter().map(|f| f.paint.clone()).collect();
    assert_eq!(paints, vec![Paint::Solid(red), Paint::Solid(green)]);
}

#[test]
fn underline_adds_a_thin_rectangle_under_the_run() {
    let Some(mut engine) = engine() else { return };
    let text = RichText::new(vec![Paragraph::new(vec![Run { underline: true, ..run("under") }])]);
    let layout = engine.layout(&text, None);
    let rect = layout.runs[0].underline.expect("run has an underline");
    assert!(rect.height() >= 0.5 && rect.height() < 4.0, "{rect:?}");
    assert!(rect.y0 > layout.lines[0].baseline - 1.0, "below the baseline: {rect:?}");
    assert!((rect.width() - layout.runs[0].advance).abs() < 0.01);
    let nodes = engine.to_display(&layout);
    assert_eq!(fills(&nodes).len(), 2, "glyphs and underline");
}

#[test]
fn hebrew_and_arabic_outlines_stay_inside_the_layout() {
    for (script, text) in [(Script::Hebrew, HEBREW), (Script::Arabic, ARABIC)] {
        let Some(mut engine) = engine_for(&[script]) else { continue };
        let layout = engine.layout(&one(text), Some(400.0));
        let nodes = engine.to_display(&layout);
        let all = fills(&nodes);
        assert!(!all.is_empty(), "{script:?}");
        for fill in all {
            let bbox = fill.path.bounding_box();
            assert!(bbox.width() > 20.0, "{script:?}: {bbox:?}");
            assert!(bbox.x0 >= -2.0 && bbox.x1 <= 402.0, "{script:?}: {bbox:?}");
        }
    }
}

#[test]
fn display_output_is_deterministic_and_the_cache_does_not_change_it() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("Determinism"), None);
    let first = engine.to_display(&layout);
    let second = engine.to_display(&layout);
    assert_eq!(first, second);
    assert_eq!(first, layout.to_display(), "uncached conversion agrees");
}

#[test]
fn display_nodes_fit_in_a_display_list_and_round_trip_through_serde() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&one("Serde"), None);
    let list = DisplayList { size: anim_core::Size::new(200.0, 50.0), background: None, nodes: engine.to_display(&layout) };
    let json = serde_json::to_string(&list).unwrap();
    assert_eq!(serde_json::from_str::<DisplayList>(&json).unwrap(), list);
}

#[test]
fn glyph_paths_move_with_the_text_position() {
    let Some(mut engine) = engine() else { return };
    let left = engine.layout(&one("Move"), None);
    let right = engine.layout(&common::para("Move", anim_text::Alignment::Right, anim_text::Direction::Ltr), Some(300.0));
    let a = fills(&engine.to_display(&left))[0].path.bounding_box();
    let b = fills(&engine.to_display(&right))[0].path.bounding_box();
    let shift = b.x0 - a.x0;
    assert!((shift - (300.0 - left.bounds.width())).abs() < 0.01, "{shift}");
    assert!(Point::new(b.x1, b.y1).x <= 300.0 + 1.0);
}

#[test]
fn tall_hostile_sizes_do_not_blow_up() {
    let Some(mut engine) = engine() else { return };
    for size in [0.0, -3.0, f64::NAN, f64::INFINITY, 1e12, 1e-12] {
        let text = RichText::new(vec![Paragraph::new(vec![Run { size, ..Run::new("Hello שלום") }])]);
        let layout = engine.layout(&text, Some(100.0));
        let nodes = engine.to_display(&layout);
        for n in &nodes {
            if let Node::Fill(f) = n {
                let bb = f.path.bounding_box();
                assert!(bb.x0.is_finite() && bb.y1.is_finite(), "size {size}: {bb:?}");
            }
        }
    }
}
