//! The property grid: label and editor rows for numbers (drag), colours, choices, text and checkboxes.

use anim_core::Rgba8;
use egui::{Align, Id, Ui};
use serde::{Deserialize, Serialize};

use crate::{
    theme::{is_rtl, visual},
    util::{color32, rgba8},
};

/// An editable value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropValue {
    Number { value: f64, min: f64, max: f64, speed: f64, decimals: usize, suffix: String },
    Color(Rgba8),
    Choice { selected: usize, options: Vec<String> },
    Text(String),
    Bool(bool),
}

impl PropValue {
    /// A number with sensible defaults (any range, drag speed 1, no decimals).
    #[must_use]
    pub const fn number(value: f64) -> Self {
        Self::Number { value, min: f64::MIN, max: f64::MAX, speed: 1.0, decimals: 0, suffix: String::new() }
    }

    /// A number clamped to `min..=max`.
    #[must_use]
    pub const fn number_in(value: f64, min: f64, max: f64) -> Self {
        Self::Number { value, min, max, speed: 1.0, decimals: 0, suffix: String::new() }
    }

    /// The same number with `decimals` fixed decimals and a drag `speed`.
    #[must_use]
    pub fn precision(self, decimals: usize, speed: f64) -> Self {
        match self {
            Self::Number { value, min, max, suffix, .. } => Self::Number { value, min, max, speed, decimals, suffix },
            other => other,
        }
    }

    /// The same number with a unit suffix such as `" px"` or `"°"`.
    #[must_use]
    pub fn with_suffix(self, text: &str) -> Self {
        match self {
            Self::Number { value, min, max, speed, decimals, .. } => Self::Number { value, min, max, speed, decimals, suffix: text.to_owned() },
            other => other,
        }
    }

    /// A choice among `options`.
    #[must_use]
    pub fn choice(selected: usize, options: &[&str]) -> Self {
        Self::Choice { selected, options: options.iter().map(|s| (*s).to_owned()).collect() }
    }
}

/// One row of the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    /// Stable identifier returned in [`PropertyEdit`]s.
    pub id: String,
    pub label: String,
    pub value: PropValue,
    pub enabled: bool,
    /// Rows with the same section are grouped under a collapsible header, in order of appearance.
    pub section: Option<String>,
    pub tooltip: Option<String>,
}

impl Property {
    /// An enabled row without section or tooltip.
    #[must_use]
    pub fn new(id: &str, label: &str, value: PropValue) -> Self {
        Self { id: id.to_owned(), label: label.to_owned(), value, enabled: true, section: None, tooltip: None }
    }

    /// The same row placed in `section`.
    #[must_use]
    pub fn in_section(mut self, section: &str) -> Self {
        self.section = Some(section.to_owned());
        self
    }
}

/// An edit made in the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyEdit {
    pub id: String,
    pub value: PropValue,
    /// `false` while a drag or a colour pick is in progress (apply it without an undo step, or merge steps); `true`
    /// for the final value. A commit may repeat the last live value.
    pub commit: bool,
}

/// Draws the grid and returns the edits made this frame.
pub fn property_grid(ui: &mut Ui, props: &[Property]) -> Vec<PropertyEdit> {
    let mut edits = Vec::new();
    let mut start = 0usize;
    while start < props.len() {
        let section = props.get(start).and_then(|p| p.section.as_deref());
        let len = props.iter().skip(start).take_while(|p| p.section.as_deref() == section).count();
        let group = props.get(start..start.saturating_add(len)).unwrap_or(&[]);
        match section {
            Some(title) => {
                egui::CollapsingHeader::new(egui::RichText::new(visual(title)).strong()).id_salt(("anim-props-section", title)).default_open(true).show(
                    ui,
                    |ui| {
                        grid_rows(ui, group, title, &mut edits);
                    },
                );
            }
            None => grid_rows(ui, group, "", &mut edits),
        }
        start = start.saturating_add(len.max(1));
    }
    edits
}

fn grid_rows(ui: &mut Ui, group: &[Property], salt: &str, edits: &mut Vec<PropertyEdit>) {
    egui::Grid::new(Id::new(("anim-props-grid", salt))).num_columns(2).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
        for prop in group {
            let label = ui.label(visual(&prop.label));
            if let Some(tip) = &prop.tooltip {
                label.on_hover_text(visual(tip));
            }
            ui.add_enabled_ui(prop.enabled, |ui| edit_row(ui, prop, edits));
            ui.end_row();
        }
    });
}

