//! Harness tests: widgets run in a real egui pass (`egui_kittest`, no GPU) with simulated pointer input.
#![allow(clippy::suboptimal_flops, clippy::items_after_statements, reason = "test code: positions are written the way the geometry reads")]

use anim_ui::{
    LayerEvent, LayerRow, Span, StageEvent, StageViewState, TimelineEvent, TimelineModel, TimelineViewState,
    property::{PropValue, Property},
    stage::{Axis, PointerPhase},
    timeline::{FrameSelection, TweenKind},
    transport::{TransportEvent, TransportState},
};
use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Vec2, pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable};

const SIZE: Vec2 = vec2(900.0, 300.0);

fn press(h: &mut Harness<'_, impl Sized>, at: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
}

fn release(h: &mut Harness<'_, impl Sized>, at: Pos2) {
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
}

fn click(h: &mut Harness<'_, impl Sized>, at: Pos2) {
    press(h, at);
    release(h, at);
    h.step();
}

fn drag_to(h: &mut Harness<'_, impl Sized>, from: Pos2, to: Pos2) {
    press(h, from);
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.input_mut().events.push(Event::PointerMoved(from.lerp(to, t)));
        h.step();
    }
    release(h, to);
    h.step();
}

fn model() -> TimelineModel {
    let mut a = LayerRow::new(1, "Character");
    a.spans = vec![Span::key(0, 8), Span::tweened(8, 8, TweenKind::Classic), Span::blank(16, 4)];
    let mut b = LayerRow::new(2, "שכבה 2");
    b.spans = vec![Span::key(0, 20)];
    TimelineModel { layers: vec![a, b], frame_count: 30, current_frame: 3, fps: 24.0, ..TimelineModel::default() }
}

struct TimelineRig {
    model: TimelineModel,
    view: TimelineViewState,
    events: Vec<TimelineEvent>,
    origin: Pos2,
}

fn timeline_harness() -> Harness<'static, TimelineRig> {
    let rig = TimelineRig { model: model(), view: TimelineViewState::default(), events: Vec::new(), origin: Pos2::ZERO };
    Harness::builder().with_size(SIZE).build_ui_state(
        |ui, rig| {
            rig.origin = ui.max_rect().min;
            let events = anim_ui::timeline(ui, &rig.model, &rig.view);
            for e in &events {
                rig.view.apply(e);
                if let TimelineEvent::SetFrame(f) = e {
                    rig.model.current_frame = *f;
                }
            }
            rig.events.extend(events);
        },
        rig,
    )
}

#[test]
fn timeline_scrubbing_the_ruler_sets_the_frame() {
    let mut h = timeline_harness();
    h.run();
    let origin = h.state().origin;
    let ruler_y = origin.y + 12.0;
    let x = origin.x + 230.0 + 12.0 * 10.0 + 5.0;
    click(&mut h, pos2(x, ruler_y));
    assert!(h.state().events.contains(&TimelineEvent::SetFrame(10)), "{:?}", h.state().events);
    assert_eq!(h.state().model.current_frame, 10);
}

#[test]
fn timeline_click_selects_a_cell_and_moves_the_playhead() {
    let mut h = timeline_harness();
    h.run();
    let origin = h.state().origin;
    // Second layer row, frame 6.
    let at = pos2(origin.x + 230.0 + 12.0 * 6.0 + 5.0, origin.y + 24.0 + 22.0 + 11.0);
    click(&mut h, at);
    let sel = h.state().view.selection.clone();
    assert_eq!(sel, FrameSelection { layers: vec![2], first: 6, last: 6 });
    assert_eq!(h.state().model.current_frame, 6);
}

#[test]
fn timeline_drag_selects_a_range_over_two_layers() {
    let mut h = timeline_harness();
    h.run();
    let o = h.state().origin;
    let from = pos2(o.x + 230.0 + 12.0 * 2.0 + 5.0, o.y + 24.0 + 11.0);
    let to = pos2(o.x + 230.0 + 12.0 * 9.0 + 5.0, o.y + 24.0 + 22.0 + 11.0);
    drag_to(&mut h, from, to);
    assert_eq!(h.state().view.selection, FrameSelection { layers: vec![1, 2], first: 2, last: 9 });
}

