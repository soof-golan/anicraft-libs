//! The [`timeline`] widget: layer column, frame grid, ruler, playhead, selection, context menu, scrolling.

use egui::{Align2, Color32, FontId, Id, Painter, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use super::{
    geometry::{FrameSelection, Hit, TimelineGeometry, layers_between},
    layers::{draw_header, draw_rows},
    model::{LabelKind, LayerRow, Marker, Span, TimelineModel, TweenKind},
    state::{FrameAction, FrameSize, LayerEvent, PreviewMode, TimelineEvent, TimelineViewState},
};
use crate::{
    icons::Icon,
    theme::{Palette, current_palette, visual},
    util::{f32_from_u32, f32_from_usize},
    widgets::icon_button,
};

/// Height of the frame-number ruler.
pub const RULER_HEIGHT: f32 = 24.0;
/// Height of the bottom bar (layer buttons, zoom).
pub const BAR_HEIGHT: f32 = 28.0;
const SCROLLBAR: f32 = 10.0;
/// Frames of empty space kept after the last used frame.
const SLACK_FRAMES: u32 = 40;

/// Which frame numbers get a label at this zoom: the smallest step from 1, 5, 10, 25, 50, 100 ... that leaves at least
/// 34 points between labels.
#[must_use]
pub fn ruler_label_step(frame_width: f32) -> u32 {
    for step in [1u32, 5, 10, 25, 50, 100, 250, 500, 1000] {
        if f32_from_u32(step) * frame_width >= 34.0 {
            return step;
        }
    }
    5000
}

/// The label for 0-based `frame` on the ruler (Animate counts from 1) given the label `step`, if it gets one.
#[must_use]
pub fn ruler_label(frame: u32, step: u32) -> Option<String> {
    let n = frame.saturating_add(1);
    let labelled = step <= 1 || n.is_multiple_of(step) || (step == 5 && frame == 0);
    labelled.then(|| n.to_string())
}

/// Animate's frame keyboard shortcuts as events.
///
/// F5 insert frame, Shift+F5 remove frames, F6 insert keyframe, Shift+F6 clear keyframe, F7 insert blank keyframe,
/// Ctrl/Cmd+Alt+C/X/V cut, copy and paste frames (with Shift paste and overwrite), Alt+Backspace clear frames and
/// Ctrl/Cmd+Alt+A select all frames.
///
/// Call it once per frame where the timeline has focus (it consumes the keys it handles).
#[must_use = "the keys are consumed: apply the returned events"]
pub fn frame_shortcuts(ctx: &egui::Context) -> Vec<TimelineEvent> {
    use egui::{Key, Modifiers};
    let table = [
        (Modifiers::SHIFT, Key::F5, FrameAction::RemoveFrames),
        (Modifiers::SHIFT, Key::F6, FrameAction::ClearKeyframe),
        (Modifiers::NONE, Key::F5, FrameAction::InsertFrame),
        (Modifiers::NONE, Key::F6, FrameAction::InsertKeyframe),
        (Modifiers::NONE, Key::F7, FrameAction::InsertBlankKeyframe),
        (Modifiers::COMMAND | Modifiers::ALT | Modifiers::SHIFT, Key::V, FrameAction::PasteOverwrite),
        (Modifiers::COMMAND | Modifiers::ALT, Key::C, FrameAction::CopyFrames),
        (Modifiers::COMMAND | Modifiers::ALT, Key::X, FrameAction::CutFrames),
        (Modifiers::COMMAND | Modifiers::ALT, Key::V, FrameAction::PasteFrames),
        (Modifiers::COMMAND | Modifiers::ALT, Key::A, FrameAction::SelectAllFrames),
        (Modifiers::ALT, Key::Backspace, FrameAction::ClearFrames),
    ];
    ctx.input_mut(|i| table.iter().filter(|(m, k, _)| i.consume_key(*m, *k)).map(|(_, _, a)| TimelineEvent::FrameAction(*a)).collect())
}

/// Selection spanning the visible rows `a_row..=b_row` and frames `a_frame..=b_frame` (in either order).
#[must_use]
pub fn range_selection(visible_ids: &[u64], a: (usize, u32), b: (usize, u32)) -> FrameSelection {
    FrameSelection { layers: layers_between(visible_ids, a.0, b.0), first: a.1.min(b.1), last: a.1.max(b.1) }
}

/// Signed frame and row offsets between two cells, with the frame offset limited so the selection never starts
/// before frame 0.
#[must_use]
pub fn drag_delta(selection_first: u32, from: (usize, u32), to: (usize, u32)) -> (i32, i32) {
    let df = i64::from(to.1).saturating_sub(i64::from(from.1));
    let df = df.max(i64::from(selection_first).saturating_neg());
    let dr = i64::try_from(to.0).unwrap_or(0).saturating_sub(i64::try_from(from.0).unwrap_or(0));
    (i32::try_from(df).unwrap_or(0), i32::try_from(dr).unwrap_or(0))
}

struct Layout {
    geo: TimelineGeometry,
    column: Rect,
    column_header: Rect,
    bar: Rect,
    frames: u32,
}

fn compute_layout(outer: Rect, model: &TimelineModel, view: &TimelineViewState, rows: usize) -> Layout {
    let bar = Rect::from_min_max(pos2(outer.left(), outer.bottom() - BAR_HEIGHT), outer.max);
    let main = Rect::from_min_max(outer.min, pos2(outer.right(), bar.top()));
    let col_w = view.layer_column_width.clamp(120.0, (main.width() * 0.6).max(120.0));
    let column = Rect::from_min_max(main.min, pos2(main.left() + col_w, main.bottom()));
    let column_header = Rect::from_min_size(column.min, vec2(col_w, RULER_HEIGHT));
    let right = Rect::from_min_max(pos2(column.right(), main.top()), main.max);
    let ruler = Rect::from_min_size(right.min, vec2(right.width() - SCROLLBAR, RULER_HEIGHT));
    let grid = Rect::from_min_max(pos2(right.left(), right.top() + RULER_HEIGHT), pos2(right.right() - SCROLLBAR, right.bottom() - SCROLLBAR));
    let geo =
        TimelineGeometry { ruler, grid, scroll_x: view.scroll_x, scroll_y: view.scroll_y, frame_width: view.frame_width, row_height: view.row_height, rows };
    let fill = crate::util::ceil_u32(grid.width() / view.frame_width);
    let frames = model.frame_count.max(model.used_frames()).max(model.current_frame.saturating_add(1)).saturating_add(SLACK_FRAMES).max(fill);
    Layout { geo, column, column_header, bar, frames }
}

/// Draws the whole timeline into the available space and returns what the user did.
pub fn timeline(ui: &mut Ui, model: &TimelineModel, view: &TimelineViewState) -> Vec<TimelineEvent> {
    let p = current_palette(ui.ctx());
    let (outer, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let id = Id::new("anim-timeline");
    let visible = model.visible_rows();
    let visible_ids: Vec<u64> = visible.iter().filter_map(|&i| model.layers.get(i).map(|l| l.id)).collect();
    let mut layout = compute_layout(outer, model, view, visible.len());
    let mut events = Vec::new();

    // Keep the playhead in view during playback.
    let mut view_local = view.clone();
    // Also reveal it when the current frame changed since the last paint (goto, opening a file, stepping), not
    // only while playing.
    let moved = ui.data_mut(|d| {
        let slot = d.get_temp_mut_or::<Option<u32>>(id.with("last-frame"), None);
        let moved = *slot != Some(model.current_frame);
        *slot = Some(model.current_frame);
        moved
    });
    if (model.playing || moved)
        && view.follow_playhead
        && let Some(x) = layout.geo.scroll_to_reveal(model.current_frame)
    {
        view_local.scroll_x = x;
        layout.geo.scroll_x = x;
        events.push(TimelineEvent::Scroll { x, y: view.scroll_y });
    }
    let view = &view_local;

    ui.painter().rect_filled(outer, 0.0, p.panel);
    events.extend(wheel_events(ui, &layout, view, outer));

    // Layer column.
    events.extend(draw_header(ui, layout.column_header, model, id).into_iter().map(TimelineEvent::Layer));
    let rows_clip = Rect::from_min_max(pos2(layout.column.left(), layout.geo.grid.top()), pos2(layout.column.right(), layout.geo.grid.bottom()));
    let (first_row, last_row) = layout.geo.visible_rows();
    {
        let mut col_ui = ui.new_child(egui::UiBuilder::new().max_rect(rows_clip));
        col_ui.set_clip_rect(rows_clip.intersect(ui.clip_rect()));
        let origin = layout.geo.grid.top() - layout.geo.scroll_y;
        events.extend(draw_rows(&col_ui, id, rows_clip.x_range(), origin, model, view, &visible, (first_row, last_row)).into_iter().map(TimelineEvent::Layer));
    }
    ui.painter().vline(layout.column.right(), outer.y_range(), Stroke::new(1.0, p.border));

    // Frame grid.
    let grid_painter = ui.painter_at(layout.geo.grid);
    paint_grid(&grid_painter, model, view, &layout, &visible, &p);
    events.extend(grid_interaction(ui, model, view, &layout, &visible_ids, id, &p));
    paint_ruler(ui, model, view, &layout, &p);
    paint_markers(ui, model, &layout, &p);
    events.extend(ruler_interaction(ui, model, view, &layout, id));
    paint_playhead(ui, model, &layout, &p);
    events.extend(scrollbars(ui, view, &layout, id));
    events.extend(bottom_bar(ui, model, view, &layout, id));
    events
}

fn wheel_events(ui: &Ui, layout: &Layout, view: &TimelineViewState, outer: Rect) -> Vec<TimelineEvent> {
    let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else { return Vec::new() };
    if !outer.contains(pointer) || !ui.rect_contains_pointer(outer) {
        return Vec::new();
    }
    let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
    if (zoom - 1.0).abs() > f32::EPSILON {
        let (frame_width, scroll_x) = layout.geo.zoom_about(pointer.x.max(layout.geo.grid.left()), view.frame_width * zoom);
        return vec![TimelineEvent::Zoom { frame_width, scroll_x }];
    }
    if scroll == Vec2::ZERO {
        return Vec::new();
    }
    let target = layout.geo.clamp_scroll(layout.frames, vec2(view.scroll_x - scroll.x, view.scroll_y - scroll.y));
    if (target.x - view.scroll_x).abs() > f32::EPSILON || (target.y - view.scroll_y).abs() > f32::EPSILON {
        vec![TimelineEvent::Scroll { x: target.x, y: target.y }]
    } else {
        Vec::new()
    }
}

fn span_colors(span: &Span, p: &Palette) -> (Color32, Color32) {
    let tween = match span.tween {
        TweenKind::None => None,
        TweenKind::Classic => Some(p.classic_tween),
        TweenKind::Shape => Some(p.shape_tween),
        TweenKind::Motion => Some(p.motion_tween),
    };
    match tween {
        Some(c) => (c.gamma_multiply(0.55), c),
        None if span.blank => (p.recessed, p.border),
        None => (p.span, p.border),
    }
}

fn paint_grid(painter: &Painter, model: &TimelineModel, view: &TimelineViewState, layout: &Layout, visible: &[usize], p: &Palette) {
    let geo = &layout.geo;
    painter.rect_filled(geo.grid, 0.0, p.recessed);
    let (f0, f1) = geo.visible_frames();
    let (r0, r1) = geo.visible_rows();
    // Rows.
    for (row, &layer_idx) in visible.iter().enumerate().skip(r0).take(r1.saturating_sub(r0)) {
        let Some(layer) = model.layers.get(layer_idx) else { continue };
        let top = geo.y_of_row(row);
        let rect = Rect::from_min_max(pos2(geo.grid.left(), top), pos2(geo.grid.right(), top + geo.row_height));
        if row % 2 == 1 {
            painter.rect_filled(rect, 0.0, p.panel.gamma_multiply(0.35));
        }
        if layer.is_folder() {
            painter.rect_filled(rect, 0.0, p.surface.gamma_multiply(0.7));
        }
        painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.grid_line));
    }
    // Columns: every fifth frame tinted, a line per frame when cells are wide enough.
    let content_bottom = geo.y_of_row(geo.rows).min(geo.grid.bottom());
    for f in f0..f1 {
        let x = geo.x_of_frame(f);
        let strong = f.saturating_add(1) % 5 == 0;
        if strong {
            painter.rect_filled(
                Rect::from_min_max(pos2(x, geo.grid.top()), pos2(x + geo.frame_width, content_bottom)),
                0.0,
                p.grid_line_strong.gamma_multiply(0.35),
            );
        }
        if geo.frame_width >= 5.0 || strong {
            let c = if strong { p.grid_line_strong } else { p.grid_line };
            painter.vline(x + geo.frame_width, geo.grid.top()..=content_bottom, Stroke::new(1.0, c));
        }
    }
    // Beyond the last frame of the timeline.
    let end_x = geo.x_of_frame(model.frame_count);
    if end_x < geo.grid.right() {
        let beyond = Rect::from_min_max(pos2(end_x.max(geo.grid.left()), geo.grid.top()), pos2(geo.grid.right(), content_bottom));
        painter.rect_filled(beyond, 0.0, p.panel.gamma_multiply(0.6));
    }
    // Spans.
    for (row, &layer_idx) in visible.iter().enumerate().skip(r0).take(r1.saturating_sub(r0)) {
        if let Some(layer) = model.layers.get(layer_idx)
            && !layer.is_folder()
        {
            paint_spans(painter, layer, row, view, geo, p, (f0, f1));
        }
    }
    paint_selection(painter, view, geo, model, visible, p);
    if model.onion.enabled || view.edit_multiple {
        let lo = model.current_frame.saturating_sub(model.onion.before);
        let hi = model.current_frame.saturating_add(model.onion.after);
        let band = Rect::from_min_max(pos2(geo.x_of_frame(lo), geo.grid.top()), pos2(geo.x_of_frame(hi.saturating_add(1)), content_bottom));
        painter.rect_filled(band, 0.0, p.onion.gamma_multiply(if view.edit_multiple { 0.18 } else { 0.10 }));
    }
}

