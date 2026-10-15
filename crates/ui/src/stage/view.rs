//! Pure geometry of the stage view: the stage-to-screen mapping, zoom maths, ruler ticks and transform handles.

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

/// Smallest zoom factor the stage view allows.
pub const MIN_ZOOM: f32 = 0.02;
/// Largest zoom factor the stage view allows.
pub const MAX_ZOOM: f32 = 64.0;

/// Pan and zoom: the whole mapping between stage coordinates (document pixels, y down) and the screen.
///
/// `pan` is the screen offset of the stage origin from the top-left corner of the canvas rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StageView {
    pub zoom: f32,
    pub pan: Vec2,
}

impl Default for StageView {
    fn default() -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO }
    }
}

impl StageView {
    /// Replaces non-finite values and clamps the zoom into [`MIN_ZOOM`], [`MAX_ZOOM`].
    #[must_use]
    pub const fn sanitized(self) -> Self {
        let zoom = if self.zoom.is_finite() { self.zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { 1.0 };
        let pan = if self.pan.x.is_finite() && self.pan.y.is_finite() { self.pan } else { Vec2::ZERO };
        Self { zoom, pan }
    }

    /// Stage point to screen point, for a canvas whose top-left corner is `canvas.min`.
    #[must_use]
    pub fn to_screen(&self, canvas: Rect, p: Pos2) -> Pos2 {
        canvas.min + self.pan + p.to_vec2() * self.zoom
    }

    /// Screen point to stage point.
    #[must_use]
    pub fn to_stage(&self, canvas: Rect, p: Pos2) -> Pos2 {
        ((p - canvas.min - self.pan) / self.zoom).to_pos2()
    }

    /// Stage rectangle to screen rectangle.
    #[must_use]
    pub fn rect_to_screen(&self, canvas: Rect, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(canvas, r.min), self.to_screen(canvas, r.max))
    }

    /// The visible part of the stage, in stage coordinates.
    #[must_use]
    pub fn visible_stage_rect(&self, canvas: Rect) -> Rect {
        Rect::from_min_max(self.to_stage(canvas, canvas.min), self.to_stage(canvas, canvas.max))
    }

    /// Zooms by `factor` keeping the stage point under `anchor` (a screen position) fixed.
    #[must_use]
    pub fn zoom_about(&self, canvas: Rect, anchor: Pos2, factor: f32) -> Self {
        if !factor.is_finite() || factor <= 0.0 {
            return *self;
        }
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let applied = zoom / self.zoom;
        let rel = anchor - canvas.min;
        Self { zoom, pan: rel - (rel - self.pan) * applied }.sanitized()
    }

    /// Zooms to an absolute factor about the canvas centre.
    #[must_use]
    pub fn with_zoom_centered(&self, canvas: Rect, zoom: f32) -> Self {
        self.zoom_about(canvas, canvas.center(), zoom / self.zoom)
    }

    /// Fits a document of `doc` pixels into a canvas of `canvas_size`, centred, leaving `margin` on every side.
    #[must_use]
    pub fn fit(canvas_size: Vec2, doc: Vec2, margin: f32) -> Self {
        if doc.x <= 0.0 || doc.y <= 0.0 || canvas_size.x <= 0.0 || canvas_size.y <= 0.0 {
            return Self::default();
        }
        let avail = vec2((2.0f32.mul_add(-margin, canvas_size.x)).max(1.0), (2.0f32.mul_add(-margin, canvas_size.y)).max(1.0));
        let zoom = (avail.x / doc.x).min(avail.y / doc.y).clamp(MIN_ZOOM, MAX_ZOOM);
        Self { zoom, pan: (canvas_size - doc * zoom) * 0.5 }.sanitized()
    }

    /// 100 % zoom with the document centred.
    #[must_use]
    pub fn actual_size(canvas_size: Vec2, doc: Vec2) -> Self {
        Self { zoom: 1.0, pan: (canvas_size - doc) * 0.5 }.sanitized()
    }

    /// Moves the view by a screen-space delta.
    #[must_use]
    pub fn panned(self, delta: Vec2) -> Self {
        Self { zoom: self.zoom, pan: self.pan + delta }.sanitized()
    }
}

/// One tick on a ruler.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RulerTick {
    /// Stage coordinate of the tick.
    pub value: f32,
    /// Major ticks carry a label.
    pub major: bool,
}

/// Maximum number of ticks produced for one ruler, whatever the zoom.
pub const MAX_TICKS: usize = 4000;