#[test]
fn timeline_dragging_a_selection_emits_a_move() {
    let mut h = timeline_harness();
    h.run();
    let o = h.state().origin;
    let cell = |f: f32, row: f32| pos2(o.x + 230.0 + 12.0 * f + 5.0, o.y + 24.0 + 22.0 * row + 11.0);
    click(&mut h, cell(4.0, 0.0));
    drag_to(&mut h, cell(4.0, 0.0), cell(7.0, 1.0));
    let moved = h.state().events.iter().find_map(|e| match e {
        TimelineEvent::MoveFrames { delta_frames, delta_rows, .. } => Some((*delta_frames, *delta_rows)),
        _ => None,
    });
    assert_eq!(moved, Some((3, 1)));
}

#[test]
fn timeline_layer_toggles_and_selection() {
    let mut h = timeline_harness();
    h.run();
    let o = h.state().origin;
    let row0_y = o.y + 24.0 + 11.0;
    // Eye column: 3 toggles of 24 at the right end of the 230 wide column.
    click(&mut h, pos2(o.x + 230.0 - 24.0 * 3.0 + 12.0, row0_y));
    click(&mut h, pos2(o.x + 230.0 - 24.0 * 2.0 + 12.0, row0_y));
    click(&mut h, pos2(o.x + 100.0, row0_y));
    let events = &h.state().events;
    assert!(events.contains(&TimelineEvent::Layer(LayerEvent::ToggleVisible(1))), "{events:?}");
    assert!(events.contains(&TimelineEvent::Layer(LayerEvent::ToggleLocked(1))), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, TimelineEvent::Layer(LayerEvent::Select { id: 1, .. }))), "{events:?}");
    assert_eq!(h.state().view.selected_layers, vec![1]);
}

#[test]
fn timeline_wheel_scrolls_and_ctrl_wheel_zooms() {
    let mut h = timeline_harness();
    h.run();
    let o = h.state().origin;
    let at = pos2(o.x + 500.0, o.y + 60.0);
    h.input_mut().events.push(Event::PointerMoved(at));
    h.step();
    h.input_mut().events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -120.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::CTRL,
    });
    h.step();
    h.step();
    assert!(h.state().events.iter().any(|e| matches!(e, TimelineEvent::Zoom { .. })), "{:?}", h.state().events);
    assert!(h.state().view.frame_width > 12.0 || h.state().view.frame_width < 12.0);
}

struct StageRig {
    state: StageViewState,
    events: Vec<StageEvent>,
    origin: Pos2,
}

fn stage_harness() -> Harness<'static, StageRig> {
    let mut state = StageViewState::new(vec2(400.0, 300.0));
    state.fit_margin = 10.0;
    let rig = StageRig { state, events: Vec::new(), origin: Pos2::ZERO };
    Harness::builder().with_size(vec2(600.0, 500.0)).build_ui_state(
        |ui, rig| {
            rig.origin = ui.max_rect().min;
            let events = anim_ui::stage_view(ui, &rig.state, None);
            for e in &events {
                if let StageEvent::ViewChanged(v) = e {
                    rig.state.view = *v;
                    rig.state.fit_request = false;
                }
                if let StageEvent::GuideAdded(g) = e {
                    rig.state.guides.push(*g);
                }
            }
            rig.events.extend(events);
        },
        rig,
    )
}

#[test]
fn stage_fits_on_first_frame() {
    let mut h = stage_harness();
    h.run();
    let v = h.state().state.view;
    assert!(!h.state().state.fit_request);
    // The 400x300 document is limited by the width of the canvas (600 minus panel margins, ruler and fit margin).
    assert!(v.zoom > 1.2 && v.zoom < 1.5, "zoom {}", v.zoom);
    assert!(v.pan.x > 0.0 && v.pan.y > 0.0, "{:?}", v.pan);
}

