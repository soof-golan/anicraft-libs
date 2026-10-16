//! Rendering and pixel inspection for the visual fixtures: draw a layout with its selection and caret overlays through
//! `anim-render`, probe pixels, crop and zoom, write PNGs.
#![allow(dead_code, reason = "each test file uses a different subset")]
#![allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::expect_used,
    clippy::panic,
    clippy::suboptimal_flops,
    clippy::many_single_char_names,
    reason = "picture helpers on small known-good fixture numbers; a failure is a failed test"
)]

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use anim_core::kurbo::Shape;
use anim_core::{Affine, Rect, Rgba8, Size};
use anim_display::{DisplayList, Fill, FillRule, ImageStore, Node, Paint};
use anim_render::{RenderOptions, Renderer, straight_rgba};
use anim_text::{Affinity, Caret, RichText, TextEngine, TextLayout};

/// Where visual output goes: `ANICRAFT_VISUAL_OUT`. `None` (unset) skips writing.
pub fn out_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("ANICRAFT_VISUAL_OUT")?).join("anim-text");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The fixture JSON under `tests/fixtures/visual`.
pub fn fixture(name: &str) -> RichText {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/visual").join(format!("{name}.json"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A straight-alpha RGBA picture.
#[derive(Debug, Clone)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Canvas {
    /// The pixel at (`x`, `y`), transparent outside.
    pub fn at(&self, x: i64, y: i64) -> [u8; 4] {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else { return [0; 4] };
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba.get(i..i + 4).map_or([0; 4], |s| [s[0], s[1], s[2], s[3]])
    }

    /// Number of pixels darker than mid grey inside the pixel rectangle.
    pub fn ink(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let [r, g, b, a] = self.at(x, y);
                if a > 0 && u32::from(r) + u32::from(g) + u32::from(b) < 3 * 150 {
                    n += 1;
                }
            }
        }
        n
    }

    /// The pixel rectangle `x0..x1` by `y0..y1` scaled up by `factor` with nearest-neighbour sampling.
    pub fn crop_zoom(&self, x0: i64, y0: i64, x1: i64, y1: i64, factor: u32) -> Self {
        let (w, h) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
        let (ow, oh) = (w * factor, h * factor);
        let mut rgba = Vec::with_capacity((ow * oh * 4) as usize);
        for oy in 0..oh {
            for ox in 0..ow {
                rgba.extend_from_slice(&self.at(x0 + i64::from(ox / factor), y0 + i64::from(oy / factor)));
            }
        }
        Self { width: ow, height: oh, rgba }
    }

    pub fn save(&self, path: &std::path::Path) {
        let file = BufWriter::new(File::create(path).expect("create png"));
        let mut encoder = png::Encoder::new(file, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut w| w.write_image_data(&self.rgba)).expect("write png");
    }
}

fn rect_node(rect: Rect, color: Rgba8) -> Node {
    Node::Fill(Fill { transform: Affine::IDENTITY, path: rect.to_path(0.1), rule: FillRule::NonZero, paint: Paint::Solid(color) })
}

/// What to draw over the text.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    pub selections: Vec<Rect>,
    /// Draw a thin caret under the text at every character boundary (teal downstream, orange upstream).
    pub all_carets: bool,
}

/// Pixels per layout unit in the images, and the margin around the layout in layout units.
pub const SCALE: f64 = 2.0;
pub const PAD: f64 = 10.0;

/// Renders `layout` on white at [`SCALE`], text origin at (`PAD`, `PAD`). Returns the canvas.
pub fn render(engine: &mut TextEngine, layout: &TextLayout, overlay: &Overlay) -> Canvas {
    let mut nodes = Vec::new();
    for rect in &overlay.selections {
        nodes.push(rect_node(*rect, Rgba8::new(70, 130, 240, 90)));
    }
    for line in &layout.lines {
        nodes.push(rect_node(Rect::new(layout.bounds.x0, line.baseline, layout.bounds.x1, line.baseline + 0.5), Rgba8::new(230, 120, 120, 120)));
    }
    nodes.extend(engine.to_display(layout));
    if overlay.all_carets {
        let text = layout.text();
        for offset in (0..=text.len()).filter(|&o| text.is_char_boundary(o)) {
            for (affinity, color) in [(Affinity::Downstream, Rgba8::new(0, 150, 140, 255)), (Affinity::Upstream, Rgba8::new(240, 150, 30, 255))] {
                if let Some(r) = layout.caret_rect(Caret { offset, affinity }, 0.5) {
                    nodes.push(rect_node(Rect::new(r.x0, r.y1 - r.height() * 0.25, r.x1, r.y1), color));
                }
            }
        }
    }
    // Ink that overflows the box on the left (a box narrower than the text, right to left) still shows in the picture.
    let overflow = layout.runs.iter().map(|r| r.origin.x).fold(0.0, f64::min);
    let group = Node::Group(anim_display::Group { transform: Affine::translate((PAD - overflow, PAD)), children: nodes, ..anim_display::Group::default() });
    let size = Size::new(2.0f64.mul_add(PAD, layout.bounds.x1.max(1.0) - overflow), 2.0f64.mul_add(PAD, layout.bounds.y1.max(1.0)));
    let list = DisplayList { size, background: Some(Rgba8::opaque(255, 255, 255)), nodes: vec![group] };
    let options = RenderOptions::scaled_stage(list.size, SCALE);
    let image = Renderer::new().render(&list, &ImageStore::new(), &options).expect("render");
    Canvas { width: image.width, height: image.height, rgba: straight_rgba(&image) }
}

/// Renders and, when an output folder exists, writes `<name>.png` there.
pub fn render_to_file(engine: &mut TextEngine, layout: &TextLayout, overlay: &Overlay, name: &str) -> Canvas {
    let canvas = render(engine, layout, overlay);
    if let Some(dir) = out_dir() {
        canvas.save(&dir.join(format!("{name}.png")));
    }
    canvas
}

/// The canvas pixel (x, y) of layout-space point (`lx`, `ly`).
pub fn px(lx: f64) -> i64 {
    ((lx + PAD) * SCALE).round() as i64
}
