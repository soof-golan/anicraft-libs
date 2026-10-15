//! What tool layers (an MCP server, test harnesses, a debugger) rely on: JSON in and out, and threads.

mod common;

use anim_text::{Affinity, Alignment, Caret, Direction, Motion, RichText, Script, Selection, TextEngine, TextLayout};
use common::{HEBREW, engine, engine_for, para};

#[test]
fn rich_text_is_plain_json_and_lays_out_the_same_after_a_round_trip() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    let text = para(HEBREW, Alignment::Center, Direction::Rtl);
    let json = serde_json::to_string(&text).unwrap();
    assert!(json.contains("\"Center\"") && json.contains("\"Rtl\""), "{json}");
    let back: RichText = serde_json::from_str(&json).unwrap();
    assert_eq!(back, text);
    let (a, b) = (engine.layout(&text, Some(200.0)), engine.layout(&back, Some(200.0)));
    assert_eq!(a.lines, b.lines);
    assert_eq!(a.bounds, b.bounds);
}

#[test]
fn a_layout_serialises_without_the_font_files() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&common::one("Tool output"), None);
    let value = serde_json::to_value(&layout).unwrap();
    assert_eq!(value["text"], "Tool output");
    assert_eq!(value["lines"].as_array().map(Vec::len), Some(1));
    let run = &value["runs"][0];
    assert_eq!(run["glyphs"].as_array().map(Vec::len), Some(11));
    assert_eq!(run["font"]["id"], serde_json::json!(layout.runs[0].font.id));
    assert!(run["font"].get("data").is_none(), "the font bytes are not part of the output");
    assert!(value.get("paras").is_none());
    assert!(serde_json::to_string(&layout).unwrap().len() < 20_000);
}

#[test]
fn carets_selections_and_motions_round_trip_through_json() {
    let caret = Caret { offset: 7, affinity: Affinity::Upstream };
    let selection = Selection { anchor: Caret::new(2), focus: caret };
    for motion in [Motion::Left, Motion::WordRight, Motion::LineEnd, Motion::TextStart] {
        let json = serde_json::to_string(&motion).unwrap();
        assert_eq!(serde_json::from_str::<Motion>(&json).unwrap(), motion);
    }
    let json = serde_json::to_string(&selection).unwrap();
    assert_eq!(serde_json::from_str::<Selection>(&json).unwrap(), selection);
    assert_eq!(serde_json::from_str::<Caret>(r#"{"offset":3,"affinity":"Downstream"}"#).unwrap(), Caret::new(3));
    assert_eq!(serde_json::to_string(&Script::Hebrew).unwrap(), "\"Hebrew\"");
}

const fn assert_send<T: Send>() {}

#[test]
fn engines_and_layouts_can_move_to_an_actor_thread() {
    assert_send::<TextEngine>();
    assert_send::<TextLayout>();
    assert_send::<RichText>();
    let Some(mut engine) = engine() else { return };
    let text = common::one("Across threads");
    let handle = std::thread::spawn(move || {
        let layout = engine.layout(&text, Some(100.0));
        let nodes = engine.to_display(&layout);
        (layout, nodes.len())
    });
    let (layout, nodes) = handle.join().unwrap();
    assert!(nodes > 0);
    assert!(layout.caret_rect(layout.caret_at(0), 1.0).is_some());
}

#[test]
fn a_layout_is_a_snapshot_that_stays_valid_while_the_engine_moves_on() {
    let Some(mut engine) = engine() else { return };
    let first = engine.layout(&common::one("First"), None);
    let second = engine.layout(&common::one("Second text"), None);
    assert_eq!(first.text(), "First");
    assert!(first.caret_rect(first.caret_at(5), 1.0).is_some());
    assert!(second.bounds.width() > first.bounds.width());
    assert_eq!(first.to_display().len(), engine.to_display(&first).len());
}
