//! Shared scene builders for the export tests.
#![allow(dead_code, reason = "each test binary uses a different subset")]

use anim_core::{Affine, BezPath, BlendMode, ColorTransform, Point, Rgba8, Size};
use anim_display::{
    Bitmap, Cap, DisplayList, Fill, FillRule, Filter, Gradient, GradientStop, Group, Image, ImageStore, Interpolation, Join, Mesh, Node, Paint, Spread, Stroke,
    StrokeScale, StrokeStyle,
};
use anim_export::VecFrameSource;

pub fn rect_path(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x, y));
    p.line_to((x + w, y));
    p.line_to((x + w, y + h));
    p.line_to((x, y + h));
    p.close_path();
    p
}

pub fn solid_rect(x: f64, y: f64, w: f64, h: f64, color: Rgba8) -> Node {
    Node::Fill(Fill { transform: Affine::IDENTITY, path: rect_path(x, y, w, h), rule: FillRule::NonZero, paint: Paint::Solid(color) })
}

pub const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
pub const GREEN: Rgba8 = Rgba8::opaque(0, 255, 0);
pub const BLUE: Rgba8 = Rgba8::opaque(0, 0, 255);
pub const WHITE: Rgba8 = Rgba8::WHITE;

/// `count` frames of a 64x48 stage with a white background and a red 16x16 square moving right.
pub fn moving_square(count: u32) -> VecFrameSource {
    let size = Size::new(64.0, 48.0);
    let frames = (0..count)
        .map(|i| {
            let mut list = DisplayList::new(size, Some(WHITE));
            list.nodes.push(solid_rect(f64::from(i) * 8.0, 16.0, 16.0, 16.0, RED));
            list
        })
        .collect();
    VecFrameSource::new(size, 24.0, frames, ImageStore::new())
}

pub fn gradient(stops: &[(f32, Rgba8)], spread: Spread) -> Gradient {
    Gradient {
        transform: Affine::scale(0.05),
        stops: stops.iter().map(|&(offset, color)| GradientStop { offset, color }).collect(),
        spread,
        interpolation: Interpolation::Rgb,
        focal: 0.0,
    }
}

pub fn tiny_image(store: &mut ImageStore) -> anim_display::ImageId {
    // 2x2 premultiplied: red, green / blue, half-transparent white.
    let px = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 128, 128];
    store.insert(Image::new(2, 2, px).unwrap())
}

/// A scene that touches every display-list feature the SVG exporter supports.
pub fn kitchen_sink() -> (DisplayList, ImageStore) {
    let mut images = ImageStore::new();
    let img = tiny_image(&mut images);
    let size = Size::new(200.0, 120.0);
    let mut list = DisplayList::new(size, Some(Rgba8::new(10, 20, 30, 255)));

    list.nodes.push(Node::Fill(Fill {
        transform: Affine::translate((5.0, 5.0)),
        path: rect_path(0.0, 0.0, 40.0, 20.0),
        rule: FillRule::EvenOdd,
        paint: Paint::Linear(gradient(&[(0.0, RED), (0.5, GREEN), (1.0, Rgba8::new(0, 0, 255, 128))], Spread::Reflect)),
    }));
    list.nodes.push(Node::Fill(Fill {
        transform: Affine::translate((50.0, 5.0)),
        path: rect_path(0.0, 0.0, 40.0, 20.0),
        rule: FillRule::NonZero,
        paint: Paint::Radial(Gradient { focal: 0.4, ..gradient(&[(0.0, WHITE), (1.0, BLUE)], Spread::Pad) }),
    }));
    list.nodes.push(Node::Fill(Fill {
        transform: Affine::translate((100.0, 5.0)),
        path: rect_path(0.0, 0.0, 20.0, 20.0),
        rule: FillRule::NonZero,
        paint: Paint::Bitmap { image: img, transform: Affine::scale(10.0), repeat: true, smoothing: false },
    }));
    list.nodes.push(Node::Stroke(Stroke {
        transform: Affine::IDENTITY,
        path: rect_path(130.0, 5.0, 30.0, 20.0),
        style: StrokeStyle { width: 3.0, cap: Cap::Square, join: Join::Miter, miter_limit: 4.0, scale: StrokeScale::None, dashes: vec![4.0, 2.0] },
        paint: Paint::Solid(Rgba8::new(255, 200, 0, 200)),
    }));
    list.nodes.push(Node::Bitmap(Bitmap { transform: Affine::translate((170.0, 5.0)) * Affine::scale(10.0), image: img, smoothing: true }));

    // Group with transform, blend, colour transform, filters and a mask.
    list.nodes.push(Node::Group(Group {
        transform: Affine::translate((10.0, 40.0)) * Affine::rotate(0.2),
        color: ColorTransform::tint(Rgba8::new(255, 0, 255, 255), 0.3),
        blend: BlendMode::Multiply,
        mask: Some(vec![solid_rect(0.0, 0.0, 60.0, 30.0, WHITE)]),
        filters: vec![
            Filter::Blur { x: 2.0, y: 2.0, quality: 2 },
            Filter::DropShadow {
                color: Rgba8::new(0, 0, 0, 160),
                angle_deg: 45.0,
                distance: 4.0,
                blur_x: 4.0,
                blur_y: 4.0,
                strength: 1.5,
                inner: false,
                knockout: false,
                hide_object: false,
            },
            Filter::Glow { color: Rgba8::new(255, 255, 0, 255), blur_x: 3.0, blur_y: 3.0, strength: 1.0, inner: true, knockout: false },
            Filter::ColorMatrix([1.0, 0.0, 0.0, 0.0, 10.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
        ],
        children: vec![solid_rect(0.0, 0.0, 80.0, 40.0, GREEN)],
        key: Some(7),
    }));
    // Alpha-only group.
    list.nodes.push(Node::Group(Group {
        transform: Affine::translate((100.0, 40.0)),
        color: ColorTransform::alpha(0.5),
        children: vec![solid_rect(0.0, 0.0, 30.0, 30.0, RED)],
        ..Group::default()
    }));
    // Mirrored (right-to-left layout) group.
    list.nodes.push(Node::Group(Group {
        transform: Affine::new([-1.0, 0.0, 0.0, 1.0, 190.0, 40.0]),
        blend: BlendMode::Layer,
        children: vec![solid_rect(0.0, 0.0, 30.0, 10.0, BLUE)],
        ..Group::default()
    }));
    // Mesh.
    list.nodes.push(Node::Mesh(Mesh {
        transform: Affine::translate((10.0, 80.0)),
        content: vec![solid_rect(0.0, 0.0, 20.0, 20.0, RED), solid_rect(10.0, 0.0, 10.0, 20.0, BLUE)],
        source: vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0), Point::new(0.0, 20.0), Point::new(20.0, 20.0)],
        target: vec![Point::new(0.0, 0.0), Point::new(30.0, 5.0), Point::new(0.0, 25.0), Point::new(35.0, 30.0)],
        triangles: vec![[0, 1, 2], [1, 3, 2]],
    }));
    (list, images)
}
