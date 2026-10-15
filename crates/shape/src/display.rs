//! Edge shape to display-list nodes.
//!
//! The nodes are in the shape's pixel space (edges divided by 20); the caller wraps them in a group carrying the
//! `DOMShape` matrix. Fills come first, by style index, then strokes by style index, which is the order Animate
//! paints them in. Gradient matrices are passed through untouched: they already map gradient space into pixels.

use anim_core::{Affine, Rgba8};
use anim_display::{Fill, FillRule, Gradient, GradientStop, ImageId, Node, Paint, Stroke, StrokeScale, StrokeStyle as DisplayStroke};

use crate::edges::EdgeShape;
use crate::styles::{BITMAP_MATRIX_SCALE, FillStyle, GradientFill, SolidStroke};

/// What a bitmap fill that cannot be resolved is painted with: mid grey, so that the shape stays visible.
pub const MISSING_BITMAP_COLOR: Rgba8 = Rgba8::opaque(128, 128, 128);

/// Looks up the image behind a bitmap fill's library path.
pub trait ImageResolver {
    /// The image for the library item at `path`, or `None` when it is not loaded.
    fn resolve(&self, path: &str) -> Option<ImageId>;
}

impl<F: Fn(&str) -> Option<ImageId>> ImageResolver for F {
    fn resolve(&self, path: &str) -> Option<ImageId> {
        self(path)
    }
}

/// A resolver that knows no images.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoImages;

impl ImageResolver for NoImages {
    fn resolve(&self, _path: &str) -> Option<ImageId> {
        None
    }
}

impl EdgeShape {
    /// The paintable nodes of this shape: a [`Node::Fill`] (even-odd) per used fill style, then a [`Node::Stroke`]
    /// per used stroke style. Patterned strokes render as solid.
    #[must_use]
    pub fn to_display(&self, images: &dyn ImageResolver) -> Vec<Node> {
        let assembled = self.assemble();
        let mut nodes = Vec::with_capacity(assembled.fills.len().saturating_add(assembled.strokes.len()));
        for region in assembled.fills {
            let Some(style) = self.fill(region.style) else { continue };
            nodes.push(Node::Fill(Fill { transform: Affine::IDENTITY, path: region.path, rule: FillRule::EvenOdd, paint: paint_for(style, images) }));
        }
        for run in assembled.strokes {
            let Some(style) = self.stroke(run.style) else { continue };
            let solid = style.solid();
            nodes.push(Node::Stroke(Stroke {
                transform: Affine::IDENTITY,
                path: run.path,
                style: display_stroke(solid),
                paint: paint_for(&solid.fill, images),
            }));
        }
        nodes
    }
}

/// The display paint for a fill style.
#[must_use]
pub fn paint_for(fill: &FillStyle, images: &dyn ImageResolver) -> Paint {
    match fill {
        FillStyle::Solid(color) => Paint::Solid(*color),
        FillStyle::LinearGradient(g) => gradient_paint(g, Paint::Linear),
        FillStyle::RadialGradient(g) => gradient_paint(g, Paint::Radial),
        FillStyle::Bitmap(b) => images.resolve(&b.path).map_or(Paint::Solid(MISSING_BITMAP_COLOR), |image| Paint::Bitmap {
            image,
            transform: b.matrix * Affine::scale(BITMAP_MATRIX_SCALE),
            repeat: !b.clipped,
            smoothing: b.smoothing,
        }),
    }
}

fn gradient_paint(g: &GradientFill, wrap: fn(Gradient) -> Paint) -> Paint {
    let mut stops: Vec<GradientStop> = g.stops.iter().map(|s| GradientStop { offset: s.offset.clamp(0.0, 1.0), color: s.color }).collect();
    stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    match stops.as_slice() {
        [] => Paint::Solid(Rgba8::TRANSPARENT),
        [only] => Paint::Solid(only.color),
        _ => wrap(Gradient { transform: g.matrix, stops, spread: g.spread, interpolation: g.interpolation, focal: g.focal.clamp(-1.0, 1.0) }),
    }
}

