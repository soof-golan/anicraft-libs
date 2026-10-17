//! Renders an Animate-like window out of every widget: with `ANIM_UI_SCREENSHOT_DIR` set it writes PNGs there (software
//! Vulkan is enough); without it the test still runs the whole UI headless and checks it produces no dock error.
#![allow(
    clippy::suboptimal_flops,
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "demo code that paints a procedural picture"
)]

use anim_core::Rgba8;
use anim_ui::{
    Guide, LibraryItemView, LibraryKind, LibraryModel, Overlay, PanelId, PropValue, Property, Span, StageView, StageViewState, Theme, TimelineModel,
    TimelineViewState, ToolItem, TransportState,
    dock::{self, docking::Layout},
    stage::{Axis, MarkerShape, OverlayStyle, PathNode},
    timeline::{FrameSelection, LayerKind, LayerRow, TweenKind, Waveform},
    toolbar,
};
use egui::{Color32, Pos2, Rect, TextureHandle, pos2, vec2};
use egui_kittest::Harness;

struct Gallery {
    theme: Theme,
    layout: Layout<PanelId>,
    stage: StageViewState,
    texture: Option<TextureHandle>,
    model: TimelineModel,
    view: TimelineViewState,
    props: Vec<Property>,
    library: LibraryModel,
    transport: TransportState,
    tools: Vec<ToolItem>,
    tool: String,
    fonts: usize,
    dock_error: bool,
}

fn artwork() -> egui::ColorImage {
    let (w, h) = (640usize, 360usize);
    let mut px = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let t = y as f32 / h as f32;
            let mut c = [(70.0 + 120.0 * t) as u8, (130.0 + 90.0 * t) as u8, (220.0 - 60.0 * t) as u8, 255];
            let (dx, dy) = (x as f32 - 320.0, y as f32 - 190.0);
            if dx * dx + dy * dy < 80.0 * 80.0 {
                c = [250, 205, 80, 255];
            }
            if (60..260).contains(&x) && (230..330).contains(&y) {
                c = [60, 160, 90, 255];
            }
            if y > 320 {
                c = [70, 60, 55, 255];
            }
            px.extend_from_slice(&c);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &px)
}

fn timeline_model() -> TimelineModel {
    let color = |r, g, b| Rgba8 { r, g, b, a: 255 };
    let mut folder = LayerRow::new(10, "Characters");
    folder.kind = LayerKind::Folder;
    let mut hero = LayerRow::new(11, "Hero");
    hero.depth = 1;
    hero.color = color(0xe5, 0x39, 0x35);
    hero.spans = vec![Span::key(0, 12), Span::tweened(12, 16, TweenKind::Classic), Span::key(28, 10), Span::tweened(38, 12, TweenKind::Shape)];
    let mut mouth = LayerRow::new(12, "Mouth");
    mouth.depth = 1;
    mouth.color = color(0x43, 0xa0, 0x47);
    mouth.spans = vec![Span::key(0, 4), Span::blank(4, 3), Span::key(7, 5), Span::key(12, 20), Span::tweened(32, 18, TweenKind::Motion)];
    let mut bg = LayerRow::new(13, "Background");
    bg.color = color(0x1e, 0x88, 0xe5);
    bg.locked = true;
    bg.spans = vec![Span::key(0, 50)];
    let mut rtl = LayerRow::new(14, "שכבת רקע");
    rtl.color = color(0xfb, 0x8c, 0x00);
    rtl.outline = true;
    rtl.spans = vec![Span::key(0, 6), Span::key(6, 44)];
    let mut sound = LayerRow::new(15, "Voice");
    sound.kind = LayerKind::Guide;
    sound.color = color(0x8e, 0x24, 0xaa);
    let peaks: Vec<f32> = (0..64).map(|i| ((i as f32 * 0.7).sin().abs() * 0.8 + 0.1).min(1.0)).collect();
    let mut voice = Span::key(2, 40);
    voice.sound = Some(Waveform { peaks });
    voice.label = Some("intro".into());
    sound.spans = vec![voice];
    let mut actions = LayerRow::new(16, "Actions");
    actions.visible = false;
    let mut s = Span::key(0, 1);
    s.has_script = true;
    let mut label = Span::key(24, 1);
    label.label = Some("jump".into());
    actions.spans = vec![s, label];
    TimelineModel {
        layers: vec![folder, hero, mouth, bg, rtl, sound, actions],
        markers: vec![anim_ui::Marker::new(1, 4, "Intro"), anim_ui::Marker::new(2, 30, "Chorus")],
        frame_count: 50,
        current_frame: 17,
        fps: 24.0,
        onion: anim_ui::timeline::OnionSkin { enabled: true, outlines: false, before: 2, after: 2 },
        playing: false,
    }
}