fn paint_spans(painter: &Painter, layer: &LayerRow, row: usize, view: &TimelineViewState, geo: &TimelineGeometry, p: &Palette, visible_frames: (u32, u32)) {
    let first = layer.spans.partition_point(|s| s.end() <= visible_frames.0);
    for span in layer.spans.iter().skip(first) {
        if span.start >= visible_frames.1 {
            break;
        }
        let rect = geo.cell_rect(row, span.start, span.end().saturating_sub(1)).shrink2(vec2(0.0, 1.0));
        let (body, edge) = if view.tinted_frames || span.tween == TweenKind::None { span_colors(span, p) } else { (p.span, p.border) };
        painter.rect(rect, 0.0, body, Stroke::new(1.0, edge), StrokeKind::Inside);
        if let (true, Some(wave)) = (view.show_waveforms, span.sound.as_ref()) {
            paint_waveform(painter, rect, &wave.peaks, p);
        }
        paint_preview(painter, span, row, geo, view.preview);
        paint_span_marks(painter, span, row, geo, p);
    }
}

fn paint_waveform(painter: &Painter, rect: Rect, peaks: &[f32], p: &Palette) {
    if peaks.is_empty() {
        return;
    }
    let n = f32_from_usize(peaks.len());
    let mid = rect.center().y;
    let half = rect.height() * 0.42;
    let stroke = Stroke::new(1.0, p.text_weak);
    let clip = painter.clip_rect();
    let step = (rect.width() / n).clamp(1.0, 2.0);
    let start = rect.left().max(clip.left());
    let end = rect.right().min(clip.right());
    let columns = crate::util::ceil_u32(((end - start) / step).min(4096.0));
    for i in 0..columns {
        let x = f32_from_u32(i).mul_add(step, start);
        let idx = crate::util::floor_u32(((x - rect.left()) / rect.width()) * n);
        let peak = peaks.get(anim_core::num::u32_to_usize(idx)).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        painter.vline(x, half.mul_add(-peak, mid)..=half.mul_add(peak, mid), stroke);
    }
}

