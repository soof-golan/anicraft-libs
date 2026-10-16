//! The library panel: a tree of items with thumbnails, a search field and drag sources for the stage.

use egui::{Align2, FontId, Id, Rect, Sense, Stroke, TextureId, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::{
    icons::{Icon, paint},
    stage::LibraryDrag,
    theme::{current_palette, visual},
    util::{f32_from_u32, f32_from_usize},
};

/// What a library item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryKind {
    Folder,
    Graphic,
    MovieClip,
    Button,
    Bitmap,
    Sound,
    Video,
    Font,
}

impl LibraryKind {
    /// The icon drawn when an item has no thumbnail.
    #[must_use]
    pub const fn icon(self) -> Icon {
        match self {
            Self::Folder => Icon::Folder,
            Self::Graphic => Icon::Graphic,
            Self::MovieClip => Icon::MovieClip,
            Self::Button => Icon::Button,
            Self::Bitmap => Icon::Bitmap,
            Self::Sound => Icon::Sound,
            Self::Video => Icon::Video,
            Self::Font => Icon::Font,
        }
    }
}

/// One row of the library tree (listed in tree order, children after their folder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryItemView {
    pub id: u64,
    pub name: String,
    pub kind: LibraryKind,
    pub depth: u32,
    /// For folders: whether the children are shown.
    pub expanded: bool,
    /// A texture for the preview and the row thumbnail, if the app has rendered one.
    pub thumbnail: Option<TextureId>,
}

/// What the preview box above the list shows for the selected item. The app renders the pixels (a symbol's frame, a
/// bitmap) and the peaks of a sound; the panel only lays them out and draws the transport.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LibraryPreview {
    /// The rendered picture, if the item has one.
    pub texture: Option<TextureId>,
    /// Pixel size of `texture`, so the picture keeps its aspect ratio.
    pub texture_size: [u32; 2],
    /// Waveform peaks of a sound, one byte (0..=255) per bin, left to right. Empty for everything else.
    pub waveform: Vec<u8>,
    /// Movie clips, graphics and sounds can be played.
    pub playable: bool,
    pub playing: bool,
    /// How far playback is, 0..=1000.
    pub progress_permille: u16,
    /// One line about the item: kind, size, duration.
    pub caption: String,
}

/// The library panel's input.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LibraryModel {
    pub items: Vec<LibraryItemView>,
    pub selected: Option<u64>,
    /// Current search text; when not empty only matching items are listed, flat.
    pub filter: String,
    /// The preview of the selected item.
    pub preview: LibraryPreview,
}

/// Commands from the item context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryAction {
    Edit,
    Rename,
    Duplicate,
    Delete,
    Properties,
}

/// What the panel reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryEvent {
    Select(u64),
    /// Double-click: open the item for editing (symbols) or playback (sounds).
    Open(u64),
    ToggleFolder(u64),
    FilterChanged(String),
    Action {
        id: u64,
        action: LibraryAction,
    },
    NewSymbol,
    NewFolder,
    /// The preview's play button was pressed.
    PreviewPlay,
    /// The preview's stop button was pressed.
    PreviewStop,
    /// The progress bar or waveform was clicked: jump to this point (0..=1000).
    PreviewSeek(u16),
}

/// Height of the preview box.
pub const PREVIEW_HEIGHT: f32 = 150.0;

/// Row height of the list.
pub const ROW_HEIGHT: f32 = 22.0;

/// Indices of the items to list: respects collapsed folders, or filters by name (case-insensitive) when
/// `model.filter` is not empty.
#[must_use]
pub fn visible_items(model: &LibraryModel) -> Vec<usize> {
    let needle = model.filter.trim().to_lowercase();
    if !needle.is_empty() {
        return model
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.kind != LibraryKind::Folder && it.name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
    }
    let mut out = Vec::with_capacity(model.items.len());
    let mut hidden_below: Option<u32> = None;
    for (i, it) in model.items.iter().enumerate() {
        if let Some(depth) = hidden_below {
            if it.depth > depth {
                continue;
            }
            hidden_below = None;
        }
        out.push(i);
        if it.kind == LibraryKind::Folder && !it.expanded {
            hidden_below = Some(it.depth);
        }
    }
    out
}