/// The 1-2-5 step (in stage units) whose on-screen length is at least `min_px` at `zoom`.
#[must_use]
pub fn nice_step(zoom: f32, min_px: f32) -> f32 {
    let target = (min_px / zoom.max(f32::EPSILON)).max(f32::MIN_POSITIVE);
    let exp = target.log10().floor();
    let base = 10.0f32.powf(exp);
    for m in [1.0, 2.0, 5.0, 10.0] {
        if base * m >= target {
            return base * m;
        }
    }
    base * 10.0
}

/// Ticks for a ruler covering stage values `start..=end` at `zoom`.
#[must_use]
pub fn ruler_ticks(start: f32, end: f32, zoom: f32) -> Vec<RulerTick> {
    if !(start.is_finite() && end.is_finite() && zoom.is_finite()) || end < start || zoom <= 0.0 {
        return Vec::new();
    }
    let major = nice_step(zoom, 64.0);
    // 1 -> 5 minor ticks per major, 2 -> 2, 5 -> 5.
    let mantissa = major / 10.0f32.powf(major.log10().floor());
    let divisions = if (mantissa - 2.0).abs() < 0.5 { 2.0 } else { 5.0 };
    let minor = major / divisions;
    let first = (start / minor).floor();
    let mut out = Vec::new();
    let mut i = first;
    while out.len() < MAX_TICKS {
        let value = i * minor;
        if value > end {
            break;
        }
        let ratio = value / major;
        let is_major = (ratio - ratio.round()).abs() < 1e-3;
        out.push(RulerTick { value, major: is_major });
        i += 1.0;
    }
    out
}

/// Which handle of a transform box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandleId {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    /// The rotation knob above the top edge.
    Rotate,
}

/// The eight resize handles and the rotation knob of a box, in stage coordinates.
///
/// `rotation` is in radians, clockwise on screen (y down), about the centre of `rect`. `rotate_offset` is the
/// distance of the rotation knob above the top edge, in stage units.
#[must_use]
pub fn transform_handles(rect: Rect, rotation: f32, rotate_offset: f32) -> [(HandleId, Pos2); 9] {
    let c = rect.center();
    let (sin, cos) = rotation.sin_cos();
    let rot = |p: Pos2| {
        let d = p - c;
        pos2(d.x.mul_add(cos, (-d.y).mul_add(sin, c.x)), d.x.mul_add(sin, d.y.mul_add(cos, c.y)))
    };
    [
        (HandleId::TopLeft, rot(rect.left_top())),
        (HandleId::Top, rot(rect.center_top())),
        (HandleId::TopRight, rot(rect.right_top())),
        (HandleId::Right, rot(rect.right_center())),
        (HandleId::BottomRight, rot(rect.right_bottom())),
        (HandleId::Bottom, rot(rect.center_bottom())),
        (HandleId::BottomLeft, rot(rect.left_bottom())),
        (HandleId::Left, rot(rect.left_center())),
        (HandleId::Rotate, rot(pos2(rect.center().x, rect.top() - rotate_offset))),
    ]
}

/// The handle under `pos` (stage coordinates) within `tolerance` stage units, preferring corners, then edges.
#[must_use]
pub fn hit_transform_handle(rect: Rect, rotation: f32, rotate_offset: f32, pos: Pos2, tolerance: f32) -> Option<HandleId> {
    let handles = transform_handles(rect, rotation, rotate_offset);
    let mut best: Option<(HandleId, f32)> = None;
    for (id, p) in handles {
        let d = p.distance(pos);
        if d <= tolerance && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((id, d));
        }
    }
    best.map(|(id, _)| id)
}

