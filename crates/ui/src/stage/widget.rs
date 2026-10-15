//! The [`stage_view`] widget itself.

use egui::{Color32, CursorIcon, Id, Key, Modifiers, Painter, PointerButton, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, Vec2, pos2, vec2};

use super::{
    LibraryDrag,
    overlay::{Overlay, paint_overlay},
    view::{StageView, ruler_ticks},
};
use crate::theme::{Palette, current_palette};

/// Thickness of the rulers, in points.
pub const RULER_SIZE: f32 = 18.0;

/// A guide line's orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Axis {
    /// A horizontal line; `pos` is its y coordinate. Dragged out of the top ruler.
    Horizontal,
    /// A vertical line; `pos` is its x coordinate. Dragged out of the left ruler.
    Vertical,
}

/// A guide line in stage coordinates.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Guide {
    pub axis: Axis,
    pub pos: f32,
}

/// What the mouse wheel does over the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum WheelMode {
    /// Scroll the view (Animate's default); Ctrl/Cmd+wheel and pinch still zoom.
    #[default]
    Pan,
    /// Zoom about the pointer.
    Zoom,
}

/// Everything [`stage_view`] draws and reacts to.
#[derive(Debug, Clone)]
pub struct StageViewState {
    /// Distinguishes several stage views in one UI.
    pub id_salt: u64,
    /// Document size in stage pixels.
    pub doc_size: Vec2,
    pub view: StageView,
    /// Ask the widget to fit the document into the canvas; the app clears it when it applies the resulting
    /// [`StageEvent::ViewChanged`].
    pub fit_request: bool,
    /// Margin kept around the document when fitting, in points.
    pub fit_margin: f32,
    pub stage_color: Color32,
    pub show_rulers: bool,
    pub show_grid: bool,
    pub show_guides: bool,
    pub lock_guides: bool,
    pub grid_size: f32,
    pub guides: Vec<Guide>,
    pub overlays: Vec<Overlay>,
    pub wheel_mode: WheelMode,
    /// The hand tool is active: primary drags pan.
    pub pan_tool: bool,
    /// Cursor to show over the canvas (set by the active tool).
    pub cursor: Option<CursorIcon>,
    /// Draw the document edge.
    pub show_stage_border: bool,
}

impl StageViewState {
    /// A state for a document of `doc_size` pixels, fitted on the first frame.
    #[must_use]
    pub fn new(doc_size: Vec2) -> Self {
        Self {
            id_salt: 0,
            doc_size,
            view: StageView::default(),
            fit_request: true,
            fit_margin: 24.0,
            stage_color: Color32::WHITE,
            show_rulers: true,
            show_grid: false,
            show_guides: true,
            lock_guides: false,
            grid_size: 20.0,
            guides: Vec::new(),
            overlays: Vec::new(),
            wheel_mode: WheelMode::Pan,
            pan_tool: false,
            cursor: None,
            show_stage_border: true,
        }
    }

    /// The pixel size a texture should have to show the document 1:1 on screen at `pixels_per_point`.
    #[must_use]
    pub fn desired_texture_size(&self, pixels_per_point: f32) -> [u32; 2] {
        let scale = self.view.zoom * pixels_per_point;
        let clamp = |v: f32| crate::util::ceil_u32((v * scale).clamp(1.0, 8192.0));
        [clamp(self.doc_size.x), clamp(self.doc_size.y)]
    }
}

/// Phase of a pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase {
    /// Hovering, no button down.
    Move,
    /// A button went down and a drag started (the position is the press position).
    Down,
    /// The pointer moved with a button down.
    Drag,
    /// The button was released after a drag.
    Up,
    /// A press and release without a drag.
    Click,
    DoubleClick,
}

/// A pointer event in stage coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StagePointer {
    pub phase: PointerPhase,
    pub button: PointerButton,
    /// Stage position.
    pub pos: Pos2,
    /// Stage position where the press happened (equals `pos` for `Down`).
    pub origin: Pos2,
    /// Stage-space movement since the previous frame (for `Drag`).
    pub delta: Vec2,
    pub modifiers: Modifiers,
    /// Zoom at the time of the event, so tools can convert pixel tolerances to stage units.
    pub zoom: f32,
}

