//! Builders shared by the integration tests.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::chunks_exact_to_as_chunks,
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

// ---- visual verification helpers ----

/// True when the pixel is within `tol` of `rgb` in every channel (and opaque).
pub fn near(image: &Image, x: u32, y: u32, rgb: [u8; 3], tol: u8) -> bool {
    let p = pixel(image, x, y);
    p[3] == 255 && p.iter().zip(rgb.iter()).all(|(a, b)| a.abs_diff(*b) <= tol)
}

/// Floors a stage coordinate to the pixel that contains it.
pub fn px(v: f64) -> u32 {
    v.floor().max(0.0) as u32
}

/// Straight-alpha RGBA bytes of an image.
fn straight(image: &Image) -> Vec<u8> {
    let mut out = image.premultiplied_rgba.clone();
    for p in out.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

/// Writes `image` as a PNG to `path`.
pub fn write_png(image: &Image, path: &std::path::Path) {
    let _ = anim_testkit::write_png(path, image.width, image.height, &straight(image));
}

/// Saves a render for inspection under the scratch directory.
pub fn save(image: &Image, name: &str) {
    write_png(image, &std::path::Path::new("/mnt/data/anim/scratch/visual/anim-timeline").join(format!("{name}.png")));
}

/// Crops `w` x `h` at (`x`, `y`) and scales it `k` times with nearest neighbour.
pub fn zoom(image: &Image, x: u32, y: u32, w: u32, h: u32, k: u32) -> Image {
    let mut out = Vec::new();
    for oy in 0..h * k {
        for ox in 0..w * k {
            let (sx, sy) = ((x + ox / k).min(image.width - 1), (y + oy / k).min(image.height - 1));
            out.extend_from_slice(&pixel(image, sx, sy));
        }
    }
    Image { width: w * k, height: h * k, premultiplied_rgba: out }
}

/// Lays images out in a grid (cell size = the largest), with a grey gutter, each scaled by `k`.
pub fn sheet(images: &[Image], cols: usize, k: u32) -> Image {
    let gap = 4u32;
    let cw = images.iter().map(|i| i.width).max().unwrap_or(1) * k;
    let ch = images.iter().map(|i| i.height).max().unwrap_or(1) * k;
    let rows = images.len().div_ceil(cols) as u32;
    let (w, h) = (cols as u32 * (cw + gap) + gap, rows * (ch + gap) + gap);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for p in buf.chunks_exact_mut(4) {
        p.copy_from_slice(&[150, 150, 160, 255]);
    }
    for (n, img) in images.iter().enumerate() {
        let (ox, oy) = ((n % cols) as u32 * (cw + gap) + gap, (n / cols) as u32 * (ch + gap) + gap);
        let big = zoom(img, 0, 0, img.width, img.height, k);
        for y in 0..big.height {
            for x in 0..big.width {
                let i = (((oy + y) * w + ox + x) * 4) as usize;
                buf[i..i + 4].copy_from_slice(&pixel(&big, x, y));
            }
        }
    }
    Image { width: w, height: h, premultiplied_rgba: buf }
}

/// Compares `image` with the committed golden `tests/fixtures/visual/<name>.png`. A missing golden is written (and
/// the test passes) so a new fixture can be blessed after inspection; a differing one fails.
pub fn golden(image: &Image, name: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/visual").join(format!("{name}.png"));
    let Ok(file) = std::fs::File::open(&path) else {
        write_png(image, &path);
        return;
    };
    drop(file);
    let golden = anim_testkit::read_png(&path).unwrap();
    assert_eq!((golden.width, golden.height), (image.width, image.height), "{name}: golden size");
    let want = straight(image);
    let bad = golden.rgba.chunks_exact(4).zip(want.chunks_exact(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 6)).count();
    assert!(bad * 500 <= (image.width * image.height) as usize, "{name}: {bad} pixels differ from the golden");
}