fn gallery(theme: Theme) -> Gallery {
    let mut stage = StageViewState::new(vec2(640.0, 360.0));
    stage.show_grid = true;
    stage.grid_size = 40.0;
    stage.guides = vec![Guide { axis: Axis::Vertical, pos: 320.0 }, Guide { axis: Axis::Horizontal, pos: 190.0 }];
    let accent = Color32::from_rgb(76, 154, 255);
    stage.overlays = vec![
        Overlay::TransformBox {
            rect: Rect::from_min_size(pos2(60.0, 230.0), vec2(200.0, 100.0)),
            rotation: 0.0,
            handles: true,
            pivot: Some(pos2(160.0, 280.0)),
            color: accent,
        },
        Overlay::Path {
            nodes: vec![
                PathNode { pos: pos2(380.0, 300.0), control_in: None, control_out: Some(pos2(430.0, 230.0)), selected: true },
                PathNode { pos: pos2(500.0, 250.0), control_in: Some(pos2(460.0, 200.0)), control_out: Some(pos2(540.0, 300.0)), selected: false },
                PathNode { pos: pos2(590.0, 310.0), control_in: Some(pos2(570.0, 330.0)), control_out: None, selected: false },
            ],
            closed: false,
            style: OverlayStyle::line(Color32::from_rgb(229, 57, 53)),
            show_controls: true,
        },
        Overlay::Marquee { rect: Rect::from_min_size(pos2(270.0, 110.0), vec2(110.0, 90.0)) },
        Overlay::Marker { pos: pos2(320.0, 190.0), shape: MarkerShape::Diamond, color: accent, filled: true },
        Overlay::Text { pos: pos2(60.0, 226.0), text: "Hero".into(), color: accent },
    ];
    let view = TimelineViewState {
        selection: FrameSelection { layers: vec![11, 12], first: 14, last: 20 },
        selected_layers: vec![11],
        ..TimelineViewState::default()
    };
    let item = |id: u64, name: &str, kind, depth, expanded, thumb: bool| LibraryItemView {
        id,
        name: name.into(),
        kind,
        depth,
        expanded,
        thumbnail: thumb.then_some(egui::TextureId::Managed(0)),
    };
    let library = LibraryModel {
        items: vec![
            item(1, "Characters", LibraryKind::Folder, 0, true, false),
            item(2, "Hero", LibraryKind::MovieClip, 1, false, false),
            item(3, "Hero mouth", LibraryKind::Graphic, 1, false, false),
            item(4, "Jump button", LibraryKind::Button, 0, false, false),
            item(5, "sky.png", LibraryKind::Bitmap, 0, false, false),
            item(6, "voice-intro.wav", LibraryKind::Sound, 0, false, false),
            item(7, "רקע", LibraryKind::Graphic, 0, false, false),
        ],
        selected: Some(2),
        filter: String::new(),
        preview: anim_ui::library::LibraryPreview {
            playable: true,
            progress_permille: 350,
            caption: "Movie clip, 24 frames, 550 x 400".into(),
            ..Default::default()
        },
    };
    let props = vec![
        Property::new("x", "X", PropValue::number(120.0).precision(1, 0.5).with_suffix(" px")).in_section("Position and size"),
        Property::new("y", "Y", PropValue::number(80.0).precision(1, 0.5).with_suffix(" px")).in_section("Position and size"),
        Property::new("w", "Width", PropValue::number_in(200.0, 0.0, 4096.0).with_suffix(" px")).in_section("Position and size"),
        Property::new("h", "Height", PropValue::number_in(100.0, 0.0, 4096.0).with_suffix(" px")).in_section("Position and size"),
        Property::new("fill", "Fill", PropValue::Color(Rgba8 { r: 60, g: 160, b: 90, a: 255 })).in_section("Colour"),
        Property::new("alpha", "Alpha", PropValue::number_in(100.0, 0.0, 100.0).with_suffix(" %")).in_section("Colour"),
        Property::new("blend", "Blend", PropValue::choice(0, &["Normal", "Multiply", "Screen", "Overlay"])).in_section("Display"),
        Property::new("name", "Instance name", PropValue::Text("hero_mc".into())).in_section("Display"),
        Property::new("vis", "Visible", PropValue::Bool(true)).in_section("Display"),
        Property::new("heb", "שם", PropValue::Text("גיבור".into())).in_section("Display"),
    ];
    Gallery {
        theme,
        layout: dock::default_layout(),
        stage,
        texture: None,
        model: timeline_model(),
        view,
        props,
        library,
        transport: TransportState { playing: false, looping: true, fps: 24.0, frame: 17, frame_count: 50 },
        tools: toolbar::animate_tools(),
        tool: "select".into(),
        fonts: 0,
        dock_error: false,
    }
}