/// What [`stage_view`] reports.
#[derive(Debug, Clone, PartialEq)]
pub enum StageEvent {
    /// Pan or zoom changed (wheel, pinch, space-drag, fit): store the new view.
    ViewChanged(StageView),
    Pointer(StagePointer),
    /// A new guide was dropped on the stage.
    GuideAdded(Guide),
    /// An existing guide was moved to a new position.
    GuideMoved {
        index: usize,
        pos: f32,
    },
    /// An existing guide was dragged off the stage.
    GuideRemoved {
        index: usize,
    },
    /// A library item was dropped on the stage at this stage position.
    DropLibraryItem {
        id: u64,
        pos: Pos2,
    },
}

const BUTTONS: [PointerButton; 3] = [PointerButton::Primary, PointerButton::Secondary, PointerButton::Middle];

struct Frame {
    outer: Rect,
    canvas: Rect,
    view: StageView,
    palette: Palette,
}

impl Frame {
    fn doc_rect(&self, state: &StageViewState) -> Rect {
        self.view.rect_to_screen(self.canvas, Rect::from_min_size(Pos2::ZERO, state.doc_size))
    }
}

/// Draws the stage view and returns the events it produced this frame.
pub fn stage_view(ui: &mut Ui, state: &StageViewState, texture: Option<&TextureHandle>) -> Vec<StageEvent> {
    let palette = current_palette(ui.ctx());
    let (outer, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let ruler = if state.show_rulers { RULER_SIZE } else { 0.0 };
    let canvas = Rect::from_min_max(outer.min + vec2(ruler, ruler), outer.max);
    let id = Id::new(("anim-stage", state.id_salt));
    let mut events = Vec::new();
    let fitting = state.fit_request && canvas.width() > 1.0 && canvas.height() > 1.0;
    let mut view = if fitting { StageView::fit(canvas.size(), state.doc_size, state.fit_margin) } else { state.view.sanitized() };

    let response = ui.interact(canvas, id.with("canvas"), Sense::click_and_drag());
    let space_down = ui.input(|i| i.key_down(Key::Space));
    let panning = response.dragged_by(PointerButton::Middle) || (response.dragged_by(PointerButton::Primary) && (space_down || state.pan_tool));
    if panning {
        view = view.panned(response.drag_delta());
    }
    if response.hovered() || response.dragged() {
        view = apply_wheel(ui, state, canvas, view);
    }
    if view != state.view.sanitized() || state.fit_request {
        events.push(StageEvent::ViewChanged(view));
    }
    let frame = Frame { outer, canvas, view, palette };

    let guide_events = guide_interactions(ui, state, &frame, id);
    events.extend(guide_events.events);
    let ruler_events = ruler_interactions(ui, state, &frame, id);
    events.extend(ruler_events.events);

    if !panning {
        events.extend(pointer_events(ui, &response, &frame));
    }
    if let Some(payload) = response.dnd_release_payload::<LibraryDrag>()
        && let Some(p) = response.hover_pos().or_else(|| ui.input(|i| i.pointer.interact_pos()))
    {
        events.push(StageEvent::DropLibraryItem { id: payload.id, pos: view.to_stage(canvas, p) });
    }

    let cursor = if panning || (space_down && response.hovered()) {
        Some(if response.dragged() { CursorIcon::Grabbing } else { CursorIcon::Grab })
    } else if state.pan_tool {
        Some(CursorIcon::Grab)
    } else {
        state.cursor
    };
    if response.hovered()
        && let Some(cursor) = cursor
    {
        ui.ctx().set_cursor_icon(cursor);
    }

    if ui.is_rect_visible(outer) {
        let painter = ui.painter_at(outer);
        paint_canvas(&painter, state, &frame, texture, response.hover_pos(), &guide_events.preview, &ruler_events.preview);
    }
    events
}

fn apply_wheel(ui: &Ui, state: &StageViewState, canvas: Rect, mut view: StageView) -> StageView {
    let (zoom_delta, scroll, pointer) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
    let anchor = pointer.unwrap_or_else(|| canvas.center());
    if (zoom_delta - 1.0).abs() > f32::EPSILON {
        view = view.zoom_about(canvas, anchor, zoom_delta);
    } else if scroll != Vec2::ZERO {
        match state.wheel_mode {
            WheelMode::Pan => view = view.panned(scroll),
            WheelMode::Zoom => view = view.zoom_about(canvas, anchor, (scroll.y / 200.0).exp()),
        }
    }
    view
}

fn pointer_events(ui: &Ui, response: &egui::Response, frame: &Frame) -> Vec<StageEvent> {
    let mut out = Vec::new();
    let (modifiers, press_origin, hover, delta) = ui.input(|i| (i.modifiers, i.pointer.press_origin(), i.pointer.hover_pos(), i.pointer.delta()));
    let view = frame.view;
    let to_stage = |p: Pos2| view.to_stage(frame.canvas, p);
    let make = |phase: PointerPhase, button: PointerButton, pos: Pos2, origin: Pos2, d: Vec2| {
        StageEvent::Pointer(StagePointer { phase, button, pos: to_stage(pos), origin: to_stage(origin), delta: d / view.zoom, modifiers, zoom: view.zoom })
    };
    let current = response.interact_pointer_pos().or(hover);
    let any_down = BUTTONS.iter().any(|b| response.dragged_by(*b));
    if let (Some(pos), false, true) = (hover, any_down, delta != Vec2::ZERO)
        && response.hovered()
    {
        out.push(make(PointerPhase::Move, PointerButton::Primary, pos, pos, Vec2::ZERO));
    }
    for button in BUTTONS {
        let origin = press_origin.or(current);
        if response.drag_started_by(button)
            && let Some(origin) = origin
        {
            out.push(make(PointerPhase::Down, button, origin, origin, Vec2::ZERO));
        } else if response.dragged_by(button)
            && let (Some(pos), Some(origin)) = (current, origin)
            && delta != Vec2::ZERO
        {
            out.push(make(PointerPhase::Drag, button, pos, origin, delta));
        }
        if response.drag_stopped_by(button)
            && let Some(pos) = current
        {
            out.push(make(PointerPhase::Up, button, pos, press_origin.unwrap_or(pos), Vec2::ZERO));
        }
        if response.clicked_by(button)
            && let Some(pos) = current
        {
            out.push(make(PointerPhase::Click, button, pos, pos, Vec2::ZERO));
        }
        if response.double_clicked_by(button)
            && let Some(pos) = current
        {
            out.push(make(PointerPhase::DoubleClick, button, pos, pos, Vec2::ZERO));
        }
    }
    out
}

#[derive(Default)]
struct Interactions {
    events: Vec<StageEvent>,
    /// Guide positions to preview while dragging.
    preview: Vec<Guide>,
}

fn guide_pos(frame: &Frame, axis: Axis, screen: Pos2) -> f32 {
    let p = frame.view.to_stage(frame.canvas, screen);
    match axis {
        Axis::Horizontal => p.y.round(),
        Axis::Vertical => p.x.round(),
    }
}

fn guide_interactions(ui: &Ui, state: &StageViewState, frame: &Frame, id: Id) -> Interactions {
    let mut out = Interactions::default();
    if !state.show_guides || state.lock_guides {
        return out;
    }
    for (index, guide) in state.guides.iter().enumerate() {
        let screen = match guide.axis {
            Axis::Horizontal => frame.view.to_screen(frame.canvas, pos2(0.0, guide.pos)).y,
            Axis::Vertical => frame.view.to_screen(frame.canvas, pos2(guide.pos, 0.0)).x,
        };
        let hit = match guide.axis {
            Axis::Horizontal => Rect::from_min_max(pos2(frame.canvas.left(), screen - 3.0), pos2(frame.canvas.right(), screen + 3.0)),
            Axis::Vertical => Rect::from_min_max(pos2(screen - 3.0, frame.canvas.top()), pos2(screen + 3.0, frame.canvas.bottom())),
        };
        if !hit.intersects(frame.canvas) {
            continue;
        }
        let r = ui.interact(hit.intersect(frame.canvas), id.with(("guide", index)), Sense::drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(match guide.axis {
                Axis::Horizontal => CursorIcon::ResizeVertical,
                Axis::Vertical => CursorIcon::ResizeHorizontal,
            });
        }
        if r.dragged()
            && let Some(p) = r.interact_pointer_pos()
        {
            out.preview.push(Guide { axis: guide.axis, pos: guide_pos(frame, guide.axis, p) });
        }
        if r.drag_stopped()
            && let Some(p) = r.interact_pointer_pos()
        {
            if frame.canvas.contains(p) {
                out.events.push(StageEvent::GuideMoved { index, pos: guide_pos(frame, guide.axis, p) });
            } else {
                out.events.push(StageEvent::GuideRemoved { index });
            }
        }
    }
    out
}

fn ruler_interactions(ui: &Ui, state: &StageViewState, frame: &Frame, id: Id) -> Interactions {
    let mut out = Interactions::default();
    if !state.show_rulers || state.lock_guides {
        return out;
    }
    let top = Rect::from_min_max(pos2(frame.canvas.left(), frame.outer.top()), pos2(frame.outer.right(), frame.canvas.top()));
    let left = Rect::from_min_max(pos2(frame.outer.left(), frame.canvas.top()), pos2(frame.canvas.left(), frame.outer.bottom()));
    for (rect, axis, tag) in [(top, Axis::Horizontal, "ruler-top"), (left, Axis::Vertical, "ruler-left")] {
        let r = ui.interact(rect, id.with(tag), Sense::drag());
        if r.dragged()
            && let Some(p) = r.interact_pointer_pos()
        {
            out.preview.push(Guide { axis, pos: guide_pos(frame, axis, p) });
        }
        if r.drag_stopped()
            && let Some(p) = r.interact_pointer_pos()
            && frame.canvas.contains(p)
        {
            out.events.push(StageEvent::GuideAdded(Guide { axis, pos: guide_pos(frame, axis, p) }));
        }
    }
    out
}

fn paint_canvas(
    painter: &Painter,
    state: &StageViewState,
    frame: &Frame,
    texture: Option<&TextureHandle>,
    hover: Option<Pos2>,
    preview_a: &[Guide],
    preview_b: &[Guide],
) {
    let p = &frame.palette;
    painter.rect_filled(frame.outer, 0.0, p.pasteboard);
    let inner = painter.with_clip_rect(frame.canvas);
    let doc = frame.doc_rect(state);
    if doc.is_positive() {
        inner.rect_filled(doc.translate(vec2(3.0, 3.0)), 0.0, Color32::from_black_alpha(70));
        inner.rect_filled(doc, 0.0, state.stage_color);
        if let Some(tex) = texture {
            inner.image(tex.id(), doc, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
    if state.show_grid {
        paint_grid(&inner, state, frame, doc);
    }
    if state.show_stage_border && doc.is_positive() {
        inner.rect_stroke(doc, 0.0, Stroke::new(1.0, p.border), egui::StrokeKind::Outside);
    }
    if state.show_guides {
        let dragged: Vec<Guide> = preview_a.iter().chain(preview_b).copied().collect();
        for guide in state.guides.iter().chain(dragged.iter()) {
            paint_guide(&inner, frame, *guide);
        }
    }
    for overlay in &state.overlays {
        paint_overlay(&inner, &frame.view, frame.canvas, overlay, p.recessed);
    }
    if state.show_rulers {
        paint_rulers(painter, state, frame, hover);
    }
}

fn paint_guide(painter: &Painter, frame: &Frame, guide: Guide) {
    let stroke = Stroke::new(1.0, frame.palette.guide);
    match guide.axis {
        Axis::Horizontal => {
            let y = frame.view.to_screen(frame.canvas, pos2(0.0, guide.pos)).y;
            painter.hline(frame.canvas.x_range(), y, stroke);
        }
        Axis::Vertical => {
            let x = frame.view.to_screen(frame.canvas, pos2(guide.pos, 0.0)).x;
            painter.vline(x, frame.canvas.y_range(), stroke);
        }
    }
}

fn paint_grid(painter: &Painter, state: &StageViewState, frame: &Frame, doc: Rect) {
    if !state.grid_size.is_finite() || state.grid_size <= 0.0 || !doc.is_positive() {
        return;
    }
    let mut step = state.grid_size;
    let mut guard = 0u8;
    while step * frame.view.zoom < 6.0 && guard < 24 {
        step *= 2.0;
        guard = guard.saturating_add(1);
    }
    let visible = frame.view.visible_stage_rect(frame.canvas).intersect(Rect::from_min_size(Pos2::ZERO, state.doc_size));
    if !visible.is_positive() {
        return;
    }
    let stroke = Stroke::new(1.0, frame.palette.grid_line_strong.gamma_multiply(0.8));
    let mut x = (visible.left() / step).ceil() * step;
    let mut count = 0usize;
    while x <= visible.right() && count < 2000 {
        let sx = frame.view.to_screen(frame.canvas, pos2(x, 0.0)).x;
        painter.vline(sx, doc.y_range(), stroke);
        x += step;
        count = count.saturating_add(1);
    }
    let mut y = (visible.top() / step).ceil() * step;
    while y <= visible.bottom() && count < 4000 {
        let sy = frame.view.to_screen(frame.canvas, pos2(0.0, y)).y;
        painter.hline(doc.x_range(), sy, stroke);
        y += step;
        count = count.saturating_add(1);
    }
}

fn ruler_label(value: f32) -> String {
    if (value - value.round()).abs() < 0.005 { format!("{}", crate::util::floor_i32(value.round())) } else { format!("{value:.1}") }
}

fn paint_rulers(painter: &Painter, state: &StageViewState, frame: &Frame, hover: Option<Pos2>) {
    let p = &frame.palette;
    let font = egui::FontId::proportional(9.0);
    let top = Rect::from_min_max(frame.outer.min, pos2(frame.outer.right(), frame.canvas.top()));
    let left = Rect::from_min_max(frame.outer.min, pos2(frame.canvas.left(), frame.outer.bottom()));
    painter.rect_filled(top, 0.0, p.surface);
    painter.rect_filled(left, 0.0, p.surface);
    let visible = frame.view.visible_stage_rect(frame.canvas);
    let tick = Stroke::new(1.0, p.text_weak);
    let top_painter = painter.with_clip_rect(Rect::from_min_max(pos2(frame.canvas.left(), top.top()), top.max));
    for t in ruler_ticks(visible.left(), visible.right(), frame.view.zoom) {
        let x = frame.view.to_screen(frame.canvas, pos2(t.value, 0.0)).x;
        let len = if t.major { RULER_SIZE } else { RULER_SIZE * 0.35 };
        top_painter.vline(x, (top.bottom() - len)..=top.bottom(), tick);
        if t.major {
            top_painter.text(pos2(x + 2.0, top.top() + 1.0), egui::Align2::LEFT_TOP, ruler_label(t.value), font.clone(), p.text_weak);
        }
    }
    let left_painter = painter.with_clip_rect(Rect::from_min_max(pos2(left.left(), frame.canvas.top()), left.max));
    for t in ruler_ticks(visible.top(), visible.bottom(), frame.view.zoom) {
        let y = frame.view.to_screen(frame.canvas, pos2(0.0, t.value)).y;
        let len = if t.major { RULER_SIZE } else { RULER_SIZE * 0.35 };
        left_painter.hline((left.right() - len)..=left.right(), y, tick);
        if t.major {
            let galley = left_painter.layout_no_wrap(ruler_label(t.value), font.clone(), p.text_weak);
            left_painter.add(egui::epaint::TextShape::new(pos2(left.left() + 1.0, y - 2.0), galley, p.text_weak).with_angle(-std::f32::consts::FRAC_PI_2));
        }
    }
    painter.rect_filled(Rect::from_min_max(frame.outer.min, frame.canvas.min), 0.0, p.panel);
    painter.hline(frame.outer.x_range(), top.bottom(), Stroke::new(1.0, p.border));
    painter.vline(left.right(), frame.outer.y_range(), Stroke::new(1.0, p.border));
    if let Some(h) = hover
        && frame.canvas.contains(h)
    {
        let mark = Stroke::new(1.0, p.accent);
        painter.vline(h.x, top.y_range(), mark);
        painter.hline(left.x_range(), h.y, mark);
    }
    let _ = state;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_texture_size_follows_zoom() {
        let mut s = StageViewState::new(vec2(1920.0, 1080.0));
        s.view.zoom = 0.5;
        assert_eq!(s.desired_texture_size(2.0), [1920, 1080]);
        s.view.zoom = 100.0;
        assert_eq!(s.desired_texture_size(1.0), [8192, 8192]);
    }

    #[test]
    fn ruler_labels() {
        assert_eq!(ruler_label(100.0), "100");
        assert_eq!(ruler_label(-50.0), "-50");
        assert_eq!(ruler_label(0.5), "0.5");
    }
}
