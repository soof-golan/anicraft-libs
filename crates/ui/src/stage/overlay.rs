//! Overlay primitives tools hand to the stage view: selection boxes, transform handles, paths, marquees.
//!
//! All geometry is in stage coordinates; line widths and handle sizes are in screen points so they stay constant
//! while zooming.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, epaint::CubicBezierShape, pos2, vec2};

use super::view::{HandleId, StageView, transform_handles};

/// How an outline is drawn.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OverlayStyle {
    pub color: Color32,
    pub width: f32,
    pub dashed: bool,
    pub fill: Option<Color32>,
}

impl OverlayStyle {
    /// A solid one-point line in `color`.
    #[must_use]
    pub const fn line(color: Color32) -> Self {
        Self { color, width: 1.0, dashed: false, fill: None }
    }

    /// A dashed one-point line in `color`.
    #[must_use]
    pub const fn dashed(color: Color32) -> Self {
        Self { color, width: 1.0, dashed: true, fill: None }
    }

    /// The same style with a translucent fill.
    #[must_use]
    pub const fn filled(mut self, fill: Color32) -> Self {
        self.fill = Some(fill);
        self
    }
}

/// Shape of a point marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MarkerShape {
    Square,
    Circle,
    Diamond,
}

/// One bezier path node: its position and optional control points (stage coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PathNode {
    pub pos: Pos2,
    pub control_in: Option<Pos2>,
    pub control_out: Option<Pos2>,
    pub selected: bool,
}

impl PathNode {
    /// A corner node without control points.
    #[must_use]
    pub const fn corner(pos: Pos2) -> Self {
        Self { pos, control_in: None, control_out: None, selected: false }
    }
}

/// A thing drawn over the stage.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Overlay {
    /// An axis-aligned rectangle.
    Rect { rect: Rect, style: OverlayStyle },
    /// A straight line.
    Line { a: Pos2, b: Pos2, style: OverlayStyle },
    /// A polyline or polygon.
    Polygon { points: Vec<Pos2>, closed: bool, style: OverlayStyle },
    /// A bezier path with node and control-point markers.
    Path { nodes: Vec<PathNode>, closed: bool, style: OverlayStyle, show_controls: bool },
    /// A selection box with optional resize and rotate handles and a pivot.
    TransformBox { rect: Rect, rotation: f32, handles: bool, pivot: Option<Pos2>, color: Color32 },
    /// A rubber-band selection rectangle.
    Marquee { rect: Rect },
    /// A single marker at a point.
    Marker { pos: Pos2, shape: MarkerShape, color: Color32, filled: bool },
    /// A text label anchored at a point.
    Text { pos: Pos2, text: String, color: Color32 },
    /// A cross-hair at a point (registration point, pivot).
    Crosshair { pos: Pos2, color: Color32 },
}

/// Handle size on screen, in points.
pub const HANDLE_SIZE: f32 = 8.0;
/// Distance of the rotation knob above the box, on screen, in points.
pub const ROTATE_OFFSET_PX: f32 = 22.0;

fn dashed_or_solid(painter: &Painter, points: Vec<Pos2>, closed: bool, style: OverlayStyle) {
    let stroke = Stroke::new(style.width, style.color);
    if style.dashed {
        let mut pts = points;
        if closed && let Some(first) = pts.first().copied() {
            pts.push(first);
        }
        painter.extend(Shape::dashed_line(&pts, stroke, 5.0, 3.0));
    } else if closed {
        painter.add(Shape::closed_line(points, stroke));
    } else {
        painter.add(Shape::line(points, stroke));
    }
}

fn marker(painter: &Painter, pos: Pos2, shape: MarkerShape, color: Color32, filled: bool, backdrop: Color32) {
    let half = HANDLE_SIZE / 2.0;
    let fill = if filled { color } else { backdrop };
    let stroke = Stroke::new(1.0, color);
    match shape {
        MarkerShape::Square => {
            let r = Rect::from_center_size(pos, vec2(HANDLE_SIZE, HANDLE_SIZE));
            painter.rect(r, 0.0, fill, stroke, StrokeKind::Middle);
        }
        MarkerShape::Circle => {
            painter.circle(pos, half, fill, stroke);
        }
        MarkerShape::Diamond => {
            let pts = vec![pos + vec2(0.0, -half - 1.0), pos + vec2(half + 1.0, 0.0), pos + vec2(0.0, half + 1.0), pos + vec2(-half - 1.0, 0.0)];
            painter.add(Shape::convex_polygon(pts, fill, stroke));
        }
    }
}

