use anim_core::{BezPath, Point};
use anim_shape::{EdgeShape, PathInput};

use super::*;
use crate::element::ShapeElement;
use crate::query::RefKind;
use crate::timeline::FrameSound;
use crate::validate::WarningKind;

fn scene() -> TimelineId {
    TimelineId::Scene(0)
}

fn doc() -> Document {
    Document::new(550.0, 400.0, 24.0)
}

fn instance(name: &str) -> Element {
    Element::SymbolInstance(Box::new(SymbolInstance::new(name, Affine::IDENTITY)))
}

fn square(x: f64, y: f64, s: f64) -> Element {
    let mut p = BezPath::new();
    p.move_to(Point::new(x, y));
    p.line_to(Point::new(x + s, y));
    p.line_to(Point::new(x + s, y + s));
    p.line_to(Point::new(x, y + s));
    p.close_path();
    Element::Shape(ShapeElement::new(EdgeShape::from_paths(&[PathInput::new(p, Some(FillStyle::Solid(Rgba8::BLACK)), None)])))
}

fn spec(name: &str, kind: SymbolKind, registration: Registration) -> SymbolSpec<'_> {
    SymbolSpec::new(name, kind, registration)
}

fn spans(d: &Document, layer: usize) -> Vec<(u32, u32)> {
    d.scenes[0].layers[layer].frames.iter().map(|f| (f.index, f.duration)).collect()
}

/// A layer with keyframes at 0 (5 frames) and 5 (5 frames).
fn two_key_doc() -> Document {
    let mut d = doc();
    d.scenes[0].layers[0].frames = vec![Frame::blank(0, 5), Frame::blank(5, 5)];
    d
}

#[test]
fn insert_frames_extends_the_covering_span_and_shifts_later_keys() {
    let mut d = two_key_doc();
    insert_frames(&mut d, &scene(), 0, 2, 3).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 8), (8, 5)]);
    insert_frames(&mut d, &scene(), 0, 20, 2).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 8), (8, 14)]);
    assert!(insert_frames(&mut d, &scene(), 0, 0, 0).is_err());
    assert!(insert_frames(&mut d, &scene(), 3, 0, 1).is_err());
    assert!(insert_frames(&mut d, &scene(), 0, MAX_FRAMES, 1).is_err());
    assert!(insert_frames(&mut d, &TimelineId::Symbol("x".into()), 0, 0, 1).is_err());
}

#[test]
fn insert_frames_on_an_empty_layer_creates_a_blank_keyframe() {
    let mut d = doc();
    d.scenes[0].layers[0].frames.clear();
    insert_frames(&mut d, &scene(), 0, 0, 6).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 6)]);
}

#[test]
fn remove_frames_shortens_spans_and_pulls_keys_left() {
    let mut d = two_key_doc();
    remove_frames(&mut d, &scene(), 0, 3, 4).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 3), (3, 3)]);
    let mut d = two_key_doc();
    remove_frames(&mut d, &scene(), 0, 5, 5).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 5)]);
    let mut d = two_key_doc();
    remove_frames(&mut d, &scene(), 0, 0, 100).unwrap();
    assert!(spans(&d, 0).is_empty());
    assert!(remove_frames(&mut d, &scene(), 0, 0, 0).is_err());
}

#[test]
fn removing_the_end_keyframe_clears_the_tween() {
    let mut d = two_key_doc();
    d.scenes[0].layers[0].frames[0].elements.push(instance("s"));
    d.add_symbol("s", SymbolKind::Graphic).unwrap();
    create_classic_tween(&mut d, &scene(), 0, 1).unwrap();
    assert!(d.scenes[0].layers[0].frames[0].is_tweened());
    remove_frames(&mut d, &scene(), 0, 5, 5).unwrap();
    assert!(!d.scenes[0].layers[0].frames[0].is_tweened());
}

#[test]
fn keyframe_in_the_middle_of_a_span_copies_content() {
    let mut d = two_key_doc();
    d.scenes[0].layers[0].frames[0].elements.push(instance("a"));
    d.scenes[0].layers[0].frames[0].label = Some("intro".into());
    d.scenes[0].layers[0].frames[0].sound = Some(FrameSound::new("snd"));
    insert_keyframe(&mut d, &scene(), 0, 2).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 2), (2, 3), (5, 5)]);
    let f = &d.scenes[0].layers[0].frames;
    assert_eq!(f[1].elements.len(), 1);
    assert!(f[1].label.is_none() && f[1].sound.is_none());
    assert_eq!(f[0].label.as_deref(), Some("intro"));
    assert!(matches!(insert_keyframe(&mut d, &scene(), 0, 2), Err(SceneError::AlreadyKeyframe { layer: 0, frame: 2 })));
}

