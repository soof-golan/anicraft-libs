//! Property tests: any sequence of editing operations leaves the document well formed, validation never panics, and
//! serde round trips. Hostile frame data never panics a query.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]

use anim_core::Affine;
use anim_scene::edit::{self, Registration, SymbolSpec};
use anim_scene::{Document, Element, Frame, LayerKind, SymbolInstance, SymbolKind, TimelineId, WarningKind};
use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Op {
    InsertFrames(usize, u32, u32),
    RemoveFrames(usize, u32, u32),
    InsertKeyframe(usize, u32),
    InsertBlank(usize, u32),
    ClearKeyframe(usize, u32),
    AddLayer(usize, bool),
    RemoveLayer(usize),
    MoveLayer(usize, usize),
    SetParent(usize, Option<usize>),
    AddInstance(usize, u32),
    ClassicTween(usize, u32),
    Convert(usize, u32),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..6_usize, 0..40_u32, 0..8_u32).prop_map(|(l, a, c)| Op::InsertFrames(l, a, c)),
        (0..6_usize, 0..40_u32, 0..8_u32).prop_map(|(l, a, c)| Op::RemoveFrames(l, a, c)),
        (0..6_usize, 0..40_u32).prop_map(|(l, f)| Op::InsertKeyframe(l, f)),
        (0..6_usize, 0..40_u32).prop_map(|(l, f)| Op::InsertBlank(l, f)),
        (0..6_usize, 0..40_u32).prop_map(|(l, f)| Op::ClearKeyframe(l, f)),
        (0..8_usize, any::<bool>()).prop_map(|(a, f)| Op::AddLayer(a, f)),
        (0..8_usize).prop_map(Op::RemoveLayer),
        (0..8_usize, 0..8_usize).prop_map(|(a, b)| Op::MoveLayer(a, b)),
        (0..8_usize, proptest::option::of(0..8_usize)).prop_map(|(a, b)| Op::SetParent(a, b)),
        (0..6_usize, 0..40_u32).prop_map(|(l, f)| Op::AddInstance(l, f)),
        (0..6_usize, 0..40_u32).prop_map(|(l, f)| Op::ClassicTween(l, f)),
        (0..6_usize, 0..40_u32).prop_map(|(l, f)| Op::Convert(l, f)),
    ]
}