/// Draws the library panel.
pub fn library_panel(ui: &mut Ui, model: &LibraryModel) -> Vec<LibraryEvent> {
    let p = current_palette(ui.ctx());
    let mut events = Vec::new();
    let id = Id::new("anim-library");

    events.extend(preview(ui, model));

    ui.horizontal(|ui| {
        let mut filter = model.filter.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search").desired_width(ui.available_width() - 56.0));
        if r.changed() {
            events.push(LibraryEvent::FilterChanged(filter));
        }
        if crate::widgets::icon_button(ui, vec2(22.0, 22.0), Icon::Graphic, false, true, "New symbol").clicked() {
            events.push(LibraryEvent::NewSymbol);
        }
        if crate::widgets::icon_button(ui, vec2(22.0, 22.0), Icon::Folder, false, true, "New folder").clicked() {
            events.push(LibraryEvent::NewFolder);
        }
    });
    ui.add_space(2.0);

    let visible = visible_items(model);
    ui.label(egui::RichText::new(format!("{} items", model.items.len())).size(10.0).color(p.text_weak));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_HEIGHT, visible.len(), |ui, range| {
        for row in range {
            let Some(item) = visible.get(row).and_then(|&i| model.items.get(i)) else { continue };
            row_ui(ui, id, item, model.selected == Some(item.id), model.filter.is_empty(), &mut events);
        }
    });
    events
}

/// The largest rectangle of the aspect `size` that fits in `bounds`, centred.
fn fit(bounds: Rect, size: [u32; 2]) -> Rect {
    let (w, h) = (f32_from_u32(size[0]).max(1.0), f32_from_u32(size[1]).max(1.0));
    let scale = (bounds.width() / w).min(bounds.height() / h);
    Rect::from_center_size(bounds.center(), vec2(w * scale, h * scale))
}

/// The box above the list: the picture or the waveform, the play controls and one line about the item.
fn preview(ui: &mut Ui, model: &LibraryModel) -> Vec<LibraryEvent> {
    let p = current_palette(ui.ctx());
    let mut events = Vec::new();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PREVIEW_HEIGHT), Sense::hover());
    ui.painter().rect(rect, 3.0, p.recessed, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    let selected = model.selected.and_then(|id| model.items.iter().find(|i| i.id == id));
    let Some(item) = selected else {
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, "No item selected", FontId::proportional(11.0), p.text_weak);
        return events;
    };
    let pv = &model.preview;
    let controls = if pv.playable { 26.0 } else { 0.0 };
    let caption_h = 16.0;
    let picture = Rect::from_min_max(rect.min + vec2(6.0, 6.0), pos2(rect.right() - 6.0, rect.bottom() - controls - caption_h - 4.0));
    if !pv.waveform.is_empty() {
        draw_waveform(ui, picture, &pv.waveform, pv.progress_permille, pv.playing);
    } else if let Some(tex) = pv.texture.or(item.thumbnail) {
        let size = if pv.texture.is_some() { pv.texture_size } else { [1, 1] };
        // A checkerboard-free plain backdrop so transparent artwork stays readable on both themes.
        let area = fit(picture, size);
        ui.painter().rect_filled(area, 2.0, p.panel);
        ui.painter().image(tex, area, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
    } else {
        paint(ui.painter(), Rect::from_center_size(picture.center(), vec2(40.0, 40.0)), item.kind.icon(), p.text_weak);
    }
    if pv.playable {
        let bar = Rect::from_min_max(pos2(rect.left() + 6.0, rect.bottom() - controls - caption_h), pos2(rect.right() - 6.0, rect.bottom() - caption_h - 2.0));
        let button = Rect::from_min_size(bar.min, vec2(bar.height(), bar.height()));
        let icon = if pv.playing { Icon::Pause } else { Icon::Play };
        let clicked = ui
            .put(button, |ui: &mut Ui| crate::widgets::icon_button(ui, button.size(), icon, pv.playing, true, if pv.playing { "Pause" } else { "Play" }))
            .clicked();
        if clicked {
            events.push(if pv.playing { LibraryEvent::PreviewStop } else { LibraryEvent::PreviewPlay });
        }
        let track = Rect::from_min_max(pos2(button.right() + 8.0, bar.center().y - 3.0), pos2(bar.right(), bar.center().y + 3.0));
        let hit = ui.interact(track.expand2(vec2(0.0, 6.0)), Id::new("anim-library-seek"), Sense::click_and_drag());
        ui.painter().rect_filled(track, 3.0, p.grid_line_strong);
        let filled = Rect::from_min_max(track.min, pos2(track.left() + track.width() * f32::from(pv.progress_permille.min(1000)) / 1000.0, track.bottom()));
        ui.painter().rect_filled(filled, 3.0, p.accent);
        if (hit.clicked() || hit.dragged())
            && let Some(pos) = hit.interact_pointer_pos()
        {
            events.push(LibraryEvent::PreviewSeek(permille((pos.x - track.left()) / track.width().max(1.0))));
        }
    }
    let caption = if pv.caption.is_empty() { visual(&item.name) } else { visual(&pv.caption) };
    ui.painter().with_clip_rect(rect).text(
        pos2(rect.left() + 8.0, caption_h.mul_add(-0.5, rect.bottom()) - 3.0),
        Align2::LEFT_CENTER,
        caption,
        FontId::proportional(10.5),
        p.text_weak,
    );
    events
}