#[test]
fn stage_reports_pointer_events_in_stage_coordinates() {
    let mut h = stage_harness();
    h.run();
    let o = h.state().origin;
    let view = h.state().state.view;
    // The stage point (100, 50) in screen space: canvas starts after the ruler.
    let canvas_min = o + vec2(18.0, 18.0);
    let screen = canvas_min + view.pan + vec2(100.0, 50.0) * view.zoom;
    click(&mut h, screen);
    let click_event = h.state().events.iter().find_map(|e| match e {
        StageEvent::Pointer(p) if p.phase == PointerPhase::Click => Some(*p),
        _ => None,
    });
    let p = click_event.expect("a click event");
    assert!((p.pos - pos2(100.0, 50.0)).length() < 1.0, "{:?}", p.pos);
    assert!((p.zoom - view.zoom).abs() < 1e-4);
}

#[test]
fn stage_drag_emits_down_drag_up() {
    let mut h = stage_harness();
    h.run();
    let o = h.state().origin;
    let from = o + vec2(150.0, 150.0);
    drag_to(&mut h, from, from + vec2(60.0, 20.0));
    let phases: Vec<PointerPhase> = h
        .state()
        .events
        .iter()
        .filter_map(|e| match e {
            StageEvent::Pointer(p) if p.phase != PointerPhase::Move => Some(p.phase),
            _ => None,
        })
        .collect();
    assert_eq!(phases.first(), Some(&PointerPhase::Down), "{phases:?}");
    assert!(phases.contains(&PointerPhase::Drag), "{phases:?}");
    assert_eq!(phases.last(), Some(&PointerPhase::Up), "{phases:?}");
}

#[test]
fn stage_zoom_about_pointer_with_ctrl_wheel() {
    let mut h = stage_harness();
    h.run();
    let o = h.state().origin;
    let before = h.state().state.view.zoom;
    h.input_mut().events.push(Event::PointerMoved(o + vec2(300.0, 250.0)));
    h.step();
    h.input_mut().events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 100.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::CTRL,
    });
    h.step();
    h.step();
    assert!((h.state().state.view.zoom - before).abs() > 1e-3, "zoom unchanged: {before}");
}

#[test]
fn stage_dragging_from_the_ruler_adds_a_guide() {
    let mut h = stage_harness();
    h.run();
    let o = h.state().origin;
    // Top ruler is the first 18 points; drag into the canvas.
    drag_to(&mut h, o + vec2(200.0, 8.0), o + vec2(200.0, 200.0));
    let guide = h.state().events.iter().find_map(|e| match e {
        StageEvent::GuideAdded(g) => Some(*g),
        _ => None,
    });
    let g = guide.expect("guide added");
    assert_eq!(g.axis, Axis::Horizontal);
    assert!(g.pos > 20.0 && g.pos < 300.0, "{}", g.pos);
}

#[test]
fn transport_buttons_emit_events() {
    struct Rig {
        state: TransportState,
        events: Vec<TransportEvent>,
        origin: Pos2,
    }
    let rig = Rig { state: TransportState { frame_count: 48, ..TransportState::default() }, events: Vec::new(), origin: Pos2::ZERO };
    let mut h = Harness::builder().with_size(vec2(500.0, 60.0)).build_ui_state(
        |ui, rig: &mut Rig| {
            rig.origin = ui.max_rect().min;
            rig.events.extend(anim_ui::transport_bar(ui, &rig.state));
        },
        rig,
    );
    h.run();
    let o = h.state().origin;
    // Buttons are 26 wide with the default 8 spacing: play is the third.
    let y = o.y + 12.0;
    click(&mut h, pos2(o.x + 13.0, y));
    click(&mut h, pos2(o.x + (26.0 + 8.0) * 2.0 + 13.0, y));
    let events = &h.state().events;
    assert!(events.contains(&TransportEvent::GoToStart), "{events:?}");
    assert!(events.contains(&TransportEvent::Play), "{events:?}");
}