#[test]
fn blank_keyframe_is_empty() {
    let mut d = two_key_doc();
    d.scenes[0].layers[0].frames[0].elements.push(instance("a"));
    insert_blank_keyframe(&mut d, &scene(), 0, 3).unwrap();
    assert!(d.scenes[0].layers[0].frames[1].elements.is_empty());
    assert_eq!(d.scenes[0].layers[0].frames[0].elements.len(), 1);
    assert_eq!(spans(&d, 0), vec![(0, 3), (3, 2), (5, 5)]);
}

#[test]
fn keyframe_past_the_end_pads_with_the_last_keyframe() {
    let mut d = two_key_doc();
    d.scenes[0].layers[0].frames[1].elements.push(instance("a"));
    insert_keyframe(&mut d, &scene(), 0, 14).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 5), (5, 9), (14, 1)]);
    assert_eq!(d.scenes[0].layers[0].frames[2].elements.len(), 1);
    let mut d = doc();
    d.scenes[0].layers[0].frames.clear();
    insert_blank_keyframe(&mut d, &scene(), 0, 4).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 4), (4, 1)]);
    let mut d = doc();
    d.scenes[0].layers[0].frames.clear();
    insert_keyframe(&mut d, &scene(), 0, 0).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 1)]);
}

#[test]
fn keyframe_in_a_gap_fills_up_to_the_next_keyframe() {
    let mut d = doc();
    d.scenes[0].layers[0].frames = vec![Frame::blank(0, 2), Frame::blank(6, 2)];
    insert_keyframe(&mut d, &scene(), 0, 3).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 2), (3, 3), (6, 2)]);
}

#[test]
fn clear_keyframe_merges_into_the_previous_one() {
    let mut d = two_key_doc();
    clear_keyframe(&mut d, &scene(), 0, 5).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 10)]);
    assert!(clear_keyframe(&mut d, &scene(), 0, 3).is_err());
    let mut d = two_key_doc();
    d.scenes[0].layers[0].frames[0].elements.push(instance("a"));
    clear_keyframe(&mut d, &scene(), 0, 0).unwrap();
    assert!(d.scenes[0].layers[0].frames[0].elements.is_empty());
    assert_eq!(spans(&d, 0).len(), 2);
}

#[test]
fn classic_tween_needs_an_instance_and_an_end_keyframe() {
    let mut d = two_key_doc();
    d.add_symbol("s", SymbolKind::Graphic).unwrap();
    assert!(matches!(create_classic_tween(&mut d, &scene(), 0, 0), Err(SceneError::NotTweenable(_))));
    d.scenes[0].layers[0].frames[0].elements.push(square(0.0, 0.0, 10.0));
    assert!(matches!(create_classic_tween(&mut d, &scene(), 0, 0), Err(SceneError::NotTweenable(_))));
    d.scenes[0].layers[0].frames[0].elements = vec![instance("s"), instance("s")];
    assert!(matches!(create_classic_tween(&mut d, &scene(), 0, 0), Err(SceneError::NotTweenable(_))));
    d.scenes[0].layers[0].frames[0].elements = vec![instance("s")];
    assert!(matches!(create_classic_tween(&mut d, &scene(), 0, 7), Err(SceneError::NoEndKeyframe { .. })));
    create_classic_tween(&mut d, &scene(), 0, 3).unwrap();
    let f = &d.scenes[0].layers[0].frames[0];
    assert_eq!(f.tween_type, TweenType::Classic);
    assert_eq!(f.key_mode, KEY_MODE_CLASSIC_TWEEN);
    assert!(f.motion_tween_snap);
    set_classic_ease(&mut d, &scene(), 0, 0, -40).unwrap();
    assert_eq!(d.scenes[0].layers[0].frames[0].acceleration, -40);
    assert!(set_classic_ease(&mut d, &scene(), 0, 0, 101).is_err());
    remove_tween(&mut d, &scene(), 0, 0).unwrap();
    assert_eq!(d.scenes[0].layers[0].frames[0].tween_type, TweenType::None);
    assert_eq!(d.scenes[0].layers[0].frames[0].acceleration, 0);
    assert!(d.validate().iter().all(|w| !matches!(w.kind, WarningKind::TweenWithoutEnd)));
}