/// 0..=1 as 0..=1000, clamped.
fn permille(fraction: f32) -> u16 {
    u16::try_from(anim_core::num::round_f64_to_u32(f64::from(fraction.clamp(0.0, 1.0)) * 1000.0)).unwrap_or(1000)
}

/// Peaks as vertical bars about the middle line; the part already played is drawn in the accent colour.
fn draw_waveform(ui: &Ui, area: Rect, peaks: &[u8], progress_permille: u16, playing: bool) {
    let p = current_palette(ui.ctx());
    let painter = ui.painter();
    painter.rect_filled(area, 2.0, p.panel);
    painter.line_segment([pos2(area.left(), area.center().y), pos2(area.right(), area.center().y)], Stroke::new(1.0, p.grid_line_strong));
    let count = peaks.len().max(1);
    let step = area.width() / f32_from_usize(count);
    let played = f32::from(progress_permille.min(1000)) / 1000.0;
    for (i, peak) in peaks.iter().enumerate() {
        let x = step.mul_add(f32_from_usize(i) + 0.5, area.left());
        let half = area.height().mul_add(0.5, -2.0) * f32::from(*peak) / 255.0;
        let half = half.max(0.5);
        let colour = if (playing || progress_permille > 0) && f32_from_usize(i) / f32_from_usize(count) <= played { p.accent } else { p.text_weak };
        painter.line_segment([pos2(x, area.center().y - half), pos2(x, area.center().y + half)], Stroke::new((step * 0.7).clamp(1.0, 3.0), colour));
    }
}