fn paint_span_marks(painter: &Painter, span: &Span, row: usize, geo: &TimelineGeometry, p: &Palette) {
    let key = geo.cell_rect(row, span.start, span.start);
    let fw = geo.frame_width;
    let r = (fw * 0.3).clamp(1.5, 3.5);
    let end = span.end().saturating_sub(1);
    if span.tween != TweenKind::None && span.len > 1 {
        let y = key.center().y;
        let (x0, x1) = (key.center().x + r + 1.0, geo.x_center_of_frame(end) - 1.0);
        let stroke = Stroke::new(1.2, p.keyframe);
        if span.tween_broken {
            painter.extend(Shape::dashed_line(&[pos2(x0, y), pos2(x1, y)], stroke, 3.0, 3.0));
        } else if span.tween != TweenKind::Motion {
            painter.line_segment([pos2(x0, y), pos2(x1, y)], stroke);
            painter.add(Shape::convex_polygon(vec![pos2(x1, y), pos2(x1 - 5.0, y - 3.5), pos2(x1 - 5.0, y + 3.5)], p.keyframe, Stroke::NONE));
        }
    }
    if fw >= 5.0 {
        if span.blank {
            painter.circle_stroke(key.center(), r, Stroke::new(1.0, p.keyframe));
        } else {
            painter.circle_filled(key.center(), r, p.keyframe);
        }
        if span.len > 1 && span.tween == TweenKind::None && fw >= 6.0 {
            let last = geo.cell_rect(row, end, end);
            painter.rect_stroke(
                Rect::from_center_size(last.center(), vec2(fw - 4.0, geo.row_height * 0.5)),
                0.0,
                Stroke::new(1.0, p.keyframe),
                StrokeKind::Middle,
            );
        }
    }
    if let Some(label) = &span.label {
        paint_label(painter, key, label, span.label_kind, p);
    }
    if span.has_script {
        painter.text(pos2(key.center().x + fw, key.center().y), Align2::LEFT_CENTER, "a", FontId::proportional(11.0), p.text);
    }
}