fn frame(ui: &mut egui::Ui, g: &mut Gallery) {
    if g.fonts == 0 {
        g.fonts = anim_ui::theme::install_fallback_fonts(ui.ctx());
        g.fonts = g.fonts.max(1);
    }
    g.theme.apply(ui.ctx());
    let texture = g.texture.get_or_insert_with(|| ui.ctx().load_texture("artwork", artwork(), egui::TextureOptions::LINEAR)).clone();
    for i in &mut g.library.items {
        if matches!(i.kind, LibraryKind::Bitmap | LibraryKind::MovieClip) {
            i.thumbnail = Some(texture.id());
        }
    }
    let Gallery { layout, stage, model, view, props, library, transport, tools, tool, dock_error, .. } = g;
    let output = dock::show_dock(ui, egui::Id::new("gallery-dock"), layout, PanelId::title, |ui, panel| match panel {
        PanelId::Tools => {
            for e in anim_ui::toolbar(ui, tools, tool) {
                let anim_ui::ToolbarEvent::Select(id) = e;
                *tool = id;
            }
        }
        PanelId::Stage => {
            ui.vertical(|ui| {
                let bar_h = 34.0;
                let rest = ui.available_height() - bar_h;
                ui.allocate_ui(vec2(ui.available_width(), rest), |ui| {
                    for e in anim_ui::stage_view(ui, stage, Some(&texture)) {
                        if let anim_ui::StageEvent::ViewChanged(v) = e {
                            stage.view = v;
                            stage.fit_request = false;
                        }
                    }
                });
                let _ = anim_ui::transport_bar(ui, transport);
            });
        }
        PanelId::Timeline => {
            for e in anim_ui::timeline(ui, model, view) {
                view.apply(&e);
            }
        }
        PanelId::Properties => {
            let _ = anim_ui::property_grid(ui, props);
        }
        PanelId::Library => {
            let _ = anim_ui::library_panel(ui, library);
        }
        _ => {}
    });
    *dock_error = output.error.is_some();
}

fn save(image: &image::RgbaImage, name: &str) {
    let Ok(dir) = std::env::var("ANIM_UI_SCREENSHOT_DIR") else { return };
    let path = std::path::Path::new(&dir).join(name);
    let _ = std::fs::create_dir_all(&dir);
    image.save(&path).unwrap_or_else(|e| panic!("saving {}: {e}", path.display()));
}

fn render(theme: Theme, name: &str) {
    let mut harness = Harness::builder().with_size(vec2(1440.0, 880.0)).with_pixels_per_point(1.0).wgpu().build_ui_state(frame, gallery(theme));
    harness.run_steps(6);
    assert!(!harness.state().dock_error, "dock reported an error");
    match harness.render() {
        Ok(image) => {
            let mut colours = std::collections::HashSet::new();
            colours.extend(image.pixels().map(|p| p.0).take(400_000));
            assert!(colours.len() > 64, "the render looks blank: {} colours", colours.len());
            save(&image, name);
        }
        Err(e) => eprintln!("no GPU adapter, skipping the PNG: {e}"),
    }
}

#[test]
fn gallery_dark() {
    render(Theme::Dark, "anim-ui-gallery-dark.png");
}

#[test]
fn gallery_light() {
    render(Theme::Light, "anim-ui-gallery-light.png");
}

#[test]
fn the_stage_view_state_is_independent_of_the_dock() {
    let mut s = StageViewState::new(vec2(100.0, 100.0));
    s.view = StageView::fit(vec2(300.0, 300.0), s.doc_size, 0.0);
    assert!(s.view.zoom > 2.9);
    let _ = (Pos2::ZERO, Rect::NOTHING);
}