#[test]
fn shape_tween_needs_shapes_on_both_ends() {
    let mut d = two_key_doc();
    d.scenes[0].layers[0].frames[0].elements.push(square(0.0, 0.0, 10.0));
    assert!(matches!(create_shape_tween(&mut d, &scene(), 0, 0), Err(SceneError::NotTweenable(_))));
    d.scenes[0].layers[0].frames[1].elements.push(square(50.0, 0.0, 20.0));
    create_shape_tween(&mut d, &scene(), 0, 0).unwrap();
    assert_eq!(d.scenes[0].layers[0].frames[0].tween_type, TweenType::Shape);
    assert_eq!(d.scenes[0].layers[0].frames[0].key_mode, KEY_MODE_SHAPE_TWEEN);
    assert!(d.validate().is_empty());
}

#[test]
fn convert_to_symbol_moves_elements_into_the_library() {
    let mut d = doc();
    {
        let f = &mut d.scenes[0].layers[0].frames[0];
        f.elements = vec![square(100.0, 100.0, 40.0), instance("zzz"), square(300.0, 100.0, 40.0)];
    }
    let at = convert_to_symbol(&mut d, &scene(), 0, 0, &[2, 0], &spec("art/box", SymbolKind::Graphic, Registration::TopLeft)).unwrap();
    assert_eq!(at, 0);
    let f = &d.scenes[0].layers[0].frames[0];
    assert_eq!(f.elements.len(), 2);
    let inst = f.elements[0].as_symbol_instance().unwrap();
    assert_eq!(inst.library_item_name, "art/box");
    assert_eq!(inst.symbol_type, Some(SymbolKind::Graphic));
    let co = inst.matrix.as_coeffs();
    assert!((co[4] - 100.0).abs() < 0.5 && (co[5] - 100.0).abs() < 0.5, "{co:?}");
    let sym = d.library.find_symbol("art/box").unwrap();
    assert_eq!(sym.timeline.layers[0].frames[0].elements.len(), 2);
    let inner = sym.timeline.layers[0].frames[0].elements[0].matrix().unwrap().as_coeffs();
    assert!((inner[4] + 100.0).abs() < 0.5, "{inner:?}");
    assert!(d.library.find("art").is_some());
    let bounds = crate::element_bounds(&f.elements[0], &d.library).unwrap();
    assert!((bounds.x0 - 100.0).abs() < 0.5 && (bounds.x1 - 340.0).abs() < 0.5, "{bounds:?}");
}

#[test]
fn convert_to_symbol_errors_leave_the_document_alone() {
    let mut d = doc();
    d.scenes[0].layers[0].frames[0].elements = vec![square(0.0, 0.0, 10.0)];
    let before = d.clone();
    let plain = spec("a", SymbolKind::Graphic, Registration::Origin);
    assert!(matches!(convert_to_symbol(&mut d, &scene(), 0, 0, &[], &plain), Err(SceneError::EmptySelection)));
    assert!(matches!(convert_to_symbol(&mut d, &scene(), 0, 0, &[3], &plain), Err(SceneError::NoSuchElement { element: 3, count: 1 })));
    assert!(convert_to_symbol(&mut d, &scene(), 0, 0, &[0], &spec("", SymbolKind::Graphic, Registration::Origin)).is_err());
    assert!(convert_to_symbol(&mut d, &scene(), 0, 9, &[0], &plain).is_err());
    assert_eq!(d, before);
    convert_to_symbol(&mut d, &scene(), 0, 0, &[0], &spec("a", SymbolKind::MovieClip, Registration::Center)).unwrap();
    d.scenes[0].layers[0].frames[0].elements.push(square(0.0, 0.0, 1.0));
    assert!(matches!(convert_to_symbol(&mut d, &scene(), 0, 0, &[1], &plain), Err(SceneError::NameTaken(_))));
}