/// The frame label next to its keyframe: a red flag for a name, `//` for a comment, an anchor mark for an anchor.
fn paint_label(painter: &Painter, key: Rect, label: &str, kind: LabelKind, p: &Palette) {
    let x = key.right() + 3.0;
    let at = pos2(x + 9.0, key.center().y);
    let (mark, text) = match kind {
        LabelKind::Name => (p.warning, p.text),
        LabelKind::Comment => (p.shape_tween, p.shape_tween),
        LabelKind::Anchor => (p.accent, p.text),
    };
    match kind {
        LabelKind::Name => {
            painter.line_segment([pos2(x, key.top() + 4.0), pos2(x, key.bottom() - 4.0)], Stroke::new(1.0, mark));
            painter.add(Shape::convex_polygon(vec![pos2(x, key.top() + 4.0), pos2(x + 5.0, key.top() + 6.5), pos2(x, key.top() + 9.0)], mark, Stroke::NONE));
            painter.text(at, Align2::LEFT_CENTER, visual(label), FontId::proportional(10.0), text);
        }
        LabelKind::Comment => {
            painter.text(pos2(x, key.center().y), Align2::LEFT_CENTER, format!("// {}", visual(label)), FontId::proportional(10.0), text);
        }
        LabelKind::Anchor => {
            let c = pos2(x + 3.0, key.center().y - 1.0);
            painter.circle_stroke(c, 2.0, Stroke::new(1.0, mark));
            painter.line_segment([pos2(c.x, c.y + 2.0), pos2(c.x, key.bottom() - 3.0)], Stroke::new(1.0, mark));
            painter.line_segment([pos2(c.x - 3.0, key.bottom() - 6.0), pos2(c.x + 3.0, key.bottom() - 6.0)], Stroke::new(1.0, mark));
            painter.text(at, Align2::LEFT_CENTER, visual(label), FontId::proportional(10.0), text);
        }
    }
}

/// The Preview / Preview in Context content of a keyframe: its boxes inside the first cell or across the span.
fn paint_preview(painter: &Painter, span: &Span, row: usize, geo: &TimelineGeometry, mode: PreviewMode) {
    if span.preview.is_empty() || mode == PreviewMode::Off {
        return;
    }
    let area = geo.cell_rect(row, span.start, span.end().saturating_sub(1)).shrink2(vec2(1.0, 2.0));
    // Preview fits the content's own bounds into the area; in context keeps the position on the stage.
    let (min, size) = if mode == PreviewMode::Preview {
        let min_x = span.preview.iter().map(|b| b.x).fold(f32::INFINITY, f32::min);
        let min_y = span.preview.iter().map(|b| b.y).fold(f32::INFINITY, f32::min);
        let max_x = span.preview.iter().map(|b| b.x + b.w).fold(f32::NEG_INFINITY, f32::max);
        let max_y = span.preview.iter().map(|b| b.y + b.h).fold(f32::NEG_INFINITY, f32::max);
        (pos2(min_x, min_y), vec2((max_x - min_x).max(1.0e-4), (max_y - min_y).max(1.0e-4)))
    } else {
        (pos2(0.0, 0.0), vec2(1.0, 1.0))
    };
    for b in &span.preview {
        let r = Rect::from_min_size(
            pos2(((b.x - min.x) / size.x).mul_add(area.width(), area.left()), ((b.y - min.y) / size.y).mul_add(area.height(), area.top())),
            vec2((b.w / size.x * area.width()).max(1.0), (b.h / size.y * area.height()).max(1.0)),
        );
        painter.rect_filled(r.intersect(area), 0.0, crate::util::color32(b.color).gamma_multiply(0.85));
    }
}

fn paint_selection(painter: &Painter, view: &TimelineViewState, geo: &TimelineGeometry, model: &TimelineModel, visible: &[usize], p: &Palette) {
    if view.selection.is_empty() {
        return;
    }
    for (row, &layer_idx) in visible.iter().enumerate() {
        let Some(layer) = model.layers.get(layer_idx) else { continue };
        if view.selection.layers.contains(&layer.id) {
            let r = geo.cell_rect(row, view.selection.first, view.selection.last);
            painter.rect(r, 0.0, p.accent_soft, Stroke::new(1.0, p.accent), StrokeKind::Inside);
        }
    }
}