/// What every overlay painter needs: where to draw and how stage coordinates map to the screen.
#[derive(Clone, Copy)]
struct Scene<'a> {
    painter: &'a Painter,
    view: &'a StageView,
    canvas: Rect,
    backdrop: Color32,
}

fn paint_path(scene: &Scene<'_>, nodes: &[PathNode], closed: bool, style: OverlayStyle, show_controls: bool) {
    let Scene { painter, view, canvas, backdrop } = *scene;
    let stroke = Stroke::new(style.width, style.color);
    let to_s = |p: Pos2| view.to_screen(canvas, p);
    let segments = if closed { nodes.len() } else { nodes.len().saturating_sub(1) };
    for i in 0..segments {
        let (Some(a), Some(b)) = (nodes.get(i), nodes.get(i.saturating_add(1)).or_else(|| nodes.first())) else { continue };
        let c1 = a.control_out.unwrap_or(a.pos);
        let c2 = b.control_in.unwrap_or(b.pos);
        painter.add(CubicBezierShape::from_points_stroke([to_s(a.pos), to_s(c1), to_s(c2), to_s(b.pos)], false, Color32::TRANSPARENT, stroke));
    }
    for node in nodes {
        if show_controls {
            for control in [node.control_in, node.control_out].into_iter().flatten() {
                painter.line_segment([to_s(node.pos), to_s(control)], Stroke::new(1.0, style.color.gamma_multiply(0.7)));
                marker(painter, to_s(control), MarkerShape::Circle, style.color, true, backdrop);
            }
        }
        marker(painter, to_s(node.pos), MarkerShape::Square, style.color, node.selected, backdrop);
    }
}

fn paint_transform_box(scene: &Scene<'_>, rect: Rect, rotation: f32, handles: bool, pivot: Option<Pos2>, color: Color32) {
    let Scene { painter, view, canvas, backdrop } = *scene;
    let offset_stage = ROTATE_OFFSET_PX / view.zoom;
    let all = transform_handles(rect, rotation, offset_stage);
    let corners: Vec<Pos2> = [HandleId::TopLeft, HandleId::TopRight, HandleId::BottomRight, HandleId::BottomLeft]
        .iter()
        .filter_map(|id| all.iter().find(|(h, _)| h == id).map(|(_, p)| view.to_screen(canvas, *p)))
        .collect();
    painter.add(Shape::closed_line(corners, Stroke::new(1.0, color)));
    if handles {
        let top = all.iter().find(|(h, _)| *h == HandleId::Top).map(|(_, p)| view.to_screen(canvas, *p));
        let knob = all.iter().find(|(h, _)| *h == HandleId::Rotate).map(|(_, p)| view.to_screen(canvas, *p));
        if let (Some(top), Some(knob)) = (top, knob) {
            painter.line_segment([top, knob], Stroke::new(1.0, color));
        }
        for (id, p) in &all {
            let shape = if *id == HandleId::Rotate { MarkerShape::Circle } else { MarkerShape::Square };
            marker(painter, view.to_screen(canvas, *p), shape, color, false, backdrop);
        }
    }
    if let Some(pivot) = pivot {
        let p = view.to_screen(canvas, pivot);
        painter.circle_stroke(p, 5.0, Stroke::new(1.0, color));
        painter.line_segment([p - vec2(8.0, 0.0), p + vec2(8.0, 0.0)], Stroke::new(1.0, color));
        painter.line_segment([p - vec2(0.0, 8.0), p + vec2(0.0, 8.0)], Stroke::new(1.0, color));
    }
}