#[test]
fn add_symbol_instance_refuses_cycles_and_missing_symbols() {
    let mut d = doc();
    d.add_symbol("a", SymbolKind::MovieClip).unwrap();
    d.add_symbol("b", SymbolKind::Graphic).unwrap();
    assert!(matches!(add_symbol_instance(&mut d, &scene(), 0, 0, "nope", Affine::IDENTITY), Err(SceneError::NoSuchItem(_))));
    let i = add_symbol_instance(&mut d, &scene(), 0, 0, "a", Affine::translate((5.0, 5.0))).unwrap();
    assert_eq!(i, 0);
    add_symbol_instance(&mut d, &TimelineId::Symbol("a".into()), 0, 0, "b", Affine::IDENTITY).unwrap();
    assert!(matches!(add_symbol_instance(&mut d, &TimelineId::Symbol("b".into()), 0, 0, "a", Affine::IDENTITY), Err(SceneError::Cycle(_))));
    assert!(matches!(add_symbol_instance(&mut d, &TimelineId::Symbol("a".into()), 0, 0, "a", Affine::IDENTITY), Err(SceneError::Cycle(_))));
    let inst = d.scenes[0].layers[0].frames[0].elements[0].as_symbol_instance().unwrap();
    assert_eq!(inst.symbol_type, Some(SymbolKind::MovieClip));
    assert!(d.validate().is_empty());
}

#[test]
fn add_element_creates_a_keyframe_when_nothing_covers_the_frame() {
    let mut d = doc();
    d.add_symbol("a", SymbolKind::Graphic).unwrap();
    add_element(&mut d, &scene(), 0, 7, square(0.0, 0.0, 3.0)).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 7), (7, 1)]);
    let removed = remove_elements(&mut d, &scene(), 0, 7, &[0]).unwrap();
    assert_eq!(removed.len(), 1);
    assert!(remove_elements(&mut d, &scene(), 0, 7, &[0]).is_err());
}

#[test]
fn add_layer_names_colours_and_parent_remapping() {
    let mut d = doc();
    d.scenes[0].layers[0].frames = vec![Frame::blank(0, 12)];
    let f = add_layer(&mut d, &scene(), 1, Some("Folder"), LayerKind::Folder).unwrap();
    assert_eq!(f, 1);
    let child = add_layer(&mut d, &scene(), 2, None, LayerKind::Normal).unwrap();
    assert_eq!(d.scenes[0].layers[child].name, "Layer_2");
    assert!(d.scenes[0].layers[child].autoname);
    assert_eq!(d.scenes[0].layers[child].frames[0].duration, 12);
    set_layer_parent(&mut d, &scene(), 2, Some(1)).unwrap();
    add_layer(&mut d, &scene(), 0, Some("Top"), LayerKind::Normal).unwrap();
    assert_eq!(d.scenes[0].layers[3].parent_layer_index, Some(2));
    assert_eq!(d.scenes[0].layers[0].name, "Top");
    assert!(add_layer(&mut d, &scene(), 0, Some(""), LayerKind::Normal).is_err());
    assert_ne!(d.scenes[0].layers[0].color, d.scenes[0].layers[1].color);
    assert!(d.validate().is_empty());
}

#[test]
fn set_layer_parent_rules() {
    let mut d = doc();
    add_layer(&mut d, &scene(), 1, Some("F"), LayerKind::Folder).unwrap();
    add_layer(&mut d, &scene(), 2, Some("M"), LayerKind::Mask).unwrap();
    add_layer(&mut d, &scene(), 3, Some("N"), LayerKind::Normal).unwrap();
    assert!(set_layer_parent(&mut d, &scene(), 3, Some(2)).is_ok());
    assert!(set_layer_parent(&mut d, &scene(), 3, Some(0)).is_err());
    assert!(set_layer_parent(&mut d, &scene(), 1, Some(3)).is_err());
    assert!(set_layer_parent(&mut d, &scene(), 3, Some(9)).is_err());
    assert!(set_layer_parent(&mut d, &scene(), 2, Some(1)).is_ok());
    assert!(matches!(set_layer_parent(&mut d, &scene(), 1, Some(0)), Err(SceneError::InvalidParent(_))));
    set_layer_parent(&mut d, &scene(), 3, None).unwrap();
    assert_eq!(d.scenes[0].layers[3].parent_layer_index, None);
    assert_eq!(d.scenes[0].resolved_kind(2), Some(LayerKind::Mask));
}

#[test]
fn remove_layer_takes_nested_layers_and_fixes_indices() {
    let mut d = doc();
    add_layer(&mut d, &scene(), 1, Some("F"), LayerKind::Folder).unwrap();
    add_layer(&mut d, &scene(), 2, Some("in1"), LayerKind::Normal).unwrap();
    add_layer(&mut d, &scene(), 3, Some("in2"), LayerKind::Normal).unwrap();
    add_layer(&mut d, &scene(), 4, Some("G"), LayerKind::Folder).unwrap();
    add_layer(&mut d, &scene(), 5, Some("gin"), LayerKind::Normal).unwrap();
    set_layer_parent(&mut d, &scene(), 2, Some(1)).unwrap();
    set_layer_parent(&mut d, &scene(), 3, Some(1)).unwrap();
    set_layer_parent(&mut d, &scene(), 5, Some(4)).unwrap();
    assert_eq!(remove_layer(&mut d, &scene(), 1).unwrap(), 3);
    let names: Vec<&str> = d.scenes[0].layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Layer_1", "G", "gin"]);
    assert_eq!(d.scenes[0].layers[2].parent_layer_index, Some(1));
    assert!(remove_layer(&mut d, &scene(), 9).is_err());
    assert!(d.validate().is_empty());
}