/// Snaps `v` to the nearest multiple of `step` when within `threshold`; otherwise returns it unchanged.
#[must_use]
pub fn snap_to_grid(v: f32, step: f32, threshold: f32) -> f32 {
    if step <= 0.0 || !step.is_finite() {
        return v;
    }
    let snapped = (v / step).round() * step;
    if (snapped - v).abs() <= threshold { snapped } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0))
    }

    #[test]
    fn stage_screen_round_trip() {
        let v = StageView { zoom: 2.5, pan: vec2(30.0, -12.0) };
        let p = pos2(123.0, 45.0);
        let back = v.to_stage(canvas(), v.to_screen(canvas(), p));
        assert!((back - p).length() < 1e-3);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let v = StageView { zoom: 1.0, pan: vec2(40.0, 40.0) };
        let anchor = pos2(400.0, 300.0);
        let before = v.to_stage(canvas(), anchor);
        let z = v.zoom_about(canvas(), anchor, 3.0);
        assert!((z.zoom - 3.0).abs() < 1e-5);
        assert!((z.to_stage(canvas(), anchor) - before).length() < 1e-3);
    }

    #[test]
    fn zoom_is_clamped_and_ignores_garbage() {
        let v = StageView::default();
        assert!((v.zoom_about(canvas(), canvas().center(), 1e9).zoom - MAX_ZOOM).abs() < 1e-3);
        assert!((v.zoom_about(canvas(), canvas().center(), 1e-9).zoom - MIN_ZOOM).abs() < 1e-5);
        assert_eq!(v.zoom_about(canvas(), canvas().center(), f32::NAN), v);
        assert_eq!(v.zoom_about(canvas(), canvas().center(), -2.0), v);
    }

    #[test]
    fn fit_centres_document() {
        let v = StageView::fit(vec2(800.0, 600.0), vec2(1920.0, 1080.0), 20.0);
        let doc = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0));
        let on_screen = v.rect_to_screen(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)), doc);
        assert!((on_screen.center() - pos2(400.0, 300.0)).length() < 1e-2);
        assert!((on_screen.width() - 760.0).abs() < 1e-2);
        assert_eq!(StageView::fit(Vec2::ZERO, vec2(10.0, 10.0), 0.0), StageView::default());
    }

    #[test]
    fn nice_steps_follow_1_2_5() {
        assert!((nice_step(1.0, 64.0) - 100.0).abs() < 1e-3);
        assert!((nice_step(4.0, 64.0) - 20.0).abs() < 1e-3);
        assert!((nice_step(0.5, 64.0) - 200.0).abs() < 1e-3);
        assert!((nice_step(100.0, 64.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn ruler_ticks_cover_range_and_mark_majors() {
        let ticks = ruler_ticks(-50.0, 450.0, 1.0);
        assert!(ticks.iter().any(|t| t.major && t.value.abs() < 1e-3));
        assert!(ticks.iter().any(|t| t.major && (t.value - 100.0).abs() < 1e-3));
        assert!(ticks.iter().any(|t| !t.major));
        assert!(ticks.iter().all(|t| t.value >= -50.0 - 20.0 && t.value <= 450.0));
    }

    #[test]
    fn ruler_ticks_are_bounded() {
        assert!(ruler_ticks(0.0, 1.0e12, 64.0).len() <= MAX_TICKS);
        assert_eq!(ruler_ticks(f32::NAN, 1.0, 1.0), vec![]);
        assert_eq!(ruler_ticks(5.0, 1.0, 1.0), vec![]);
    }

    #[test]
    fn handles_rotate_with_the_box() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 50.0));
        let straight = transform_handles(r, 0.0, 20.0);
        assert_eq!(straight.first().map(|h| h.1), Some(pos2(0.0, 0.0)));
        assert_eq!(straight.get(8).map(|h| h.1), Some(pos2(50.0, -20.0)));
        let quarter = transform_handles(r, std::f32::consts::FRAC_PI_2, 20.0);
        // The top-left corner swings to the top-right of the rotated box.
        let tl = quarter.first().map_or(Pos2::ZERO, |h| h.1);
        assert!((tl - pos2(75.0, -25.0)).length() < 1e-3, "{tl:?}");
    }

    #[test]
    fn handle_hit_testing_prefers_nearest() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        assert_eq!(hit_transform_handle(r, 0.0, 20.0, pos2(2.0, 1.0), 6.0), Some(HandleId::TopLeft));
        assert_eq!(hit_transform_handle(r, 0.0, 20.0, pos2(50.0, 99.0), 6.0), Some(HandleId::Bottom));
        assert_eq!(hit_transform_handle(r, 0.0, 20.0, pos2(50.0, -19.0), 6.0), Some(HandleId::Rotate));
        assert_eq!(hit_transform_handle(r, 0.0, 20.0, pos2(50.0, 50.0), 6.0), None);
    }

    #[test]
    fn snapping() {
        assert!((snap_to_grid(19.0, 10.0, 3.0) - 20.0).abs() < 1e-5);
        assert!((snap_to_grid(16.0, 10.0, 3.0) - 16.0).abs() < 1e-5);
        assert!((snap_to_grid(5.0, 0.0, 3.0) - 5.0).abs() < 1e-5);
    }
}