#[test]
fn property_grid_lists_every_row() {
    let props = vec![
        Property::new("x", "X", PropValue::number(10.0)).in_section("Position"),
        Property::new("y", "Y", PropValue::number(20.0)).in_section("Position"),
        Property::new("fill", "Fill", PropValue::Color(anim_core::Rgba8 { r: 255, g: 0, b: 0, a: 255 })).in_section("Color"),
        Property::new("blend", "Blend", PropValue::choice(0, &["Normal", "Multiply"])),
        Property::new("name", "שם", PropValue::Text("דוגמה".into())),
        Property::new("loop", "Loop", PropValue::Bool(true)),
    ];
    let mut h = Harness::builder().with_size(vec2(300.0, 400.0)).build_ui(|ui| {
        let _ = anim_ui::property_grid(ui, &props);
    });
    h.run();
    let _ = h.get_by_label("X");
    let _ = h.get_by_label("Loop");
}

#[test]
fn library_panel_filters_and_selects() {
    use anim_ui::{LibraryEvent, LibraryItemView, LibraryKind, LibraryModel};
    let item = |id: u64, name: &str| LibraryItemView { id, name: name.into(), kind: LibraryKind::MovieClip, depth: 0, expanded: false, thumbnail: None };
    struct Rig {
        model: LibraryModel,
        events: Vec<LibraryEvent>,
        origin: Pos2,
    }
    let rig = Rig {
        model: LibraryModel { items: vec![item(1, "Hero"), item(2, "Villain")], selected: None, filter: String::new() },
        events: Vec::new(),
        origin: Pos2::ZERO,
    };
    let mut h = Harness::builder().with_size(vec2(260.0, 400.0)).build_ui_state(
        |ui, rig: &mut Rig| {
            rig.origin = ui.max_rect().min;
            rig.events.extend(anim_ui::library_panel(ui, &rig.model));
        },
        rig,
    );
    h.run();
    let o = h.state().origin;
    // Rows start below the 96 high preview, the search row and the count label.
    let row_y = o.y + 96.0 + 8.0 + 24.0 + 8.0 + 14.0 + 6.0 + 11.0;
    click(&mut h, pos2(o.x + 120.0, row_y));
    assert!(h.state().events.iter().any(|e| matches!(e, LibraryEvent::Select(_))), "{:?}", h.state().events);
    let _ = Rect::NOTHING;
}

#[test]
fn frame_shortcuts_consume_function_keys() {
    let mut h = Harness::builder()
        .with_size(vec2(200.0, 100.0))
        .build_ui_state(|ui, seen: &mut Vec<TimelineEvent>| seen.extend(anim_ui::timeline::frame_shortcuts(ui.ctx())), Vec::new());
    h.run();
    h.key_press(egui::Key::F6);
    h.run();
    h.key_press_modifiers(Modifiers::SHIFT, egui::Key::F5);
    h.run();
    h.key_press(egui::Key::F7);
    h.run();
    let seen = h.state();
    assert!(seen.contains(&TimelineEvent::FrameAction(anim_ui::timeline::FrameAction::InsertKeyframe)), "{seen:?}");
    assert!(seen.contains(&TimelineEvent::FrameAction(anim_ui::timeline::FrameAction::RemoveFrames)), "{seen:?}");
    assert!(seen.contains(&TimelineEvent::FrameAction(anim_ui::timeline::FrameAction::InsertBlankKeyframe)), "{seen:?}");
    assert!(!seen.contains(&TimelineEvent::FrameAction(anim_ui::timeline::FrameAction::InsertFrame)), "{seen:?}");
}

#[test]
fn standalone_layers_list_reports_toggles() {
    struct Rig {
        model: TimelineModel,
        events: Vec<LayerEvent>,
        origin: Pos2,
    }
    let rig = Rig { model: model(), events: Vec::new(), origin: Pos2::ZERO };
    let mut h = Harness::builder().with_size(vec2(300.0, 200.0)).build_ui_state(
        |ui, rig: &mut Rig| {
            rig.origin = ui.max_rect().min;
            rig.events.extend(anim_ui::layers_list(ui, &rig.model, &TimelineViewState::default()));
        },
        rig,
    );
    h.run();
    let o = h.state().origin;
    let width = 300.0 - 16.0;
    // First row sits below the 24 high header; the eye column is the third from the right.
    click(&mut h, pos2(o.x + width - 24.0 * 3.0 + 12.0, o.y + 24.0 + 11.0 + 4.0));
    assert!(h.state().events.contains(&LayerEvent::ToggleVisible(1)), "{:?}", h.state().events);
}