#[test]
fn move_layer_moves_blocks_and_remaps_parents() {
    let mut d = doc();
    add_layer(&mut d, &scene(), 1, Some("F"), LayerKind::Folder).unwrap();
    add_layer(&mut d, &scene(), 2, Some("c"), LayerKind::Normal).unwrap();
    add_layer(&mut d, &scene(), 3, Some("z"), LayerKind::Normal).unwrap();
    set_layer_parent(&mut d, &scene(), 2, Some(1)).unwrap();
    // Move the folder block (F, c) to the very top.
    let new = move_layer(&mut d, &scene(), 1, 0).unwrap();
    assert_eq!(new, 0);
    let names: Vec<&str> = d.scenes[0].layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["F", "c", "Layer_1", "z"]);
    assert_eq!(d.scenes[0].layers[1].parent_layer_index, Some(0));
    // Move the loose layer to between F and c: the child stays under F.
    let new = move_layer(&mut d, &scene(), 3, 1).unwrap();
    assert_eq!(new, 1);
    assert!(d.validate().is_empty(), "{:?}", d.validate());
    // Moving a child above its folder drops the link.
    let c = d.scenes[0].layer_index_by_name("c").unwrap();
    move_layer(&mut d, &scene(), c, 0).unwrap();
    assert_eq!(d.scenes[0].layers[0].name, "c");
    assert_eq!(d.scenes[0].layers[0].parent_layer_index, None);
    assert!(d.validate().is_empty());
    assert!(move_layer(&mut d, &scene(), 99, 0).is_err());
    let last = move_layer(&mut d, &scene(), 0, 999).unwrap();
    assert_eq!(last, d.scenes[0].layers.len() - 1);
}

#[test]
fn rename_layer_marks_the_name_as_typed() {
    let mut d = doc();
    rename_layer(&mut d, &scene(), 0, "Background").unwrap();
    assert_eq!(d.scenes[0].layers[0].name, "Background");
    assert!(!d.scenes[0].layers[0].autoname);
    assert!(rename_layer(&mut d, &scene(), 0, "").is_err());
    assert!(rename_layer(&mut d, &scene(), 4, "x").is_err());
}

#[test]
fn rename_library_item_updates_every_reference() {
    let mut d = doc();
    d.add_symbol("dir/hero", SymbolKind::MovieClip).unwrap();
    d.add_symbol("dir/hat", SymbolKind::Graphic).unwrap();
    add_symbol_instance(&mut d, &scene(), 0, 0, "dir/hero", Affine::IDENTITY).unwrap();
    add_symbol_instance(&mut d, &TimelineId::Symbol("dir/hero".into()), 0, 0, "dir/hat", Affine::IDENTITY).unwrap();
    d.scenes[0].layers[0].frames[0].sound = Some(FrameSound::new("dir/hero"));
    rename_library_item(&mut d, "dir/hero", "dir/knight").unwrap();
    assert!(d.library.find_symbol("dir/knight").is_some());
    assert_eq!(d.library.find_symbol("dir/knight").map(|s| s.timeline.name.as_str()), Some("knight"));
    assert!(d.references_to("dir/hero").is_empty());
    let uses = d.references_to("dir/knight");
    assert_eq!(uses.len(), 2);
    assert!(uses.iter().any(|r| r.kind == RefKind::Sound) && uses.iter().any(|r| r.kind == RefKind::SymbolInstance));
    assert!(matches!(rename_library_item(&mut d, "dir/knight", "dir/hat"), Err(SceneError::NameTaken(_))));
    assert!(matches!(rename_library_item(&mut d, "ghost", "x"), Err(SceneError::NoSuchItem(_))));
    assert!(rename_library_item(&mut d, "dir/hat", "bad//name").is_err());
    rename_library_item(&mut d, "dir/hat", "dir/hat").unwrap();
    // Folder rename moves its contents and their references.
    rename_library_item(&mut d, "dir", "assets").unwrap();
    assert!(d.library.find_symbol("assets/knight").is_some());
    assert!(d.library.find_symbol("assets/hat").is_some());
    assert!(d.library.find("dir").is_none());
    let dangling: Vec<_> = d.validate().into_iter().filter(|w| matches!(w.kind, WarningKind::DanglingSymbol(_))).collect();
    assert!(dangling.is_empty(), "{dangling:?}");
}

