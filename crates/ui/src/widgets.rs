//! Small shared building blocks.

use egui::{Response, Sense, Stroke, Ui, Vec2};

use crate::{
    icons::{Icon, paint},
    theme::current_palette,
};

/// A flat icon button. `active` shows the toggled-on state; a disabled button ignores clicks and draws dimmed.
pub fn icon_button(ui: &mut Ui, size: Vec2, icon: Icon, active: bool, enabled: bool, tooltip: &str) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let p = current_palette(ui.ctx());
        let painter = ui.painter();
        if active {
            painter.rect(rect, 3.0, p.accent_soft, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
        } else if response.hovered() && enabled {
            painter.rect_filled(rect, 3.0, p.grid_line_strong);
        }
        let color = if enabled { p.text } else { p.text_weak.gamma_multiply(0.5) };
        paint(painter, rect.shrink(size.min_elem() * 0.2), icon, color);
    }
    if tooltip.is_empty() { response } else { response.on_hover_text(crate::theme::visual(tooltip)) }
}