/// The display stroke geometry for a solid stroke; a weight of 0 is a hairline that ignores scaling.
#[must_use]
pub fn display_stroke(stroke: &SolidStroke) -> DisplayStroke {
    let hairline = stroke.weight <= 0.0;
    DisplayStroke {
        width: stroke.weight.max(0.0),
        cap: stroke.caps,
        join: stroke.joints,
        miter_limit: stroke.miter_limit.max(1.0),
        scale: if hairline { StrokeScale::None } else { stroke.scale_mode },
        dashes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use anim_core::Point;
    use anim_display::{Cap, Interpolation, Join, Spread};

    use super::*;
    use crate::codec::parse_edges;
    use crate::edges::Edge;
    use crate::styles::{BitmapFill, StrokeStyle};

    fn square_shape(fill: FillStyle) -> EdgeShape {
        let segments = parse_edges("!0 0|200 0|200 200|0 200|0 0").unwrap();
        EdgeShape { fills: vec![fill], strokes: vec![StrokeStyle::plain(2.0, Rgba8::opaque(1, 2, 3))], edges: vec![Edge::new(0, 1, 1, segments)] }
    }

    fn gradient(stops: &[(f32, Rgba8)]) -> GradientFill {
        GradientFill {
            matrix: Affine::translate((5.0, 5.0)) * Affine::scale(0.01),
            stops: stops.iter().map(|(offset, color)| GradientStop { offset: *offset, color: *color }).collect(),
            spread: Spread::Reflect,
            interpolation: Interpolation::LinearRgb,
            focal: 0.5,
        }
    }

    #[test]
    fn fills_come_before_strokes_and_use_pixel_space() {
        let nodes = square_shape(FillStyle::Solid(Rgba8::opaque(255, 0, 0))).to_display(&NoImages);
        assert_eq!(nodes.len(), 2);
        let Some(Node::Fill(fill)) = nodes.first() else { panic!("fill first") };
        assert_eq!(fill.rule, FillRule::EvenOdd);
        assert_eq!(fill.paint, Paint::Solid(Rgba8::opaque(255, 0, 0)));
        assert_eq!(fill.transform, Affine::IDENTITY);
        let bounds = anim_core::kurbo::Shape::bounding_box(&fill.path);
        assert!((bounds.x1 - 10.0).abs() < 1e-9 && (bounds.y1 - 10.0).abs() < 1e-9);
        let Some(Node::Stroke(stroke)) = nodes.get(1) else { panic!("stroke second") };
        assert!((stroke.style.width - 2.0).abs() < 1e-12);
        assert_eq!(stroke.style.cap, Cap::Round);
        assert_eq!(stroke.style.join, Join::Round);
        assert_eq!(stroke.paint, Paint::Solid(Rgba8::opaque(1, 2, 3)));
    }

    #[test]
    fn gradients_map_to_display_gradients() {
        let g = gradient(&[(1.0, Rgba8::BLACK), (0.0, Rgba8::WHITE)]);
        let nodes = square_shape(FillStyle::LinearGradient(g.clone())).to_display(&NoImages);
        let Some(Node::Fill(fill)) = nodes.first() else { panic!("fill") };
        let Paint::Linear(out) = &fill.paint else { panic!("linear gradient") };
        assert_eq!(out.transform, g.matrix);
        assert_eq!(out.spread, Spread::Reflect);
        assert_eq!(out.interpolation, Interpolation::LinearRgb);
        assert!((out.focal - 0.5).abs() < 1e-12);
        let offsets: Vec<f32> = out.stops.iter().map(|s| s.offset).collect();
        assert_eq!(offsets, vec![0.0, 1.0]);

        let radial = square_shape(FillStyle::RadialGradient(g)).to_display(&NoImages);
        assert!(matches!(radial.first(), Some(Node::Fill(Fill { paint: Paint::Radial(_), .. }))));
    }

    #[test]
    fn degenerate_gradients_become_solid() {
        assert_eq!(gradient_paint(&gradient(&[]), Paint::Linear), Paint::Solid(Rgba8::TRANSPARENT));
        assert_eq!(gradient_paint(&gradient(&[(0.3, Rgba8::WHITE)]), Paint::Linear), Paint::Solid(Rgba8::WHITE));
    }

    #[test]
    fn bitmap_fills_resolve_or_fall_back() {
        let bitmap = FillStyle::Bitmap(BitmapFill {
            path: "dir/pic.png".into(),
            matrix: Affine::new([20.0, 0.0, 0.0, 20.0, 3.0, 4.0]),
            clipped: false,
            smoothing: true,
        });
        let mut known = BTreeMap::new();
        known.insert("dir/pic.png".to_owned(), ImageId(7));
        let resolver = |path: &str| known.get(path).copied();
        let nodes = square_shape(bitmap.clone()).to_display(&resolver);
        let Some(Node::Fill(fill)) = nodes.first() else { panic!("fill") };
        let Paint::Bitmap { image, transform, repeat, smoothing } = &fill.paint else { panic!("bitmap paint") };
        assert_eq!(*image, ImageId(7));
        assert!(*repeat && *smoothing);
        assert_eq!(*transform * Point::new(10.0, 10.0), Point::new(13.0, 14.0));

        let missing = square_shape(bitmap).to_display(&NoImages);
        assert!(matches!(missing.first(), Some(Node::Fill(Fill { paint: Paint::Solid(c), .. })) if *c == MISSING_BITMAP_COLOR));
    }

    #[test]
    fn hairlines_ignore_scaling_and_miter_limits_are_sane() {
        let hairline = display_stroke(&SolidStroke { weight: 0.0, scale_mode: StrokeScale::Normal, miter_limit: 0.0, ..SolidStroke::default() });
        assert_eq!(hairline.scale, StrokeScale::None);
        assert!(hairline.width.abs() < 1e-12);
        assert!(hairline.miter_limit >= 1.0);
        let normal = display_stroke(&SolidStroke { weight: 3.0, scale_mode: StrokeScale::Horizontal, ..SolidStroke::default() });
        assert_eq!(normal.scale, StrokeScale::Horizontal);
    }

    #[test]
    fn empty_shapes_produce_no_nodes() {
        assert_eq!(EdgeShape::default().to_display(&NoImages).len(), 0);
    }
}