fn paint_ruler(ui: &Ui, model: &TimelineModel, view: &TimelineViewState, layout: &Layout, p: &Palette) {
    let geo = &layout.geo;
    let painter = ui.painter_at(geo.ruler);
    painter.rect_filled(geo.ruler, 0.0, p.surface);
    painter.hline(geo.ruler.x_range(), geo.ruler.bottom(), Stroke::new(1.0, p.border));
    let step = ruler_label_step(geo.frame_width);
    let (f0, f1) = geo.visible_frames();
    let font = FontId::proportional(10.0);
    for f in f0..f1 {
        let x = geo.x_of_frame(f);
        let n = f.saturating_add(1);
        let major = n % 5 == 0 || f == 0;
        if geo.frame_width >= 5.0 || major {
            let h = if major { 6.0 } else { 3.0 };
            painter.vline(geo.frame_width.mul_add(0.5, x), (geo.ruler.bottom() - h)..=geo.ruler.bottom(), Stroke::new(1.0, p.text_weak));
        }
        // A button's first four frames are its states, named instead of numbered (initials when the cells are narrow).
        let state = model.button_states.then(|| usize::try_from(f).ok().and_then(|i| super::model::BUTTON_STATE_LABELS.get(i).copied())).flatten();
        if let Some(name) = state {
            let text = if geo.frame_width >= 34.0 { name.to_owned() } else { name.chars().take(1).collect() };
            painter.text(pos2(geo.frame_width.mul_add(0.5, x), geo.ruler.top() + 5.0), Align2::CENTER_TOP, text, font.clone(), p.text);
        } else if let Some(label) = ruler_label(f, step) {
            painter.text(pos2(geo.frame_width.mul_add(0.5, x), geo.ruler.top() + 5.0), Align2::CENTER_TOP, label, font.clone(), p.text_weak);
        }
    }
    if model.onion.enabled || view.edit_multiple {
        let lo = model.current_frame.saturating_sub(model.onion.before);
        let hi = model.current_frame.saturating_add(model.onion.after).saturating_add(1);
        let band = Rect::from_min_max(pos2(geo.x_of_frame(lo), geo.ruler.bottom() - 7.0), pos2(geo.x_of_frame(hi), geo.ruler.bottom() - 1.0));
        painter.rect(band, 1.0, p.onion.gamma_multiply(0.45), Stroke::new(1.0, p.onion), StrokeKind::Inside);
        for x in [band.left(), band.right()] {
            painter.rect_filled(Rect::from_center_size(pos2(x, band.center().y), vec2(4.0, 10.0)), 1.0, p.onion);
        }
    }
}

/// The marker flags on the ruler: a pennant on the frame with the name beside it (cut off at the next flag).
fn paint_markers(ui: &Ui, model: &TimelineModel, layout: &Layout, p: &Palette) {
    let geo = &layout.geo;
    if model.markers.is_empty() {
        return;
    }
    let painter = ui.painter_at(geo.ruler);
    let (f0, f1) = geo.visible_frames();
    let mut sorted: Vec<&Marker> = model.markers.iter().collect();
    sorted.sort_by_key(|m| m.frame);
    let font = FontId::proportional(10.0);
    for (i, m) in sorted.iter().enumerate() {
        if m.frame < f0.saturating_sub(40) || m.frame > f1 {
            continue;
        }
        let x = geo.x_center_of_frame(m.frame);
        let color = Color32::from_rgba_unmultiplied(m.color.r, m.color.g, m.color.b, m.color.a);
        let bottom = geo.ruler.bottom();
        painter.vline(x, (bottom - 14.0)..=bottom, Stroke::new(1.0, color));
        painter.add(Shape::convex_polygon(vec![pos2(x, bottom - 14.0), pos2(x + 9.0, bottom - 10.5), pos2(x, bottom - 7.0)], color, Stroke::NONE));
        let next_x = sorted.get(i.saturating_add(1)).map_or(f32::INFINITY, |n| geo.x_center_of_frame(n.frame));
        let room = Rect::from_min_max(pos2(x + 11.0, geo.ruler.top()), pos2(next_x.min(geo.ruler.right()) - 2.0, bottom));
        if room.width() > 12.0 && !m.name.is_empty() {
            painter.with_clip_rect(room).text(pos2(room.left(), bottom - 11.0), Align2::LEFT_CENTER, visual(&m.name), font.clone(), p.text);
        }
    }
}

fn paint_playhead(ui: &Ui, model: &TimelineModel, layout: &Layout, p: &Palette) {
    let geo = &layout.geo;
    let region = Rect::from_min_max(geo.ruler.min, geo.grid.max);
    let painter = ui.painter_at(region);
    let x = geo.x_center_of_frame(model.current_frame);
    if x < geo.grid.left() - geo.frame_width || x > geo.grid.right() + geo.frame_width {
        return;
    }
    let head = Rect::from_center_size(pos2(x, geo.ruler.center().y + 2.0), vec2(geo.frame_width.max(9.0), geo.ruler.height() - 8.0));
    painter.rect_filled(head, 2.0, p.playhead);
    painter.vline(x, geo.ruler.bottom()..=geo.grid.bottom(), Stroke::new(1.5, p.playhead));
    if geo.frame_width >= 9.0 {
        painter.text(head.center(), Align2::CENTER_CENTER, model.current_frame.saturating_add(1).to_string(), FontId::proportional(9.0), Color32::WHITE);
    }
}