fn apply(doc: &mut Document, op: &Op, counter: &mut u32) {
    let id = TimelineId::Scene(0);
    match *op {
        Op::InsertFrames(l, a, c) => drop(edit::insert_frames(doc, &id, l, a, c)),
        Op::RemoveFrames(l, a, c) => drop(edit::remove_frames(doc, &id, l, a, c)),
        Op::InsertKeyframe(l, f) => drop(edit::insert_keyframe(doc, &id, l, f)),
        Op::InsertBlank(l, f) => drop(edit::insert_blank_keyframe(doc, &id, l, f)),
        Op::ClearKeyframe(l, f) => drop(edit::clear_keyframe(doc, &id, l, f)),
        Op::AddLayer(at, folder) => {
            let kind = if folder { LayerKind::Folder } else { LayerKind::Normal };
            drop(edit::add_layer(doc, &id, at, None, kind));
        }
        Op::RemoveLayer(l) => {
            if doc.scenes[0].layers.len() > 1 {
                drop(edit::remove_layer(doc, &id, l));
            }
        }
        Op::MoveLayer(a, b) => drop(edit::move_layer(doc, &id, a, b)),
        Op::SetParent(l, p) => drop(edit::set_layer_parent(doc, &id, l, p)),
        Op::AddInstance(l, f) => drop(edit::add_symbol_instance(doc, &id, l, f, "sym", Affine::IDENTITY)),
        Op::ClassicTween(l, f) => drop(edit::create_classic_tween(doc, &id, l, f)),
        Op::Convert(l, f) => {
            *counter += 1;
            let name = format!("conv{counter}");
            drop(edit::convert_to_symbol(doc, &id, l, f, &[0], &SymbolSpec { name: &name, kind: SymbolKind::Graphic, registration: Registration::Center }));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn edit_sequences_keep_layers_well_formed(ops in proptest::collection::vec(op(), 0..40)) {
        let mut doc = Document::new(100.0, 100.0, 24.0);
        doc.add_symbol("sym", SymbolKind::Graphic).unwrap();
        let mut counter = 0;
        for op in &ops {
            apply(&mut doc, op, &mut counter);
            let tl = &doc.scenes[0];
            for (i, layer) in tl.layers.iter().enumerate() {
                prop_assert!(layer.is_well_formed(), "layer {} after {:?}: {:?}", i, op, layer.frames.iter().map(|f| (f.index, f.duration)).collect::<Vec<_>>());
                prop_assert!(layer.frames.iter().all(|f| f.duration > 0));
                if let Some(p) = layer.parent_layer_index {
                    prop_assert!(p < i, "parent {} of layer {} after {:?}", p, i, op);
                }
            }
            let warnings = doc.validate();
            let structural = warnings.iter().any(|w| matches!(
                w.kind,
                WarningKind::OverlappingFrames { .. } | WarningKind::UnsortedFrames | WarningKind::ZeroDuration | WarningKind::BadLayerParent(_)
                    | WarningKind::SymbolCycle { .. } | WarningKind::DanglingSymbol(_)
            ));
            prop_assert!(!structural, "{:?} after {:?}", warnings, op);
        }
        let json = serde_json::to_string(&doc).unwrap();
        prop_assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), doc);
    }

    #[test]
    fn queries_survive_hostile_frames(spans in proptest::collection::vec((any::<u32>(), any::<u32>()), 0..20), probe in any::<u32>()) {
        let mut doc = Document::new(10.0, 10.0, 24.0);
        let layer = &mut doc.scenes[0].layers[0];
        layer.frames = spans.iter().map(|&(i, d)| Frame::blank(i, d)).collect();
        let _ = layer.keyframe_at(probe);
        let _ = layer.next_keyframe_after(probe);
        let _ = layer.frame_count();
        layer.sort_frames();
        let _ = layer.keyframe_at(probe);
        let _ = doc.validate();
        let _ = doc.scenes[0].frame_count();
        let _ = doc.scenes[0].keyframes_at(probe);
    }

    #[test]
    fn hostile_layer_parents_never_break_the_tree(parents in proptest::collection::vec(proptest::option::of(any::<usize>()), 0..30)) {
        let mut doc = Document::new(10.0, 10.0, 24.0);
        for (i, p) in parents.iter().enumerate() {
            let at = doc.scenes[0].layers.len();
            let _ = edit::add_layer(&mut doc, &TimelineId::Scene(0), at, None, LayerKind::Folder);
            doc.scenes[0].layers[i.min(at)].parent_layer_index = *p;
        }
        let tl = &doc.scenes[0];
        let mut count = 0;
        let mut stack: Vec<_> = tl.layer_tree();
        while let Some(n) = stack.pop() {
            count += 1;
            stack.extend(n.children);
        }
        prop_assert_eq!(count, tl.layers.len());
        let _ = tl.layer_depths();
        for i in 0..tl.layers.len() {
            let _ = tl.is_effectively_visible(i);
            let _ = tl.ancestors_of(i);
        }
        let _ = doc.validate();
    }

    #[test]
    fn media_store_round_trips_content(blobs in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..64), 0..16)) {
        let mut store = anim_scene::MediaStore::new();
        let ids: Vec<_> = blobs.iter().map(|b| store.insert(b.clone()).unwrap()).collect();
        for (id, blob) in ids.iter().zip(&blobs) {
            prop_assert_eq!(store.get(*id).unwrap(), blob.as_slice());
        }
        let unique: std::collections::BTreeSet<_> = blobs.iter().collect();
        prop_assert_eq!(store.len(), unique.len());
    }
}

#[test]
fn instance_element_json_shape_is_stable() {
    // A guard for dependents that persist documents with serde: the variant names are part of the format.
    let e = Element::SymbolInstance(Box::new(SymbolInstance::new("a", Affine::IDENTITY)));
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.starts_with("{\"SymbolInstance\":{\"library_item_name\":\"a\""), "{json}");
}