#[test]
fn bitmap_fill_paths_follow_bitmap_renames() {
    use anim_shape::{BitmapFill, StrokeStyle};
    let mut d = doc();
    d.library.push(LibraryItem::Bitmap(crate::BitmapItem::new("tex", "i1"))).unwrap();
    let mut shape = EdgeShape::default();
    shape.fills.push(FillStyle::Bitmap(BitmapFill { path: "tex".into(), matrix: Affine::IDENTITY, clipped: false, smoothing: true }));
    shape.strokes.push(StrokeStyle::default());
    d.scenes[0].layers[0].frames[0].elements.push(Element::Shape(ShapeElement::new(shape)));
    assert_eq!(d.references_to("tex").len(), 1);
    rename_library_item(&mut d, "tex", "paper").unwrap();
    assert_eq!(d.references_to("paper").len(), 1);
    assert!(d.references_to("tex").is_empty());
}

#[test]
fn scenes_can_be_added_and_removed() {
    let mut d = doc();
    assert_eq!(add_scene(&mut d, "Scene 2", 5).unwrap(), 1);
    assert_eq!(add_scene(&mut d, "Intro", 0).unwrap(), 0);
    assert_eq!(d.current_timeline, 1);
    assert!(add_scene(&mut d, "Intro", 0).is_err());
    assert!(add_scene(&mut d, "", 0).is_err());
    assert_eq!(remove_scene(&mut d, 0).unwrap().name, "Intro");
    assert_eq!(d.current_timeline, 0);
    remove_scene(&mut d, 1).unwrap();
    assert!(remove_scene(&mut d, 0).is_err());
    assert!(remove_scene(&mut d, 4).is_err());
}

#[test]
fn edits_on_symbol_timelines_work_like_scenes() {
    let mut d = doc();
    d.add_symbol("s", SymbolKind::MovieClip).unwrap();
    let id = TimelineId::Symbol("s".into());
    insert_frames(&mut d, &id, 0, 0, 9).unwrap();
    insert_keyframe(&mut d, &id, 0, 4).unwrap();
    add_layer(&mut d, &id, 0, None, LayerKind::Normal).unwrap();
    assert_eq!(d.timeline(&id).unwrap().frame_count(), 10);
    assert_eq!(d.timeline(&id).unwrap().layers[0].frames[0].duration, 10);
}

#[test]
fn json_and_postcard_round_trip_a_busy_document() {
    let mut d = doc();
    d.add_symbol("s", SymbolKind::Graphic).unwrap();
    add_symbol_instance(&mut d, &scene(), 0, 0, "s", Affine::translate((1.0, 2.0))).unwrap();
    d.scenes[0].layers[0].frames[0].elements.push(square(0.0, 0.0, 5.0));
    d.extra.set_attr("xflVersion", "22.01");
    d.extra.children.push("<PrinterSettings/>".into());
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), d);
    let bytes = postcard::to_allocvec(&d).unwrap();
    assert_eq!(postcard::from_bytes::<Document>(&bytes).unwrap(), d);
}

#[test]
fn remove_library_item_refuses_items_in_use() {
    let mut d = doc();
    d.add_symbol("f/a", SymbolKind::Graphic).unwrap();
    add_symbol_instance(&mut d, &scene(), 0, 0, "f/a", Affine::IDENTITY).unwrap();
    assert!(matches!(remove_library_item(&mut d, "f/a"), Err(SceneError::InUse(_))));
    assert!(matches!(remove_library_item(&mut d, "f"), Err(SceneError::InUse(_))));
    d.scenes[0].layers[0].frames[0].elements.clear();
    assert!(remove_library_item(&mut d, "f/a").is_ok());
    assert!(remove_library_item(&mut d, "f").is_ok());
    assert!(matches!(remove_library_item(&mut d, "f"), Err(SceneError::NoSuchItem(_))));
}

