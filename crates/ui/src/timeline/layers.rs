//! The layer column: name, eye, lock, outline toggle, colour, type icon, folders, drag to reorder.

use egui::{Align2, Color32, FontId, Id, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::{
    model::{LayerKind, LayerRow, TimelineModel},
    state::{DropPlacement, LayerEvent, TimelineViewState},
};
use crate::{
    icons::{Icon, paint},
    theme::{Palette, current_palette, visual},
    util::{color32, f32_from_u32, f32_from_usize},
};

/// Width of each of the eye / lock / outline columns, in points.
pub const TOGGLE_WIDTH: f32 = 24.0;
/// Indentation per folder level, in points.
pub const INDENT: f32 = 14.0;

/// Drag payload for reordering layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerDrag(pub u64);

const fn kind_icon(kind: LayerKind, expanded: bool) -> Icon {
    match kind {
        LayerKind::Normal | LayerKind::Guided | LayerKind::Masked => Icon::LayerNormal,
        LayerKind::Guide => Icon::LayerGuide,
        LayerKind::Mask => Icon::LayerMask,
        LayerKind::Folder if expanded => Icon::FolderOpen,
        LayerKind::Folder => Icon::Folder,
        LayerKind::Camera => Icon::Camera,
    }
}

fn dim(c: Color32, on: bool) -> Color32 {
    if on { c } else { c.gamma_multiply(0.35) }
}

/// Paints the header strip of the layer column (column toggles for all layers) and returns its events.
pub fn draw_header(ui: &Ui, rect: Rect, model: &TimelineModel, id: Id) -> Vec<LayerEvent> {
    let p = current_palette(ui.ctx());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, p.surface);
    painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.border));
    painter.text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, "Layers", FontId::proportional(11.0), p.text_weak);
    let mut events = Vec::new();
    let all_visible = model.layers.iter().all(|l| l.visible);
    let all_locked = model.layers.iter().all(|l| l.locked);
    let all_outline = model.layers.iter().all(|l| l.outline);
    let columns = [(Icon::Eye, all_visible, 2usize, 0u8), (Icon::Lock, all_locked, 1, 1), (Icon::Outline, all_outline, 0, 2)];
    for (icon, on, from_right, which) in columns {
        let cell = toggle_rect(rect, from_right);
        let r = ui.interact(cell, id.with(("hdr", which)), Sense::click());
        paint(&painter, cell.shrink(5.0), icon, dim(p.text, on || r.hovered()));
        if r.clicked() {
            events.push(match which {
                0 => LayerEvent::SetAllVisible(!all_visible),
                1 => LayerEvent::SetAllLocked(!all_locked),
                _ => LayerEvent::SetAllOutline(!all_outline),
            });
        }
        r.on_hover_text(match which {
            0 => "Show or hide all layers",
            1 => "Lock or unlock all layers",
            _ => "Show all layers as outlines",
        });
    }
    events
}

fn toggle_rect(row: Rect, from_right: usize) -> Rect {
    let right = TOGGLE_WIDTH.mul_add(-f32_from_usize(from_right), row.right());
    Rect::from_min_max(pos2(right - TOGGLE_WIDTH, row.top()), pos2(right, row.bottom()))
}

/// Draws the rows `first..last` of `visible` (indices into `model.layers`) with row 0 starting at `origin_y`.
#[expect(clippy::too_many_arguments, reason = "geometry and state are independent inputs of one draw pass")]
pub fn draw_rows(
    ui: &Ui,
    id: Id,
    x_range: egui::Rangef,
    origin_y: f32,
    model: &TimelineModel,
    view: &TimelineViewState,
    visible: &[usize],
    range: (usize, usize),
) -> Vec<LayerEvent> {
    let p = current_palette(ui.ctx());
    let mut events = Vec::new();
    for (row_idx, &layer_idx) in visible.iter().enumerate().skip(range.0).take(range.1.saturating_sub(range.0)) {
        let Some(layer) = model.layers.get(layer_idx) else { continue };
        let top = f32_from_usize(row_idx).mul_add(view.row_height, origin_y);
        let rect = Rect::from_x_y_ranges(x_range, top..=top + view.row_height);
        draw_row(ui, id, rect, layer, row_idx, view, &p, &mut events);
    }
    events
}

