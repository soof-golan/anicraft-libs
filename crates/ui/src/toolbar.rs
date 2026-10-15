//! The toolbar: a grid of tool buttons with code-drawn icons.

use egui::{Ui, vec2};
use serde::{Deserialize, Serialize};

use crate::{icons::Icon, widgets::icon_button};

/// One tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolItem {
    /// Stable identifier returned in [`ToolbarEvent::Select`].
    pub id: String,
    pub label: String,
    pub icon: Icon,
    /// Single-key shortcut shown in the tooltip.
    pub shortcut: Option<char>,
    pub enabled: bool,
    /// Draw a separator before this tool.
    pub separator_before: bool,
}

impl ToolItem {
    /// An enabled tool.
    #[must_use]
    pub fn new(id: &str, label: &str, icon: Icon, shortcut: Option<char>) -> Self {
        Self { id: id.to_owned(), label: label.to_owned(), icon, shortcut, enabled: true, separator_before: false }
    }

    /// The same tool preceded by a separator.
    #[must_use]
    pub const fn after_separator(mut self) -> Self {
        self.separator_before = true;
        self
    }

    fn tooltip(&self) -> String {
        self.shortcut.map_or_else(|| self.label.clone(), |c| format!("{} ({})", self.label, c.to_ascii_uppercase()))
    }
}

/// What the toolbar reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolbarEvent {
    Select(String),
}

/// Animate-style drawing and editing tools.
#[must_use]
pub fn animate_tools() -> Vec<ToolItem> {
    vec![
        ToolItem::new("select", "Selection Tool", Icon::Select, Some('v')),
        ToolItem::new("subselect", "Subselection Tool", Icon::Subselect, Some('a')),
        ToolItem::new("transform", "Free Transform Tool", Icon::FreeTransform, Some('q')),
        ToolItem::new("lasso", "Lasso Tool", Icon::Lasso, Some('l')),
        ToolItem::new("pen", "Pen Tool", Icon::Pen, Some('p')).after_separator(),
        ToolItem::new("text", "Text Tool", Icon::Text, Some('t')),
        ToolItem::new("line", "Line Tool", Icon::Line, Some('n')),
        ToolItem::new("rect", "Rectangle Tool", Icon::Rectangle, Some('r')),
        ToolItem::new("oval", "Oval Tool", Icon::Oval, Some('o')),
        ToolItem::new("pencil", "Pencil Tool", Icon::Pencil, Some('y')),
        ToolItem::new("brush", "Brush Tool", Icon::Brush, Some('b')),
        ToolItem::new("bone", "Bone Tool", Icon::Bone, Some('m')).after_separator(),
        ToolItem::new("bucket", "Paint Bucket Tool", Icon::PaintBucket, Some('k')),
        ToolItem::new("eyedropper", "Eyedropper Tool", Icon::Eyedropper, Some('i')),
        ToolItem::new("eraser", "Eraser Tool", Icon::Eraser, Some('e')),
        ToolItem::new("camera", "Camera Tool", Icon::Camera, Some('c')).after_separator(),
        ToolItem::new("hand", "Hand Tool", Icon::Hand, Some('h')),
        ToolItem::new("zoom", "Zoom Tool", Icon::Zoom, Some('z')),
    ]
}

/// Tools for rigging and performing a puppet.
#[must_use]
pub fn puppet_tools() -> Vec<ToolItem> {
    vec![
        ToolItem::new("select", "Select", Icon::Select, Some('v')),
        ToolItem::new("pin", "Add Pin", Icon::Pin, Some('p')),
        ToolItem::new("bone", "Stick", Icon::Bone, Some('s')),
        ToolItem::new("transform", "Transform", Icon::FreeTransform, Some('t')),
        ToolItem::new("mic", "Record", Icon::Mic, Some('r')).after_separator(),
        ToolItem::new("hand", "Hand", Icon::Hand, Some('h')).after_separator(),
        ToolItem::new("zoom", "Zoom", Icon::Zoom, Some('z')),
    ]
}

/// Draws the tools in a wrapping grid sized to the available width and returns the selection events.
pub fn toolbar(ui: &mut Ui, tools: &[ToolItem], selected: &str) -> Vec<ToolbarEvent> {
    let mut events = Vec::new();
    let button = vec2(30.0, 30.0);
    let spacing = ui.spacing().item_spacing.x;
    let per_row = columns(ui.available_width(), button.x, spacing);
    // Rows of at most `per_row` tools; a separator starts a new row after a gap.
    let mut rows: Vec<Vec<&ToolItem>> = Vec::new();
    let mut gap_before: Vec<bool> = Vec::new();
    for tool in tools {
        let start_new = tool.separator_before || rows.last().is_none_or(|r| r.len() >= per_row);
        if start_new {
            rows.push(Vec::new());
            gap_before.push(tool.separator_before && rows.len() > 1);
        }
        if let Some(row) = rows.last_mut() {
            row.push(tool);
        }
    }
    ui.spacing_mut().item_spacing = vec2(spacing, 3.0);
    for (row, gap) in rows.iter().zip(gap_before) {
        if gap {
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            for tool in row {
                if icon_button(ui, button, tool.icon, tool.id == selected, tool.enabled, &tool.tooltip()).clicked() {
                    events.push(ToolbarEvent::Select(tool.id.clone()));
                }
            }
        });
    }
    events
}

/// How many `button`-wide buttons fit in `width` with `spacing` between them (at least one).
#[must_use]
pub fn columns(width: f32, button: f32, spacing: f32) -> usize {
    if width <= button {
        return 1;
    }
    crate::util::round_usize(((width + spacing) / (button + spacing)).floor()).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_count() {
        assert_eq!(columns(10.0, 30.0, 4.0), 1);
        assert_eq!(columns(64.0, 30.0, 4.0), 2);
        assert_eq!(columns(100.0, 30.0, 4.0), 3);
    }

    #[test]
    fn tool_sets_have_unique_ids() {
        for set in [animate_tools(), puppet_tools()] {
            let mut ids: Vec<&str> = set.iter().map(|t| t.id.as_str()).collect();
            ids.sort_unstable();
            let n = ids.len();
            ids.dedup();
            assert_eq!(ids.len(), n);
        }
    }

    #[test]
    fn tooltips_show_shortcuts() {
        assert_eq!(ToolItem::new("a", "Arrow", Icon::Select, Some('v')).tooltip(), "Arrow (V)");
        assert_eq!(ToolItem::new("a", "Arrow", Icon::Select, None).tooltip(), "Arrow");
    }
}