fn row_ui(ui: &mut Ui, id: Id, item: &LibraryItemView, selected: bool, tree: bool, events: &mut Vec<LibraryEvent>) {
    let p = current_palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let row = ui.interact(rect, id.with(("row", item.id)), Sense::click_and_drag());
    if selected {
        ui.painter().rect_filled(rect, 2.0, p.accent_soft);
    } else if row.hovered() {
        ui.painter().rect_filled(rect, 2.0, p.grid_line_strong.gamma_multiply(0.5));
    }
    if row.clicked() {
        events.push(LibraryEvent::Select(item.id));
    }
    if row.double_clicked() {
        events.push(if item.kind == LibraryKind::Folder { LibraryEvent::ToggleFolder(item.id) } else { LibraryEvent::Open(item.id) });
    }
    if item.kind != LibraryKind::Folder {
        row.dnd_set_drag_payload(LibraryDrag { id: item.id });
        if row.dragged() {
            events.push(LibraryEvent::Select(item.id));
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("anim-library-drag"));
                ui.ctx().layer_painter(layer).text(pos + vec2(12.0, 12.0), Align2::LEFT_TOP, visual(&item.name), FontId::proportional(11.0), p.text);
            }
        }
    }
    let item_id = item.id;
    row.context_menu(|ui| {
        for (label, action) in [
            ("Edit", LibraryAction::Edit),
            ("Rename", LibraryAction::Rename),
            ("Duplicate", LibraryAction::Duplicate),
            ("Delete", LibraryAction::Delete),
            ("Properties...", LibraryAction::Properties),
        ] {
            if ui.button(label).clicked() {
                events.push(LibraryEvent::Action { id: item_id, action });
                ui.close();
            }
        }
    });
    let mut x = rect.left() + 4.0 + if tree { 14.0 * f32_from_u32(item.depth) } else { 0.0 };
    if item.kind == LibraryKind::Folder {
        let chev = Rect::from_min_size(pos2(x, rect.top()), vec2(14.0, rect.height()));
        let r = ui.interact(chev, id.with(("chev", item.id)), Sense::click());
        paint(ui.painter(), chev.shrink2(vec2(2.0, 6.0)), if item.expanded { Icon::ChevronDown } else { Icon::ChevronRight }, p.text_weak);
        if r.clicked() {
            events.push(LibraryEvent::ToggleFolder(item.id));
        }
    }
    x += 16.0;
    let icon_rect = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(ROW_HEIGHT - 4.0, ROW_HEIGHT - 4.0));
    match item.thumbnail {
        Some(tex) if item.kind != LibraryKind::Folder => {
            ui.painter().image(tex, icon_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        _ => paint(ui.painter(), icon_rect, item.kind.icon(), p.text_weak),
    }
    ui.painter().with_clip_rect(rect).text(
        pos2(icon_rect.right() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        visual(&item.name),
        FontId::proportional(12.0),
        p.text,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u64, name: &str, kind: LibraryKind, depth: u32, expanded: bool) -> LibraryItemView {
        LibraryItemView { id, name: name.into(), kind, depth, expanded, thumbnail: None }
    }

    fn model() -> LibraryModel {
        LibraryModel {
            items: vec![
                item(1, "Characters", LibraryKind::Folder, 0, false),
                item(2, "Hero", LibraryKind::MovieClip, 1, false),
                item(3, "Sounds", LibraryKind::Folder, 0, true),
                item(4, "Boom", LibraryKind::Sound, 1, false),
                item(5, "hero_sprite.png", LibraryKind::Bitmap, 0, false),
            ],
            selected: None,
            filter: String::new(),
            preview: LibraryPreview::default(),
        }
    }

    #[test]
    fn fit_keeps_the_aspect_ratio_and_centres() {
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0));
        let wide = fit(bounds, [400, 100]);
        assert!((wide.width() - 200.0).abs() < 1e-3 && (wide.height() - 50.0).abs() < 1e-3);
        assert!((wide.center().y - 50.0).abs() < 1e-3);
        let tall = fit(bounds, [10, 100]);
        assert!((tall.height() - 100.0).abs() < 1e-3 && (tall.width() - 10.0).abs() < 1e-3);
    }

    #[test]
    fn seek_positions_are_per_mille() {
        assert_eq!(permille(0.5), 500);
        assert_eq!(permille(-1.0), 0);
        assert_eq!(permille(7.0), 1000);
    }

    #[test]
    fn collapsed_folders_hide_children() {
        assert_eq!(visible_items(&model()), vec![0, 2, 3, 4]);
    }

    #[test]
    fn filter_is_flat_and_case_insensitive() {
        let mut m = model();
        m.filter = "HERO".into();
        assert_eq!(visible_items(&m), vec![1, 4]);
        m.filter = "  ".into();
        assert_eq!(visible_items(&m).len(), 4);
    }
}