fn grid_interaction(ui: &Ui, model: &TimelineModel, view: &TimelineViewState, layout: &Layout, visible_ids: &[u64], id: Id, p: &Palette) -> Vec<TimelineEvent> {
    let geo = &layout.geo;
    let mut events = Vec::new();
    let response = ui.interact(geo.grid, id.with("grid"), Sense::click_and_drag());
    let max_frame = model.frame_count.saturating_sub(1);
    let mode_id = id.with("grid-drag-mode");
    let (press, pos, modifiers) = ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()), i.modifiers));
    let cell_at = |at: egui::Pos2| match geo.hit(at) {
        Hit::Cell { row, frame } => Some((row, frame)),
        Hit::Empty { frame } => Some((visible_ids.len().saturating_sub(1), frame)),
        _ => None,
    };
    let anchor = view.selection.layers.first().and_then(|l| visible_ids.iter().position(|v| v == l)).map(|r| (r, view.selection.first));

    if response.drag_started()
        && let Some(start) = press.and_then(cell_at)
    {
        let inside = visible_ids.get(start.0).is_some_and(|l| view.selection.contains(*l, start.1));
        // Where the drag began and whether it moves the selection: egui forgets the press origin on release.
        ui.data_mut(|d| d.insert_temp(mode_id, (inside, start.0, start.1)));
        if !inside {
            let from = if modifiers.shift { anchor.unwrap_or(start) } else { start };
            events.push(TimelineEvent::SelectFrames(range_selection(visible_ids, from, start)));
            events.push(TimelineEvent::SetFrame(start.1.min(max_frame)));
        }
    }
    let drag_info: Option<(bool, usize, u32)> = ui.data(|d| d.get_temp(mode_id));
    if let Some((moving, row, frame)) = drag_info {
        let start = (row, frame);
        if response.dragged()
            && let Some(now) = pos.and_then(cell_at)
        {
            if moving {
                paint_drag_ghost(ui, view, visible_ids, geo, drag_delta(view.selection.first, start, now), p);
            } else {
                let from = if modifiers.shift { anchor.unwrap_or(start) } else { start };
                let sel = range_selection(visible_ids, from, now);
                if sel != view.selection {
                    events.push(TimelineEvent::SelectFrames(sel));
                }
            }
        }
        if response.drag_stopped() {
            if moving && let Some(now) = pos.and_then(cell_at) {
                let (df, dr) = drag_delta(view.selection.first, start, now);
                if df != 0 || dr != 0 {
                    events.push(TimelineEvent::MoveFrames { selection: view.selection.clone(), delta_frames: df, delta_rows: dr });
                }
            }
            ui.data_mut(|d| d.remove::<(bool, usize, u32)>(mode_id));
        }
    }
    if response.clicked()
        && let Some(at) = pos.and_then(cell_at)
    {
        let from = if modifiers.shift { anchor.unwrap_or(at) } else { at };
        events.push(TimelineEvent::SelectFrames(range_selection(visible_ids, from, at)));
        events.push(TimelineEvent::SetFrame(at.1.min(max_frame)));
    }
    if response.secondary_clicked()
        && let Some(at) = pos.and_then(cell_at)
        && visible_ids.get(at.0).is_some_and(|l| !view.selection.contains(*l, at.1))
    {
        events.push(TimelineEvent::SelectFrames(range_selection(visible_ids, at, at)));
    }
    if response.double_clicked() && !view.selection.is_empty() {
        events.push(TimelineEvent::FrameAction(FrameAction::Properties));
    }
    let has_selection = !view.selection.is_empty();
    response.context_menu(|ui| frame_menu(ui, has_selection, &mut events));
    events
}

fn paint_drag_ghost(ui: &Ui, view: &TimelineViewState, visible_ids: &[u64], geo: &TimelineGeometry, delta: (i32, i32), p: &Palette) {
    let Some(first_row) = view.selection.layers.first().and_then(|l| visible_ids.iter().position(|v| v == l)) else { return };
    let signed = |n: i32| f32_from_u32(n.unsigned_abs()) * if n < 0 { -1.0 } else { 1.0 };
    let width = f32_from_u32(view.selection.width()) * geo.frame_width;
    let height = f32_from_usize(view.selection.layers.len()) * geo.row_height;
    let origin =
        pos2(signed(delta.0).mul_add(geo.frame_width, geo.x_of_frame(view.selection.first)), signed(delta.1).mul_add(geo.row_height, geo.y_of_row(first_row)));
    ui.painter_at(geo.grid).rect_stroke(Rect::from_min_size(origin, vec2(width, height)), 0.0, Stroke::new(1.5, p.accent), StrokeKind::Inside);
}

fn frame_menu(ui: &mut Ui, has_selection: bool, events: &mut Vec<TimelineEvent>) {
    let items: &[(&str, &str, FrameAction)] = &[
        ("Insert Frame", "F5", FrameAction::InsertFrame),
        ("Remove Frames", "Shift+F5", FrameAction::RemoveFrames),
        ("Insert Keyframe", "F6", FrameAction::InsertKeyframe),
        ("Insert Blank Keyframe", "F7", FrameAction::InsertBlankKeyframe),
        ("Clear Keyframe", "Shift+F6", FrameAction::ClearKeyframe),
        ("", "", FrameAction::SelectAllFrames),
        ("Create Classic Tween", "", FrameAction::CreateClassicTween),
        ("Create Shape Tween", "", FrameAction::CreateShapeTween),
        ("Create Motion Tween", "", FrameAction::CreateMotionTween),
        ("Remove Tween", "", FrameAction::RemoveTween),
        ("", "", FrameAction::SelectAllFrames),
        ("Convert to Keyframes", "", FrameAction::ConvertToKeyframes),
        ("Convert to Blank Keyframes", "", FrameAction::ConvertToBlankKeyframes),
        ("Reverse Frames", "", FrameAction::ReverseFrames),
        ("", "", FrameAction::SelectAllFrames),
        ("Cut Frames", "Ctrl+Alt+X", FrameAction::CutFrames),
        ("Copy Frames", "Ctrl+Alt+C", FrameAction::CopyFrames),
        ("Paste Frames", "Ctrl+Alt+V", FrameAction::PasteFrames),
        ("Paste and Overwrite Frames", "Ctrl+Alt+Shift+V", FrameAction::PasteOverwrite),
        ("Clear Frames", "Alt+Backspace", FrameAction::ClearFrames),
        ("Select All Frames", "Ctrl+Alt+A", FrameAction::SelectAllFrames),
        ("", "", FrameAction::SelectAllFrames),
        ("Add Frame Label...", "", FrameAction::EditLabel),
        ("Frame Properties...", "", FrameAction::Properties),
    ];
    for (label, shortcut, action) in items {
        if label.is_empty() {
            ui.separator();
            continue;
        }
        let enabled = has_selection || matches!(action, FrameAction::SelectAllFrames | FrameAction::PasteFrames | FrameAction::PasteOverwrite);
        let mut button = egui::Button::new(*label);
        if !shortcut.is_empty() {
            button = button.shortcut_text(*shortcut);
        }
        if ui.add_enabled(enabled, button).clicked() {
            events.push(TimelineEvent::FrameAction(*action));
            ui.close();
        }
    }
}

/// The context menu of a marker: go to it, rename it, delete it.
fn marker_menu(ui: &mut Ui, model: &TimelineModel, id: u64, events: &mut Vec<TimelineEvent>) {
    let Some(marker) = model.marker(id) else {
        ui.close();
        return;
    };
    if ui.button("Go to Marker").clicked() {
        events.push(TimelineEvent::SetFrame(marker.frame));
        ui.close();
    }
    if ui.button("Rename Marker\u{2026}").clicked() {
        events.push(TimelineEvent::MarkerRename { id });
        ui.close();
    }
    if ui.button("Delete Marker").clicked() {
        events.push(TimelineEvent::MarkerRemove { id });
        ui.close();
    }
}

