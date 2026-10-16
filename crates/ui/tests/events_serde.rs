//! Every event and view model is plain serde data, so a scripting or MCP layer can build the same events the widgets
//! produce and feed them through the app's normal event path. These tests pin the JSON round trips.

#![allow(clippy::expect_used, clippy::panic, reason = "test helpers")]

use anim_ui::{
    Guide, LayerEvent, LibraryAction, LibraryEvent, Overlay, PropValue, PropertyEdit, StageEvent, StageView, TimelineEvent, TransportEvent,
    stage::{Axis, PointerPhase, StagePointer},
    timeline::{DropPlacement, FrameAction, FrameSelection},
    toolbar::ToolbarEvent,
};
use egui::{Color32, Modifiers, PointerButton, Rect, pos2, vec2};
use serde::{Serialize, de::DeserializeOwned};

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) {
    let json = serde_json::to_string(value).expect("serialises");
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, value, "{json}");
}

#[test]
fn timeline_events_round_trip() {
    for e in [
        TimelineEvent::SetFrame(12),
        TimelineEvent::SelectFrames(FrameSelection { layers: vec![3, 4], first: 2, last: 9 }),
        TimelineEvent::MoveFrames { selection: FrameSelection::single(1, 5), delta_frames: -2, delta_rows: 1 },
        TimelineEvent::FrameAction(FrameAction::CreateShapeTween),
        TimelineEvent::Layer(LayerEvent::Select { id: 7, modifiers: Modifiers::SHIFT }),
        TimelineEvent::Layer(LayerEvent::Move { id: 1, target: 2, placement: DropPlacement::Inside }),
        TimelineEvent::Scroll { x: 10.0, y: 0.0 },
        TimelineEvent::Zoom { frame_width: 24.0, scroll_x: 5.5 },
        TimelineEvent::SetOnionRange { before: 3, after: 1 },
        TimelineEvent::ToggleOnion,
    ] {
        round_trip(&e);
    }
}

#[test]
fn stage_events_round_trip() {
    let pointer = StagePointer {
        phase: PointerPhase::Drag,
        button: PointerButton::Primary,
        pos: pos2(10.5, 20.0),
        origin: pos2(0.0, 0.0),
        delta: vec2(1.0, -1.0),
        modifiers: Modifiers::ALT,
        zoom: 2.0,
    };
    for e in [
        StageEvent::Pointer(pointer),
        StageEvent::ViewChanged(StageView { zoom: 1.5, pan: vec2(4.0, 8.0) }),
        StageEvent::GuideAdded(Guide { axis: Axis::Horizontal, pos: 120.0 }),
        StageEvent::GuideMoved { index: 2, pos: 7.0 },
        StageEvent::GuideRemoved { index: 0 },
        StageEvent::DropLibraryItem { id: 9, pos: pos2(1.0, 2.0) },
    ] {
        round_trip(&e);
    }
    round_trip(&Overlay::Rect { rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(5.0, 5.0)), style: anim_ui::stage::OverlayStyle::line(Color32::RED) });
}

#[test]
fn other_events_round_trip() {
    for e in [TransportEvent::Play, TransportEvent::SetFps(30.0), TransportEvent::SetFrame(4), TransportEvent::SetLoop(false)] {
        round_trip(&e);
    }
    round_trip(&ToolbarEvent::Select("pen".into()));
    for e in [
        LibraryEvent::Select(1),
        LibraryEvent::FilterChanged("hero".into()),
        LibraryEvent::Action { id: 2, action: LibraryAction::Duplicate },
        LibraryEvent::PreviewPlay,
        LibraryEvent::PreviewStop,
        LibraryEvent::PreviewSeek(500),
    ] {
        round_trip(&e);
    }
    round_trip(&PropertyEdit { id: "x".into(), value: PropValue::number(3.0).precision(1, 0.1), commit: true });
    round_trip(&PropertyEdit { id: "c".into(), value: PropValue::Color(anim_core::Rgba8 { r: 1, g: 2, b: 3, a: 4 }), commit: false });
}

#[test]
fn events_have_a_readable_json_shape() {
    let json = serde_json::to_string(&TimelineEvent::FrameAction(FrameAction::InsertKeyframe)).expect("serialises");
    assert_eq!(json, r#"{"FrameAction":"InsertKeyframe"}"#);
    let json = serde_json::to_string(&TimelineEvent::SetFrame(3)).expect("serialises");
    assert_eq!(json, r#"{"SetFrame":3}"#);
}
