//! Rectangle and oval primitives (`DOMRectangleObject`, `DOMOvalObject`): shapes that stay editable after drawing.
//!
//! A primitive keeps its parameters next to the geometry they generate. The shape lives at the origin of its own
//! space (the box spans `0,0` to `width,height`) and the element's matrix places it on the stage, so scaling it with
//! the Free Transform tool never changes the parameters.

use anim_core::kurbo::{Arc, Ellipse, PathEl, RoundedRect, RoundedRectRadii, Shape as _};
use anim_core::{BezPath, Rect, Vec2};
use anim_shape::{EdgeShape, FillStyle, PathInput, StrokeStyle};
use serde::{Deserialize, Serialize};

/// A rectangle with individually rounded corners.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RectPrimitive {
    pub width: f64,
    pub height: f64,
    /// Corner radii: top left, top right, bottom right, bottom left.
    pub radii: [f64; 4],
    /// Whether the corner radii are edited together (`lockFlag`).
    pub lock: bool,
}

/// An ellipse, arc, pie or ring segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OvalPrimitive {
    pub width: f64,
    pub height: f64,
    /// Where the arc starts, in degrees clockwise from three o'clock.
    pub start_angle: f64,
    /// Where the arc ends, in degrees.
    pub end_angle: f64,
    /// Radius of the hole as a percentage of the outer radius (0 to 99).
    pub inner_radius: f64,
    /// Whether an open arc is closed with a straight edge.
    pub close_path: bool,
}

/// The parameters of a primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrimitiveKind {
    Rectangle(RectPrimitive),
    Oval(OvalPrimitive),
}

/// A primitive: parameters plus the styles of the shape they generate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Primitive {
    pub kind: PrimitiveKind,
    pub fill: Option<FillStyle>,
    pub stroke: Option<StrokeStyle>,
}

/// The rectangle with the given corner radii (top left, top right, bottom right, bottom left), each limited to half
/// the shorter side.
#[must_use]
pub fn rounded_rect_path(rect: Rect, radii: [f64; 4]) -> BezPath {
    let limit = rect.width().min(rect.height()) / 2.0;
    let [tl, tr, br, bl] = radii.map(|r| if r.is_finite() { r.max(0.0).min(limit) } else { 0.0 });
    if tl + tr + br + bl > 0.0 { RoundedRect::from_rect(rect, RoundedRectRadii::new(tl, tr, br, bl)).to_path(0.1) } else { rect.to_path(0.1) }
}

/// An ellipse, or the arc, pie or ring segment the angles describe, inside `rect`.
#[must_use]
pub fn oval_path(rect: Rect, start_angle: f64, end_angle: f64, inner_radius: f64, close_path: bool) -> BezPath {
    let centre = rect.center();
    let radii = Vec2::new(rect.width() / 2.0, rect.height() / 2.0);
    if inner_radius <= 0.0 && (start_angle - end_angle).rem_euclid(360.0) < 1.0e-9 {
        return Ellipse::new(centre, radii, 0.0).to_path(0.1);
    }
    let start = start_angle.to_radians();
    let mut sweep = (end_angle - start_angle).rem_euclid(360.0).to_radians();
    if sweep <= 0.0 {
        sweep = std::f64::consts::TAU;
    }
    let inner = (inner_radius / 100.0).clamp(0.0, 0.99);
    let mut path = BezPath::new();
    for el in Arc::new(centre, radii, start, sweep, 0.0).path_elements(0.05) {
        path.push(el);
    }
    if inner > 0.0 {
        let mut tail = Arc::new(centre, radii * inner, start + sweep, -sweep, 0.0).path_elements(0.05);
        if let Some(PathEl::MoveTo(p)) = tail.next() {
            path.line_to(p);
        }
        for el in tail {
            path.push(el);
        }
        path.close_path();
    } else if close_path {
        path.line_to(centre);
        path.close_path();
    }
    path
}

impl PrimitiveKind {
    /// The box of the primitive at the origin of its own space.
    #[must_use]
    pub const fn size(&self) -> (f64, f64) {
        match self {
            Self::Rectangle(r) => (r.width, r.height),
            Self::Oval(o) => (o.width, o.height),
        }
    }

    /// The outline in the primitive's own space.
    #[must_use]
    pub fn path(&self) -> BezPath {
        let (w, h) = self.size();
        let rect = Rect::new(0.0, 0.0, w.max(0.0), h.max(0.0));
        match self {
            Self::Rectangle(r) => rounded_rect_path(rect, r.radii),
            Self::Oval(o) => oval_path(rect, o.start_angle, o.end_angle, o.inner_radius, o.close_path),
        }
    }
}

impl Primitive {
    /// The shape these parameters and styles generate, in the primitive's own space.
    #[must_use]
    pub fn build_shape(&self) -> EdgeShape {
        EdgeShape::from_paths(&[PathInput::new(self.kind.path(), self.fill.clone(), self.stroke.clone())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(radii: [f64; 4]) -> Primitive {
        Primitive {
            kind: PrimitiveKind::Rectangle(RectPrimitive { width: 40.0, height: 20.0, radii, lock: false }),
            fill: Some(FillStyle::Solid(anim_core::Rgba8::BLACK)),
            stroke: None,
        }
    }

    #[test]
    fn a_rectangle_primitive_builds_a_filled_shape() {
        let shape = rect([0.0; 4]).build_shape();
        assert!(!shape.is_empty());
        assert_eq!(shape.fills.len(), 1);
    }

    #[test]
    fn rounded_corners_change_the_outline() {
        let square = rect([0.0; 4]).kind.path().bounding_box();
        let round = rect([5.0, 0.0, 0.0, 0.0]).kind.path();
        assert_eq!(round.bounding_box(), square);
        assert!(round.elements().len() > 5);
    }

    #[test]
    fn an_open_arc_with_a_hole_is_a_ring_segment() {
        let kind = PrimitiveKind::Oval(OvalPrimitive { width: 20.0, height: 20.0, start_angle: 0.0, end_angle: 90.0, inner_radius: 50.0, close_path: true });
        let bbox = kind.path().bounding_box();
        assert!(bbox.width() < 20.0 && bbox.height() < 20.0);
    }
}