#[test]
fn duplicate_symbol_copies_independently() {
    let mut d = doc();
    d.add_symbol("a", SymbolKind::MovieClip).unwrap();
    d.library.find_symbol_mut("a").unwrap().timeline.layers[0].frames[0].elements.push(square(0.0, 0.0, 4.0));
    duplicate_symbol(&mut d, "a", "dup/b").unwrap();
    let (a, b) = (d.library.find_symbol("a").unwrap(), d.library.find_symbol("dup/b").unwrap());
    assert_ne!(a.item_id, b.item_id);
    assert_eq!(a.timeline.layers, b.timeline.layers);
    assert_eq!(b.timeline.name, "b");
    assert!(d.library.find("dup").is_some());
    assert!(matches!(duplicate_symbol(&mut d, "a", "a"), Err(SceneError::NameTaken(_))));
    assert!(matches!(duplicate_symbol(&mut d, "zzz", "q"), Err(SceneError::NoSuchItem(_))));
}

#[test]
fn labels_move_keyframe_and_duplicate_layer() {
    let mut d = two_key_doc();
    set_frame_label(&mut d, &scene(), 0, 7, Some(("end", LabelType::Anchor))).unwrap();
    assert_eq!(d.scenes[0].labels(), vec![(5, "end")]);
    set_frame_label(&mut d, &scene(), 0, 5, None).unwrap();
    assert!(d.scenes[0].labels().is_empty());
    assert!(set_frame_label(&mut d, &scene(), 0, 99, None).is_err());

    move_keyframe(&mut d, &scene(), 0, 5, 8).unwrap();
    assert_eq!(spans(&d, 0), vec![(0, 5), (8, 5)]);
    assert!(matches!(move_keyframe(&mut d, &scene(), 0, 8, 3), Err(SceneError::Occupied { .. })));
    assert!(matches!(move_keyframe(&mut d, &scene(), 0, 6, 9), Err(SceneError::NoFrame { .. })));
    assert!(matches!(move_keyframe(&mut d, &scene(), 0, 0, 5), Err(SceneError::Occupied { .. })));
    move_keyframe(&mut d, &scene(), 0, 0, 2).unwrap();
    assert_eq!(spans(&d, 0), vec![(2, 5), (8, 5)]);
}

#[test]
fn duplicate_layer_inserts_above_and_remaps_parents() {
    let mut d = doc();
    add_layer(&mut d, &scene(), 1, Some("F"), LayerKind::Folder).unwrap();
    add_layer(&mut d, &scene(), 2, Some("c"), LayerKind::Normal).unwrap();
    set_layer_parent(&mut d, &scene(), 2, Some(1)).unwrap();
    let at = duplicate_layer(&mut d, &scene(), 1).unwrap();
    assert_eq!(at, 1);
    let names: Vec<&str> = d.scenes[0].layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Layer_1", "F copy", "F", "c"]);
    assert_eq!(d.scenes[0].layers[3].parent_layer_index, Some(2));
    assert!(d.validate().is_empty());
    assert!(duplicate_layer(&mut d, &scene(), 9).is_err());
}

#[test]
fn stats_ticks_and_instance_kind() {
    let mut d = Document::new(100.0, 100.0, 30.0);
    d.add_symbol("g", SymbolKind::Graphic).unwrap();
    d.add_symbol("m/x", SymbolKind::Button).unwrap();
    add_symbol_instance(&mut d, &scene(), 0, 0, "g", Affine::IDENTITY).unwrap();
    let s = d.stats();
    assert_eq!((s.scenes, s.symbols, s.folders, s.layers, s.keyframes, s.elements), (1, 2, 1, 3, 3, 1));
    assert_eq!(d.tick_to_frame(d.frame_to_tick(45)), 45);
    assert_eq!(d.tick_to_frame(anim_core::Tick(-5)), 0);
    let mut inst = SymbolInstance::new("m/x", Affine::IDENTITY);
    assert_eq!(inst.effective_kind(&d.library), SymbolKind::Button);
    inst.symbol_type = Some(SymbolKind::Graphic);
    assert_eq!(inst.effective_kind(&d.library), SymbolKind::Graphic);
    assert_eq!(SymbolInstance::new("nope", Affine::IDENTITY).effective_kind(&d.library), SymbolKind::MovieClip);
}