#[expect(clippy::too_many_arguments, reason = "one row needs its rect, data, state, palette and the event sink")]
fn draw_row(ui: &Ui, id: Id, rect: Rect, layer: &LayerRow, row_idx: usize, view: &TimelineViewState, p: &Palette, events: &mut Vec<LayerEvent>) {
    let painter = ui.painter_at(rect);
    let selected = view.selected_layers.contains(&layer.id);
    let fill = if selected {
        p.accent_soft
    } else if row_idx.is_multiple_of(2) {
        p.panel
    } else {
        p.surface.gamma_multiply(0.6)
    };
    painter.rect_filled(rect, 0.0, fill);
    painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.grid_line));

    let row_id = id.with(("row", layer.id));
    let row = ui.interact(rect, row_id, Sense::click_and_drag());
    if row.clicked() || row.drag_started() {
        events.push(LayerEvent::Select { id: layer.id, modifiers: ui.input(|i| i.modifiers) });
    }
    if row.double_clicked() {
        events.push(LayerEvent::RenameRequested(layer.id));
    }
    row.dnd_set_drag_payload(LayerDrag(layer.id));
    drop_target(&row, &painter, layer, p, events);
    let layer_id = layer.id;
    let kind = layer.kind;
    row.context_menu(|ui| layer_menu(ui, layer_id, kind, events));

    // Left side: indent, chevron, icon, colour chip, name.
    let mut x = INDENT.mul_add(f32_from_u32(layer.depth), rect.left() + 4.0);
    if layer.is_folder() {
        let chev = Rect::from_min_size(pos2(x, rect.top()), vec2(14.0, rect.height()));
        let r = ui.interact(chev, id.with(("chev", layer.id)), Sense::click());
        paint(&painter, chev.shrink2(vec2(2.0, 6.0)), if layer.expanded { Icon::ChevronDown } else { Icon::ChevronRight }, p.text_weak);
        if r.clicked() {
            events.push(LayerEvent::ToggleFolder(layer.id));
        }
    }
    x += 16.0;
    let icon_rect = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(rect.height() - 6.0, rect.height() - 6.0));
    paint(&painter, icon_rect, kind_icon(layer.kind, layer.expanded), p.text_weak);
    x += icon_rect.width() + 6.0;
    let name_font = FontId::proportional(12.0);
    let name_color = if layer.visible { p.text } else { p.text_weak };
    let name_right = TOGGLE_WIDTH.mul_add(-3.0, rect.right()) - 4.0;
    painter.with_clip_rect(Rect::from_min_max(pos2(x, rect.top()), pos2(name_right, rect.bottom()))).text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        visual(&layer.name),
        name_font,
        name_color,
    );

    // Right side: eye, lock, outline.
    for (from_right, which) in [(2usize, 0u8), (1, 1), (0, 2)] {
        let cell = toggle_rect(rect, from_right);
        let r = ui.interact(cell, id.with(("toggle", layer.id, which)), Sense::click());
        let inner = cell.shrink2(vec2(5.0, 5.0));
        match which {
            0 => paint(&painter, inner, if layer.visible { Icon::Eye } else { Icon::EyeOff }, dim(p.text, layer.visible)),
            1 => paint(&painter, inner, if layer.locked { Icon::Lock } else { Icon::Unlock }, dim(p.text, layer.locked)),
            _ => {
                let chip = Rect::from_center_size(cell.center(), vec2(11.0, 11.0));
                let color = color32(layer.color);
                if layer.outline {
                    painter.rect_stroke(chip, 1.0, Stroke::new(1.5, color), egui::StrokeKind::Inside);
                } else {
                    painter.rect_filled(chip, 1.0, color);
                }
            }
        }
        if r.clicked() {
            events.push(match which {
                0 => LayerEvent::ToggleVisible(layer.id),
                1 => LayerEvent::ToggleLocked(layer.id),
                _ => LayerEvent::ToggleOutline(layer.id),
            });
        }
    }
}