fn ruler_interaction(ui: &Ui, model: &TimelineModel, view: &TimelineViewState, layout: &Layout, id: Id) -> Vec<TimelineEvent> {
    let geo = &layout.geo;
    let mut events = Vec::new();
    let max_frame = model.frame_count.saturating_sub(1);

    // Onion skin handles first so they win over scrubbing.
    let mut onion_drag = false;
    if model.onion.enabled || view.edit_multiple {
        let lo = model.current_frame.saturating_sub(model.onion.before);
        let hi = model.current_frame.saturating_add(model.onion.after).saturating_add(1);
        for (tag, x, is_before) in [("onion-before", geo.x_of_frame(lo), true), ("onion-after", geo.x_of_frame(hi), false)] {
            let handle = Rect::from_center_size(pos2(x, geo.ruler.bottom() - 4.0), vec2(8.0, 12.0)).intersect(geo.ruler);
            let r = ui.interact(handle, id.with(tag), Sense::drag());
            if r.hovered() || r.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if r.dragged()
                && let Some(p) = r.interact_pointer_pos()
            {
                onion_drag = true;
                let f = geo.frame_at_x(p.x);
                let (before, after) = if is_before {
                    (model.current_frame.saturating_sub(f), model.onion.after)
                } else {
                    (model.onion.before, f.saturating_sub(model.current_frame).saturating_sub(1))
                };
                let (before, after) = (before.min(16), after.min(16));
                if (before, after) != (model.onion.before, model.onion.after) {
                    events.push(TimelineEvent::SetOnionRange { before, after });
                }
            }
        }
    }
    let response = ui.interact(geo.ruler, id.with("ruler"), Sense::click_and_drag());
    // Marker flags are registered after the ruler so they sit on top of it: dragging one moves it, and the scrub
    // below ignores that drag.
    let mut marker_drag = false;
    for m in &model.markers {
        let r = ui.interact(geo.marker_handle(m.frame), id.with(("marker", m.id)), Sense::click_and_drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if r.dragged() || r.drag_started() || r.clicked() {
            marker_drag = true;
        }
        if r.clicked() {
            events.push(TimelineEvent::SetFrame(m.frame));
        }
        r.context_menu(|ui| marker_menu(ui, model, m.id, &mut events));
        if r.dragged()
            && let Some(p) = r.interact_pointer_pos()
        {
            let f = geo.frame_at_x(p.x).min(max_frame);
            if f != m.frame {
                events.push(TimelineEvent::MarkerMoved { id: m.id, frame: f });
            }
        }
    }
    // A right-click on the bare ruler remembers its frame; the menu adds a marker there.
    let frame_id = id.with("ruler-menu-frame");
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        ui.data_mut(|d| d.insert_temp(frame_id, geo.frame_at_x(p.x).min(max_frame)));
    }
    response.context_menu(|ui| {
        let frame = ui.data(|d| d.get_temp::<u32>(frame_id)).unwrap_or(model.current_frame);
        if ui.button("Add Marker").clicked() {
            events.push(TimelineEvent::MarkerAdd { frame });
            ui.close();
        }
    });
    if !onion_drag
        && !marker_drag
        && (response.dragged() || response.clicked() || response.drag_started())
        && let Some(p) = response.interact_pointer_pos()
    {
        let f = geo.frame_at_x(p.x).min(max_frame);
        if f != model.current_frame {
            events.push(TimelineEvent::SetFrame(f));
        }
    }
    events
}

fn scrollbars(ui: &Ui, view: &TimelineViewState, layout: &Layout, id: Id) -> Vec<TimelineEvent> {
    let p = current_palette(ui.ctx());
    let geo = &layout.geo;
    let content = geo.content_size(layout.frames);
    let mut events = Vec::new();
    let h_track = Rect::from_min_max(pos2(geo.grid.left(), geo.grid.bottom()), pos2(geo.grid.right(), geo.grid.bottom() + SCROLLBAR));
    let v_track = Rect::from_min_max(pos2(geo.grid.right(), geo.grid.top()), pos2(geo.grid.right() + SCROLLBAR, geo.grid.bottom()));
    for (track, horizontal) in [(h_track, true), (v_track, false)] {
        let (len, view_len, offset) = if horizontal { (content.x, geo.grid.width(), view.scroll_x) } else { (content.y, geo.grid.height(), view.scroll_y) };
        ui.painter().rect_filled(track, 0.0, p.surface);
        if len <= view_len + 0.5 {
            continue;
        }
        let track_len = if horizontal { track.width() } else { track.height() };
        let thumb_len = (view_len / len * track_len).max(18.0);
        let max_offset = len - view_len;
        let thumb_start = offset / max_offset * (track_len - thumb_len);
        let thumb = if horizontal {
            Rect::from_min_size(pos2(track.left() + thumb_start, track.top() + 1.0), vec2(thumb_len, track.height() - 2.0))
        } else {
            Rect::from_min_size(pos2(track.left() + 1.0, track.top() + thumb_start), vec2(track.width() - 2.0, thumb_len))
        };
        let r = ui.interact(thumb, id.with(("scrollbar", horizontal)), Sense::drag());
        ui.painter().rect_filled(thumb, 3.0, if r.hovered() || r.dragged() { p.text_weak } else { p.border });
        if r.dragged() {
            let delta = if horizontal { r.drag_delta().x } else { r.drag_delta().y };
            let new_offset = (delta / (track_len - thumb_len).max(1.0)).mul_add(max_offset, offset).clamp(0.0, max_offset);
            events.push(if horizontal {
                TimelineEvent::Scroll { x: new_offset, y: view.scroll_y }
            } else {
                TimelineEvent::Scroll { x: view.scroll_x, y: new_offset }
            });
        }
    }
    events
}