fn edit_row(ui: &mut Ui, prop: &Property, edits: &mut Vec<PropertyEdit>) {
    let id = ui.id().with(("prop", &prop.id));
    let push = |edits: &mut Vec<PropertyEdit>, value: PropValue, commit: bool| edits.push(PropertyEdit { id: prop.id.clone(), value, commit });
    match &prop.value {
        PropValue::Number { value, min, max, speed, decimals, suffix } => {
            let mut v = *value;
            let r = ui.add(egui::DragValue::new(&mut v).range(*min..=*max).speed(*speed).fixed_decimals(*decimals).suffix(suffix.clone()));
            if r.changed() || r.drag_stopped() || r.lost_focus() {
                let commit = r.drag_stopped() || r.lost_focus() || (!r.dragged() && !r.has_focus());
                push(edits, PropValue::Number { value: v, min: *min, max: *max, speed: *speed, decimals: *decimals, suffix: suffix.clone() }, commit);
            }
        }
        PropValue::Color(c) => {
            let mut color = color32(*c);
            let r = egui::color_picker::color_edit_button_srgba(ui, &mut color, egui::color_picker::Alpha::OnlyBlend);
            let pending: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(id, true));
                push(edits, PropValue::Color(rgba8(color)), false);
            } else if pending && !ui.input(|i| i.pointer.any_down()) {
                ui.data_mut(|d| d.insert_temp(id, false));
                push(edits, PropValue::Color(*c), true);
            }
        }
        PropValue::Choice { selected, options } => {
            let mut sel = *selected;
            let current = options.get(sel).map_or_else(String::new, |s| visual(s).into_owned());
            egui::ComboBox::from_id_salt(id).selected_text(current).show_ui(ui, |ui| {
                for (i, option) in options.iter().enumerate() {
                    ui.selectable_value(&mut sel, i, visual(option));
                }
            });
            if sel != *selected {
                push(edits, PropValue::Choice { selected: sel, options: options.clone() }, true);
            }
        }
        PropValue::Text(text) => {
            let mut s = text.clone();
            let rtl = is_rtl(&s);
            let align = if rtl { Align::RIGHT } else { Align::LEFT };
            let width = ui.available_width().min(160.0);
            let r = if rtl {
                // egui shapes left to right: show the text in visual order. The caret still follows logical order.
                let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    ui.painter().layout(visual(text.as_str()).into_owned(), font, ui.visuals().text_color(), wrap)
                };
                ui.add(egui::TextEdit::singleline(&mut s).horizontal_align(align).desired_width(width).layouter(&mut layouter))
            } else {
                ui.add(egui::TextEdit::singleline(&mut s).horizontal_align(align).desired_width(width))
            };
            if r.changed() || r.lost_focus() {
                push(edits, PropValue::Text(s), r.lost_focus());
            }
        }
        PropValue::Bool(b) => {
            let mut v = *b;
            if ui.checkbox(&mut v, "").changed() {
                push(edits, PropValue::Bool(v), true);
            }
        }
    }
}

/// Applies an edit to a property list in place (a convenience for apps that keep their inspector rows as state).
pub fn apply_edit(props: &mut [Property], edit: &PropertyEdit) {
    if let Some(p) = props.iter_mut().find(|p| p.id == edit.id) {
        p.value = edit.value.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders() {
        let n = PropValue::number_in(5.0, 0.0, 10.0).precision(2, 0.1).with_suffix(" px");
        assert_eq!(n, PropValue::Number { value: 5.0, min: 0.0, max: 10.0, speed: 0.1, decimals: 2, suffix: " px".into() });
        assert_eq!(PropValue::choice(1, &["a", "b"]), PropValue::Choice { selected: 1, options: vec!["a".into(), "b".into()] });
    }

    #[test]
    fn apply_edit_replaces_value_by_id() {
        let mut props = vec![Property::new("x", "X", PropValue::number(1.0)), Property::new("y", "Y", PropValue::number(2.0))];
        apply_edit(&mut props, &PropertyEdit { id: "y".into(), value: PropValue::number(9.0), commit: true });
        assert_eq!(props.get(1).map(|p| p.value.clone()), Some(PropValue::number(9.0)));
        assert_eq!(props.first().map(|p| p.value.clone()), Some(PropValue::number(1.0)));
    }
}
