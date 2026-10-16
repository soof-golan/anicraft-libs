//! Scene-building and pixel-checking helpers shared by the golden tests.

#![allow(dead_code)]

use std::path::PathBuf;

use anim_core::kurbo::{Circle, Shape};
use anim_core::{Affine, BezPath, ColorTransform, Rgba8, Size};
use anim_display::{DisplayList, Fill, FillRule, Gradient, GradientStop, Group, Image, ImageStore, Interpolation, Node, Paint, Spread, Stroke, StrokeStyle};
use anim_render::{Background, RenderOptions, Renderer};

/// A display list plus the images it refers to.
pub struct Scene {
    pub list: DisplayList,
    pub images: ImageStore,
}

impl Scene {
    /// An empty scene of `width` x `height` on a white background.
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_background(width, height, Some(Rgba8::WHITE))
    }

    pub fn with_background(width: u32, height: u32, background: Option<Rgba8>) -> Self {
        Self { list: DisplayList::new(Size::new(f64::from(width), f64::from(height)), background), images: ImageStore::new() }
    }

    pub fn add(&mut self, node: Node) -> &mut Self {
        self.list.nodes.push(node);
        self
    }

    /// Renders at the stage size with default options.
    pub fn render(&self) -> Image {
        let options = RenderOptions::for_stage(self.list.size);
        self.render_with(&options)
    }

    pub fn render_with(&self, options: &RenderOptions) -> Image {
        anim_render::render(&self.list, &self.images, options).expect("render")
    }

    pub fn render_aliased(&self) -> Image {
        self.render_with(&RenderOptions::for_stage(self.list.size).with_antialias(false))
    }

    pub fn render_transparent(&self) -> Image {
        self.render_with(&RenderOptions::for_stage(self.list.size).with_background(Background::Transparent))
    }

    /// Renders with a fresh [`Renderer`] and returns its stats too.
    pub fn render_stats(&self) -> (Image, anim_render::RenderStats) {
        let mut renderer = Renderer::new();
        let image = renderer.render(&self.list, &self.images, &RenderOptions::for_stage(self.list.size)).expect("render");
        (image, renderer.stats())
    }
}

pub fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((x0, y0));
    path.line_to((x1, y0));
    path.line_to((x1, y1));
    path.line_to((x0, y1));
    path.close_path();
    path
}

pub fn circle_path(cx: f64, cy: f64, radius: f64) -> BezPath {
    Circle::new((cx, cy), radius).to_path(0.05)
}

pub fn line_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((x0, y0));
    path.line_to((x1, y1));
    path
}

pub fn fill(path: BezPath, paint: Paint) -> Node {
    Node::Fill(Fill { transform: Affine::IDENTITY, path, rule: FillRule::NonZero, paint })
}

pub fn solid_rect(x0: f64, y0: f64, x1: f64, y1: f64, color: Rgba8) -> Node {
    fill(rect_path(x0, y0, x1, y1), Paint::Solid(color))
}

pub fn solid_circle(cx: f64, cy: f64, radius: f64, color: Rgba8) -> Node {
    fill(circle_path(cx, cy, radius), Paint::Solid(color))
}

pub fn stroke(path: BezPath, style: StrokeStyle, color: Rgba8) -> Node {
    Node::Stroke(Stroke { transform: Affine::IDENTITY, path, style, paint: Paint::Solid(color) })
}

pub fn group(children: Vec<Node>) -> Group {
    Group { children, ..Group::default() }
}

pub fn with_color(children: Vec<Node>, color: ColorTransform) -> Node {
    Node::Group(Group { color, children, ..Group::default() })
}

pub const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
pub const GREEN: Rgba8 = Rgba8::opaque(0, 255, 0);
pub const BLUE: Rgba8 = Rgba8::opaque(0, 0, 255);
pub const BLACK: Rgba8 = Rgba8::BLACK;
pub const WHITE: Rgba8 = Rgba8::WHITE;

/// A gradient from `from` to `to` over `x0..x1` in the paint's local space (the standard Flash matrix).
pub fn gradient(x0: f64, x1: f64, from: Rgba8, to: Rgba8) -> Gradient {
    let extent = anim_display::GRADIENT_EXTENT * 2.0;
    Gradient {
        transform: Affine::translate((f64::midpoint(x0, x1), 0.0)) * Affine::scale_non_uniform((x1 - x0) / extent, 1.0),
        stops: vec![GradientStop { offset: 0.0, color: from }, GradientStop { offset: 1.0, color: to }],
        spread: Spread::Pad,
        interpolation: Interpolation::Rgb,
        focal: 0.0,
    }
}

pub fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    assert!(x < image.width && y < image.height, "pixel ({x}, {y}) outside {}x{}", image.width, image.height);
    let at = usize::try_from((y * image.width + x) * 4).unwrap();
    image.premultiplied_rgba[at..at + 4].try_into().unwrap()
}

/// Asserts that the pixel is within `tolerance` of `expected` on every channel.
#[track_caller]
pub fn assert_px(image: &Image, x: u32, y: u32, expected: [u8; 4], tolerance: u8) {
    let got = pixel(image, x, y);
    let close = got.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= tolerance);
    assert!(close, "pixel ({x}, {y}) is {got:?}, expected {expected:?} +-{tolerance}");
}

/// Number of pixels for which `test` holds.
pub fn count(image: &Image, test: impl Fn([u8; 4]) -> bool) -> usize {
    image.premultiplied_rgba.chunks_exact(4).filter(|px| test([px[0], px[1], px[2], px[3]])).count()
}

/// Largest per-channel difference between two images of the same size.
pub fn max_diff(a: &Image, b: &Image) -> u8 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    a.premultiplied_rgba.iter().zip(&b.premultiplied_rgba).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

/// Where test output goes: `$ANICRAFT_VISUAL_OUT/anim-render`, or nowhere when the variable is unset (CI).
pub fn scratch_dir() -> Option<PathBuf> {
    anim_testkit::out_dir("anim-render")
}

/// Writes `image` as a straight-alpha PNG named `name` (without extension) when `ANICRAFT_VISUAL_OUT` is set.
/// Failures are ignored: the PNGs are for people to look at, not part of the assertions.
pub fn save(name: &str, image: &Image) {
    let Some(dir) = scratch_dir() else { return };
    let _ = anim_testkit::write_png(&dir.join(format!("{name}.png")), image.width, image.height, &anim_render::straight_rgba(image));
}

/// A cheap deterministic pseudo-random sequence for hand-rolled fuzzing.
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        u32::try_from(self.0 >> 33).unwrap()
    }

    /// Uniform in `lo..hi`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * f64::from(self.next_u32()) / f64::from(u32::MAX >> 1)
    }
}