fn bottom_bar(ui: &mut Ui, model: &TimelineModel, view: &TimelineViewState, layout: &Layout, id: Id) -> Vec<TimelineEvent> {
    let p = current_palette(ui.ctx());
    let bar = layout.bar;
    ui.painter().rect_filled(bar, 0.0, p.surface);
    ui.painter().hline(bar.x_range(), bar.top(), Stroke::new(1.0, p.border));
    let mut events = Vec::new();
    let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    bar_ui.push_id(id.with("bar"), |ui| {
        let size = vec2(22.0, 22.0);
        if icon_button(ui, size, Icon::LayerNormal, false, true, "New layer").clicked() {
            events.push(TimelineEvent::Layer(LayerEvent::Add));
        }
        if icon_button(ui, size, Icon::Folder, false, true, "New folder").clicked() {
            events.push(TimelineEvent::Layer(LayerEvent::AddFolder));
        }
        let active = view.selected_layers.first().copied();
        if icon_button(ui, size, Icon::Remove, false, active.is_some(), "Delete layer").clicked()
            && let Some(layer) = active
        {
            events.push(TimelineEvent::Layer(LayerEvent::Delete(layer)));
        }
        ui.add_space(12.0);
        if icon_button(ui, size, Icon::Onion, model.onion.enabled, true, "Onion skin").clicked() {
            events.push(TimelineEvent::ToggleOnion);
        }
        if icon_button(ui, size, Icon::OnionOutlines, model.onion.outlines, true, "Onion skin outlines").clicked() {
            events.push(TimelineEvent::ToggleOnionOutlines);
        }
        if icon_button(ui, size, Icon::EditMultiple, view.edit_multiple, true, "Edit multiple frames").clicked() {
            events.push(TimelineEvent::ToggleEditMultiple);
        }
        if icon_button(ui, size, Icon::AutoKeyframe, view.auto_keyframe, true, "Auto keyframe: changing a held frame makes it a keyframe").clicked() {
            events.push(TimelineEvent::ToggleAutoKeyframe);
        }
        ui.add_space(12.0);
        let seconds = if model.fps > 0.0 { f32_from_u32(model.current_frame) / model.fps } else { 0.0 };
        ui.label(
            egui::RichText::new(format!("{}   {:.0} fps   {seconds:.2}s", model.current_frame.saturating_add(1), model.fps)).size(11.0).color(p.text_weak),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, size, Icon::Add, false, true, "Zoom in").clicked() {
                let (w, s) = layout.geo.zoom_about(layout.geo.grid.center().x, view.frame_width * 1.25);
                events.push(TimelineEvent::Zoom { frame_width: w, scroll_x: s });
            }
            if icon_button(ui, size, Icon::Remove, false, true, "Zoom out").clicked() {
                let (w, s) = layout.geo.zoom_about(layout.geo.grid.center().x, view.frame_width / 1.25);
                events.push(TimelineEvent::Zoom { frame_width: w, scroll_x: s });
            }
            ui.menu_button("Frame View", |ui| frame_view_menu(ui, view, &mut events));
        });
    });
    events
}

/// The Frame View menu: cell size, short rows, tinted frames and the previews.
fn frame_view_menu(ui: &mut Ui, view: &TimelineViewState, events: &mut Vec<TimelineEvent>) {
    let mut pick = |size: FrameSize, short: bool, tinted: bool, preview: PreviewMode| {
        events.push(TimelineEvent::SetFrameView { size, short, tinted, preview });
    };
    for size in FrameSize::ALL {
        if ui.add(egui::Button::selectable(view.frame_size == size, size.label())).clicked() {
            pick(size, view.short_rows, view.tinted_frames, view.preview);
            ui.close();
        }
    }
    ui.separator();
    if ui.add(egui::Button::selectable(view.short_rows, "Short")).clicked() {
        pick(view.frame_size, !view.short_rows, view.tinted_frames, view.preview);
        ui.close();
    }
    if ui.add(egui::Button::selectable(view.tinted_frames, "Tinted Frames")).clicked() {
        pick(view.frame_size, view.short_rows, !view.tinted_frames, view.preview);
        ui.close();
    }
    ui.separator();
    for (label, mode) in [("Preview", PreviewMode::Preview), ("Preview in Context", PreviewMode::InContext)] {
        let on = view.preview == mode;
        if ui.add(egui::Button::selectable(on, label)).clicked() {
            pick(view.frame_size, view.short_rows, view.tinted_frames, if on { PreviewMode::Off } else { mode });
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruler_label_steps_grow_as_cells_shrink() {
        assert_eq!(ruler_label_step(40.0), 1);
        assert_eq!(ruler_label_step(12.0), 5);
        assert_eq!(ruler_label_step(5.0), 10);
        assert_eq!(ruler_label_step(3.0), 25);
    }

    #[test]
    fn ruler_labels_are_one_based() {
        assert_eq!(ruler_label(0, 5).as_deref(), Some("1"));
        assert_eq!(ruler_label(4, 5).as_deref(), Some("5"));
        assert_eq!(ruler_label(5, 5), None);
        assert_eq!(ruler_label(9, 10).as_deref(), Some("10"));
        assert_eq!(ruler_label(3, 1).as_deref(), Some("4"));
    }

    #[test]
    fn range_selection_orders_and_clips() {
        let ids = [1, 2, 3, 4];
        let s = range_selection(&ids, (2, 9), (0, 3));
        assert_eq!(s, FrameSelection { layers: vec![1, 2, 3], first: 3, last: 9 });
    }

    #[test]
    fn drag_delta_never_moves_before_frame_zero() {
        assert_eq!(drag_delta(3, (1, 10), (2, 4)), (-3, 1));
        assert_eq!(drag_delta(3, (1, 10), (1, 14)), (4, 0));
        assert_eq!(drag_delta(0, (0, 5), (0, 5)), (0, 0));
    }
}
