//! Builders shared by the integration tests.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::missing_const_for_fn
)]

use anim_core::{Affine, BezPath, Point, Rgba8};
use anim_display::{DisplayList, Image, ImageStore, Node};
use anim_render::{RenderOptions, render};
use anim_scene::{Document, Element, Frame, Layer, LayerKind, ShapeElement, SymbolInstance, SymbolKind};
use anim_shape::{EdgeShape, FillStyle, PathInput};

pub const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
pub const GREEN: Rgba8 = Rgba8::opaque(0, 255, 0);
pub const BLUE: Rgba8 = Rgba8::opaque(0, 0, 255);
pub const WHITE: Rgba8 = Rgba8::WHITE;

pub fn rect_path(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(Point::new(x, y));
    p.line_to(Point::new(x + w, y));
    p.line_to(Point::new(x + w, y + h));
    p.line_to(Point::new(x, y + h));
    p.close_path();
    p
}

pub fn rect_shape(x: f64, y: f64, w: f64, h: f64, color: Rgba8) -> EdgeShape {
    EdgeShape::from_paths(&[PathInput::new(rect_path(x, y, w, h), Some(FillStyle::Solid(color)), None)])
}

/// A filled rectangle element with an identity matrix.
pub fn rect(x: f64, y: f64, w: f64, h: f64, color: Rgba8) -> Element {
    Element::Shape(ShapeElement::new(rect_shape(x, y, w, h, color)))
}

pub fn instance(name: &str, m: Affine) -> Element {
    Element::SymbolInstance(Box::new(SymbolInstance::new(name, m)))
}

pub fn keyframe(index: u32, duration: u32, elements: Vec<Element>) -> Frame {
    Frame::new(index, duration, elements)
}

pub fn layer(name: &str, frames: Vec<Frame>) -> Layer {
    let mut l = Layer::new(name, Rgba8::opaque(1, 2, 3));
    l.frames = frames;
    l
}

pub fn layer_kind(name: &str, kind: LayerKind, parent: Option<usize>, frames: Vec<Frame>) -> Layer {
    let mut l = layer(name, frames);
    l.kind = kind;
    l.parent_layer_index = parent;
    l
}

/// A 100x100 white document whose first scene has `layers` (top layer first).
pub fn doc(layers: Vec<Layer>) -> Document {
    let mut d = Document::new(100.0, 100.0, 24.0);
    d.scenes[0].layers = layers;
    d
}

pub fn add_symbol(d: &mut Document, name: &str, kind: SymbolKind, layers: Vec<Layer>) {
    d.add_symbol(name, kind).unwrap();
    d.library.find_symbol_mut(name).unwrap().timeline.layers = layers;
}

/// Renders a display list at stage size with no anti-aliasing.
pub fn draw(list: &DisplayList, images: &ImageStore) -> Image {
    let options = RenderOptions::for_stage(list.size).with_antialias(false);
    render(list, images, &options).unwrap()
}

pub fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    [image.premultiplied_rgba[i], image.premultiplied_rgba[i + 1], image.premultiplied_rgba[i + 2], image.premultiplied_rgba[i + 3]]
}

pub fn is(image: &Image, x: u32, y: u32, c: Rgba8) -> bool {
    pixel(image, x, y) == [c.r, c.g, c.b, c.a]
}

/// The groups at the top of a list.
pub fn top_groups(list: &DisplayList) -> Vec<&anim_display::Group> {
    list.nodes.iter().filter_map(|n| if let Node::Group(g) = n { Some(g) } else { None }).collect()
}