#[test]
fn ease_lookup_falls_back_to_all() {
    use crate::timeline::{EaseCurve, EaseTarget, TweenEase};
    let mut f = Frame::blank(0, 4);
    assert!(f.ease_for(EaseTarget::Rotation).is_none());
    f.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Named("cubicIn".into()) });
    f.eases.push(TweenEase { target: EaseTarget::Rotation, curve: EaseCurve::Custom(vec![[0.0, 0.0], [1.0, 1.0]]) });
    assert!(matches!(f.ease_for(EaseTarget::Rotation), Some(EaseCurve::Custom(_))));
    assert!(matches!(f.ease_for(EaseTarget::Position), Some(EaseCurve::Named(n)) if n == "cubicIn"));
}

#[test]
fn registration_grid_names_and_points() {
    let b = Rect::new(10.0, 20.0, 50.0, 100.0);
    assert_eq!(Registration::Origin.point(b), None);
    assert_eq!(Registration::TopLeft.point(b), Some(Point::new(10.0, 20.0)));
    assert_eq!(Registration::Center.point(b), Some(Point::new(30.0, 60.0)));
    assert_eq!(Registration::BottomRight.point(b), Some(Point::new(50.0, 100.0)));
    assert_eq!(Registration::TopCenter.point(b), Some(Point::new(30.0, 20.0)));
    assert_eq!(Registration::MiddleLeft.point(b), Some(Point::new(10.0, 60.0)));
    assert_eq!(Registration::GRID.len(), 9);
    for r in Registration::GRID.into_iter().chain([Registration::Origin]) {
        assert_eq!(Registration::from_name(r.name()), Some(r));
        assert!(!r.label().is_empty());
    }
    assert_eq!(Registration::from_name("nowhere"), None);
}

#[test]
fn convert_to_symbol_takes_a_folder_linkage_and_nine_slice_guides() {
    let mut d = doc();
    add_element(&mut d, &scene(), 0, 0, square(100.0, 50.0, 90.0)).unwrap();
    let linkage = crate::symbols::default_linkage("panel", SymbolKind::MovieClip);
    let mut s = SymbolSpec::new("panel", SymbolKind::MovieClip, Registration::BottomRight);
    s.folder = Some("ui/skins");
    s.linkage = Some(&linkage);
    s.scale9 = true;
    assert_eq!(s.full_name(), "ui/skins/panel");
    convert_to_symbol(&mut d, &scene(), 0, 0, &[0], &s).unwrap();
    let sym = d.library.find_symbol("ui/skins/panel").unwrap();
    assert_eq!(sym.linkage.as_ref().and_then(|l| l.class_name.as_deref()), Some("panel"));
    // The registration point is the bottom right corner (190, 140), so the square sits at (-90, -90)..(0, 0).
    assert_eq!(crate::bounds::timeline_bounds(&sym.timeline, 0, &d.library), Some(Rect::new(-90.0, -90.0, 0.0, 0.0)));
    assert_eq!(sym.scale9_grid, Some(Rect::new(-60.0, -60.0, -30.0, -30.0)), "the middle third of the bounds, in the symbol's pixels");
    assert!(d.library.find("ui").is_some() && d.library.find("ui/skins").is_some(), "the folders were made");
    let Element::SymbolInstance(i) = &d.scenes[0].layers[0].frames[0].elements[0] else { panic!() };
    assert_eq!(i.matrix, Affine::translate((190.0, 140.0)));
}

#[test]
fn convert_to_symbol_refuses_what_a_graphic_cannot_do_and_changes_nothing() {
    let mut d = doc();
    add_element(&mut d, &scene(), 0, 0, square(0.0, 0.0, 10.0)).unwrap();
    let mut nine = SymbolSpec::new("g", SymbolKind::Graphic, Registration::Center);
    nine.scale9 = true;
    assert!(convert_to_symbol(&mut d, &scene(), 0, 0, &[0], &nine).is_err());
    let exported = crate::symbols::default_linkage("g", SymbolKind::MovieClip);
    let mut export = SymbolSpec::new("g", SymbolKind::Graphic, Registration::Center);
    export.linkage = Some(&exported);
    assert!(convert_to_symbol(&mut d, &scene(), 0, 0, &[0], &export).is_err());
    let mut no_class = crate::symbols::default_linkage("g", SymbolKind::MovieClip);
    no_class.class_name = None;
    let mut bad = SymbolSpec::new("g", SymbolKind::MovieClip, Registration::Center);
    bad.linkage = Some(&no_class);
    assert!(convert_to_symbol(&mut d, &scene(), 0, 0, &[0], &bad).is_err());
    assert_eq!(d.scenes[0].layers[0].frames[0].elements.len(), 1, "the square is still on the stage");
    assert!(d.library.find("g").is_none());
}