/// The layer context menu: insert, delete, the layer type (Guide and Mask toggle), the motion guide and properties.
fn layer_menu(ui: &mut Ui, layer: u64, kind: LayerKind, events: &mut Vec<LayerEvent>) {
    let convertible = !matches!(kind, LayerKind::Camera);
    let toggled = |target: LayerKind| if kind == target { LayerKind::Normal } else { target };
    let items: [(&str, Option<LayerEvent>, bool, bool); 11] = [
        ("Insert Layer", Some(LayerEvent::Add), true, false),
        ("Insert Folder", Some(LayerEvent::AddFolder), true, false),
        ("Delete Layer", Some(LayerEvent::Delete(layer)), true, false),
        ("Duplicate Layer", Some(LayerEvent::Duplicate(layer)), true, false),
        ("", None, true, false),
        ("Guide", Some(LayerEvent::Convert { id: layer, kind: toggled(LayerKind::Guide) }), convertible, kind == LayerKind::Guide),
        ("Add Classic Motion Guide", Some(LayerEvent::AddMotionGuide(layer)), matches!(kind, LayerKind::Normal | LayerKind::Guided | LayerKind::Masked), false),
        ("Mask", Some(LayerEvent::Convert { id: layer, kind: toggled(LayerKind::Mask) }), convertible, kind == LayerKind::Mask),
        ("Folder", Some(LayerEvent::Convert { id: layer, kind: toggled(LayerKind::Folder) }), convertible, kind == LayerKind::Folder),
        ("", None, true, false),
        ("Properties...", Some(LayerEvent::Properties(layer)), true, false),
    ];
    for (label, event, enabled, checked) in items {
        let Some(event) = event else {
            ui.separator();
            continue;
        };
        if ui.add_enabled(enabled, egui::Button::selectable(checked, label)).clicked() {
            events.push(event);
            ui.close();
        }
    }
}

fn drop_target(row: &egui::Response, painter: &Painter, layer: &LayerRow, p: &Palette, events: &mut Vec<LayerEvent>) {
    let Some(cursor) = row.ctx.input(|i| i.pointer.interact_pos()) else { return };
    let rel = (cursor.y - row.rect.top()) / row.rect.height().max(1.0);
    let placement = if layer.is_folder() && (0.25..0.75).contains(&rel) {
        DropPlacement::Inside
    } else if rel < 0.5 {
        DropPlacement::Before
    } else {
        DropPlacement::After
    };
    if row.dnd_hover_payload::<LayerDrag>().is_some_and(|d| d.0 != layer.id) {
        let stroke = Stroke::new(2.0, p.accent);
        match placement {
            DropPlacement::Before => {
                painter.hline(row.rect.x_range(), row.rect.top() + 1.0, stroke);
            }
            DropPlacement::After => {
                painter.hline(row.rect.x_range(), row.rect.bottom() - 1.0, stroke);
            }
            DropPlacement::Inside => {
                painter.rect_stroke(row.rect.shrink(1.0), 2.0, stroke, egui::StrokeKind::Inside);
            }
        }
    }
    if let Some(dragged) = row.dnd_release_payload::<LayerDrag>()
        && dragged.0 != layer.id
    {
        events.push(LayerEvent::Move { id: dragged.0, target: layer.id, placement });
    }
}

/// A standalone layer list: header with all-layer toggles, then rows.
///
/// For panels that show layers without a frame grid, such as a `CharCraft` puppet layer tree.
pub fn layers_list(ui: &mut Ui, model: &TimelineModel, view: &TimelineViewState) -> Vec<LayerEvent> {
    let id = Id::new("anim-layers-list");
    let width = ui.available_width();
    let (header, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    let mut events = draw_header(ui, header, model, id);
    let visible = model.visible_rows();
    let total = f32_from_usize(visible.len()) * view.row_height;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        let (area, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
        let first = crate::util::round_usize((viewport.top() / view.row_height).floor().max(0.0));
        let last = crate::util::round_usize((viewport.bottom() / view.row_height).ceil().max(0.0)).saturating_add(1).min(visible.len());
        events.extend(draw_rows(ui, id, area.x_range(), area.top(), model, view, &visible, (first, last)));
    });
    events
}
