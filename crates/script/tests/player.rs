//! Predictions first. The document: one layer of 20 frames on the main timeline, a button symbol `btn` placed at frame 1
//! with instance name `go`, and the labels `start` (frame 1) and `end` (frame 15).
//!
//! Predicted:
//! - `stop();` on frame 10 halts the playhead on frame 10, however many ticks follow.
//! - Without a script the playhead loops 20 -> 1.
//! - A click on `go` jumps to the label `end` and stops there (AS3 listener and AS2 `onRelease` forms).
//! - `gotoAndPlay` in a frame script runs the scripts of the frame it lands on.

// Test code: unwrap, indexing and arithmetic are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use anim_core::Affine;
use anim_scene::element::{Element, SymbolInstance, SymbolKind};
use anim_scene::library::LibraryItem;
use anim_scene::timeline::{Frame, Layer};
use anim_scene::{Document, Symbol};
use anim_script::Player;

fn doc(scripts: &[(u32, &str)], publish: Option<&str>) -> Document {
    let mut d = Document::new(200.0, 100.0, 24.0);
    d.publish_settings_xml = publish.map(str::to_owned);
    let Some(scene) = d.scenes.first_mut() else { return d };
    scene.layers.clear();
    let mut actions = Layer::new("actions", anim_core::Rgba8::BLACK);
    let mut keys: Vec<(u32, Option<&str>)> = scripts.iter().map(|(f, s)| (*f, Some(*s))).collect();
    if !keys.iter().any(|k| k.0 == 0) {
        keys.push((0, None));
    }
    keys.sort_by_key(|k| k.0);
    for (i, (index, src)) in keys.iter().enumerate() {
        let end = keys.get(i + 1).map_or(20, |n| n.0);
        let mut f = Frame::blank(*index, end - index);
        f.actionscript = src.map(str::to_owned);
        if *index == 0 {
            f.label = Some("start".into());
        }
        actions.frames.push(f);
    }
    if let Some(f) = actions.frames.iter_mut().find(|f| f.index == 14) {
        f.label = Some("end".into());
    } else {
        // A label-only keyframe at frame 15.
        let mut labelled = Layer::new("labels", anim_core::Rgba8::BLACK);
        labelled.frames.push(Frame::blank(0, 14));
        let mut end = Frame::blank(14, 6);
        end.label = Some("end".into());
        labelled.frames.push(end);
        scene.layers.push(labelled);
    }
    let mut art = Layer::new("art", anim_core::Rgba8::BLACK);
    let mut inst = SymbolInstance::new("btn", Affine::IDENTITY);
    inst.name = Some("go".into());
    inst.symbol_type = Some(SymbolKind::Button);
    art.frames.push(Frame::new(0, 20, vec![Element::SymbolInstance(Box::new(inst))]));
    scene.layers.push(art);
    scene.layers.push(actions);
    let sym = Symbol::new("btn", SymbolKind::Button, "id-1");
    d.library.push(LibraryItem::Symbol(Box::new(sym))).unwrap();
    d
}

fn run(player: &mut Player, d: &anim_scene::Document, ticks: u32) {
    for _ in 0..ticks {
        player.advance(d);
    }
}

#[test]
fn stop_on_frame_ten_halts_playback() {
    let d = doc(&[(9, "stop();")], None);
    let mut p = Player::new(&d);
    p.start(&d);
    assert_eq!(p.position(), (0, 0));
    run(&mut p, &d, 30);
    assert_eq!(p.position(), (0, 9), "frame 10, 1-based");
    assert!(!p.is_playing());
}

#[test]
fn without_scripts_the_timeline_loops() {
    let d = doc(&[], None);
    let mut p = Player::new(&d);
    p.start(&d);
    run(&mut p, &d, 19);
    assert_eq!(p.position().1, 19);
    run(&mut p, &d, 1);
    assert_eq!(p.position().1, 0);
}

#[test]
fn as3_click_jumps_to_the_label_and_stops() {
    let src = "stop();\nfunction onGo(e:MouseEvent):void {\n  gotoAndStop(\"end\");\n}\ngo.addEventListener(MouseEvent.CLICK, onGo);";
    let d = doc(&[(0, src)], None);
    let mut p = Player::new(&d);
    p.start(&d);
    run(&mut p, &d, 5);
    assert_eq!(p.position().1, 0, "stopped on frame 1");
    assert_eq!(p.listening(&d, "click"), vec!["go".to_owned()]);
    p.dispatch(&d, "go", "click");
    assert_eq!(p.position().1, 14, "label `end` is frame 15");
    assert!(!p.is_playing());
    // A click on something that is not there does nothing.
    p.dispatch(&d, "nobody", "click");
    assert_eq!(p.position().1, 14);
}

#[test]
fn as2_on_release_form() {
    let publish = "<PublishFlashProperties><ActionScriptVersion>2</ActionScriptVersion></PublishFlashProperties>";
    let d = doc(&[(0, "stop();\ngo.onRelease = function() { gotoAndPlay(\"end\"); };")], Some(publish));
    let mut p = Player::new(&d);
    assert_eq!(p.version(), anim_script::Version::As2);
    p.start(&d);
    p.dispatch(&d, "go", "click");
    assert_eq!(p.position().1, 14);
    assert!(p.is_playing());
    run(&mut p, &d, 2);
    assert_eq!(p.position().1, 16);
}

#[test]
fn goto_runs_the_target_frames_script() {
    let d = doc(&[(0, "gotoAndStop(5);"), (4, "trace(\"at five\", currentFrame);\nstop();")], None);
    let mut p = Player::new(&d);
    p.start(&d);
    assert_eq!(p.position().1, 4);
    assert_eq!(p.output(), ["at five 5"]);
}

#[test]
fn variables_conditions_and_enter_frame() {
    let src = "var n:int = 0;\nfunction tick(e:Event):void { n++; if (n >= 3) { stop(); } }\naddEventListener(Event.ENTER_FRAME, tick);";
    let d = doc(&[(0, src)], None);
    let mut p = Player::new(&d);
    p.start(&d);
    run(&mut p, &d, 10);
    assert!(!p.is_playing(), "stopped by the listener on the third tick");
    assert_eq!(p.position().1, 3);
}

#[test]
fn unsupported_code_is_reported_and_skipped() {
    let d = doc(&[(0, "for (var i:int = 0; i < 3; i++) { trace(i); }\nstop();")], None);
    let mut p = Player::new(&d);
    p.start(&d);
    run(&mut p, &d, 3);
    assert!(!p.is_playing(), "the statement after the loop still ran");
    assert_eq!(p.diagnostics().len(), 1);
}

#[test]
fn a_script_that_jumps_to_itself_cannot_hang() {
    let d = doc(&[(0, "gotoAndPlay(1);")], None);
    let mut p = Player::new(&d);
    p.start(&d);
    run(&mut p, &d, 3);
}

#[test]
fn runtime_errors_go_to_the_output() {
    let d = doc(&[(0, "nowhere.stop();\ngotoAndStop(\"missing\");")], None);
    let mut p = Player::new(&d);
    p.start(&d);
    assert_eq!(p.output().len(), 1);
    assert!(p.output().first().is_some_and(|l| l.starts_with("Error:")));
}