/// Paints one overlay into `painter`, clipped by the painter's own clip rectangle.
pub fn paint_overlay(painter: &Painter, view: &StageView, canvas: Rect, overlay: &Overlay, backdrop: Color32) {
    let to_s = |p: Pos2| view.to_screen(canvas, p);
    let scene = Scene { painter, view, canvas, backdrop };
    match overlay {
        Overlay::Rect { rect, style } => {
            let r = view.rect_to_screen(canvas, *rect);
            if let Some(fill) = style.fill {
                painter.rect_filled(r, 0.0, fill);
            }
            let corners = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
            dashed_or_solid(painter, corners, true, *style);
        }
        Overlay::Line { a, b, style } => dashed_or_solid(painter, vec![to_s(*a), to_s(*b)], false, *style),
        Overlay::Polygon { points, closed, style } => {
            let pts: Vec<Pos2> = points.iter().map(|p| to_s(*p)).collect();
            if let (Some(fill), true) = (style.fill, *closed) {
                painter.add(Shape::convex_polygon(pts.clone(), fill, Stroke::NONE));
            }
            dashed_or_solid(painter, pts, *closed, *style);
        }
        Overlay::Path { nodes, closed, style, show_controls } => paint_path(&scene, nodes, *closed, *style, *show_controls),
        Overlay::TransformBox { rect, rotation, handles, pivot, color } => paint_transform_box(&scene, *rect, *rotation, *handles, *pivot, *color),
        Overlay::Marquee { rect } => {
            let r = view.rect_to_screen(canvas, *rect);
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(76, 154, 255, 36));
            dashed_or_solid(
                painter,
                vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()],
                true,
                OverlayStyle::dashed(Color32::from_rgb(76, 154, 255)),
            );
        }
        Overlay::Marker { pos, shape, color, filled } => marker(painter, to_s(*pos), *shape, *color, *filled, backdrop),
        Overlay::Text { pos, text, color } => {
            painter.text(to_s(*pos), Align2::LEFT_BOTTOM, crate::theme::visual(text), FontId::proportional(11.0), *color);
        }
        Overlay::Crosshair { pos, color } => {
            let p = to_s(*pos);
            let stroke = Stroke::new(1.0, *color);
            painter.line_segment([p - vec2(7.0, 0.0), p + vec2(7.0, 0.0)], stroke);
            painter.line_segment([p - vec2(0.0, 7.0), p + vec2(0.0, 7.0)], stroke);
            painter.circle_stroke(p, 3.0, stroke);
        }
    }
}

/// Convenience: the dashed selection outline a tool draws around a selected element's bounds.
#[must_use]
pub const fn selection_box(rect: Rect, color: Color32) -> Overlay {
    Overlay::Rect { rect, style: OverlayStyle::line(color) }
}

/// Convenience: a position helper for overlays built from integers.
#[must_use]
pub const fn at(x: f32, y: f32) -> Pos2 {
    pos2(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_overlay_kind_paints() {
        let ctx = egui::Context::default();
        let overlays = vec![
            selection_box(Rect::from_min_size(at(10.0, 10.0), vec2(50.0, 40.0)), Color32::BLUE),
            Overlay::Line { a: at(0.0, 0.0), b: at(40.0, 40.0), style: OverlayStyle::dashed(Color32::RED) },
            Overlay::Polygon {
                points: vec![at(0.0, 0.0), at(30.0, 0.0), at(15.0, 20.0)],
                closed: true,
                style: OverlayStyle::line(Color32::RED).filled(Color32::GREEN),
            },
            Overlay::Path {
                nodes: vec![
                    PathNode { pos: at(0.0, 0.0), control_in: None, control_out: Some(at(10.0, -10.0)), selected: true },
                    PathNode { pos: at(40.0, 0.0), control_in: Some(at(30.0, -10.0)), control_out: None, selected: false },
                ],
                closed: false,
                style: OverlayStyle::line(Color32::WHITE),
                show_controls: true,
            },
            Overlay::TransformBox {
                rect: Rect::from_min_size(at(20.0, 20.0), vec2(60.0, 30.0)),
                rotation: 0.3,
                handles: true,
                pivot: Some(at(50.0, 35.0)),
                color: Color32::BLUE,
            },
            Overlay::Marquee { rect: Rect::from_min_size(at(0.0, 0.0), vec2(10.0, 10.0)) },
            Overlay::Marker { pos: at(5.0, 5.0), shape: MarkerShape::Diamond, color: Color32::RED, filled: true },
            Overlay::Text { pos: at(5.0, 5.0), text: "שלום".into(), color: Color32::WHITE },
            Overlay::Crosshair { pos: at(5.0, 5.0), color: Color32::WHITE },
        ];
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let canvas = ui.available_rect_before_wrap();
            let view = StageView::default();
            for o in &overlays {
                paint_overlay(ui.painter(), &view, canvas, o, Color32::BLACK);
            }
        });
        out.textures_delta.clear();
        assert!(out.shapes.len() > overlays.len());
    }
}
