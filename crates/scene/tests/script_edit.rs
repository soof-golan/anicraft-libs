//! Frame scripts and component parameters through the editing functions.
// Test code: unwrap and indexing are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use anim_core::Affine;
use anim_scene::edit::{set_component_param, set_frame_script};
use anim_scene::element::Element;
use anim_scene::timeline::Frame;
use anim_scene::{ComponentInstance, Document, SymbolInstance, TimelineId};

fn doc() -> Document {
    let mut d = Document::new(100.0, 100.0, 24.0);
    let layer = &mut d.scenes[0].layers[0];
    layer.frames.clear();
    layer.frames.push(Frame::blank(0, 10));
    d
}

#[test]
fn script_goes_on_a_keyframe_and_makes_one_when_needed() {
    let mut d = doc();
    let id = TimelineId::Scene(0);
    set_frame_script(&mut d, &id, 0, 0, Some("stop();")).unwrap();
    assert_eq!(d.scenes[0].layers[0].frames[0].actionscript.as_deref(), Some("stop();"));
    // Frame 5 is inside the span: a blank keyframe appears there.
    set_frame_script(&mut d, &id, 0, 4, Some("play();")).unwrap();
    let frames = &d.scenes[0].layers[0].frames;
    assert_eq!(frames.len(), 2);
    assert_eq!((frames[1].index, frames[1].actionscript.as_deref()), (4, Some("play();")));
    // Clearing removes the script but keeps the keyframe.
    set_frame_script(&mut d, &id, 0, 4, Some("  ")).unwrap();
    assert_eq!(d.scenes[0].layers[0].frames[1].actionscript, None);
    // Clearing on a plain frame does nothing at all.
    set_frame_script(&mut d, &id, 0, 2, None).unwrap();
    assert_eq!(d.scenes[0].layers[0].frames.len(), 2);
}

#[test]
fn component_parameters_edit_in_place() {
    let mut d = doc();
    let mut inst = SymbolInstance::new("Button", Affine::IDENTITY);
    inst.component = Some(ComponentInstance::from_params(&[("label", "Go", "String"), ("enabled", "true", "Boolean")]));
    d.scenes[0].layers[0].frames[0].elements.push(Element::SymbolInstance(Box::new(inst)));
    let id = TimelineId::Scene(0);
    set_component_param(&mut d, &id, 0, 0, 0, "label", "Next").unwrap();
    let Element::SymbolInstance(i) = &d.scenes[0].layers[0].frames[0].elements[0] else { panic!("not an instance") };
    assert_eq!(i.component.as_ref().unwrap().param("label").as_deref(), Some("Next"));
    assert!(set_component_param(&mut d, &id, 0, 0, 0, "nope", "x").is_err());
    assert!(set_component_param(&mut d, &id, 0, 0, 3, "label", "x").is_err());
}
