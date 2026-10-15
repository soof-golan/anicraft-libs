//! Timeline view state and the events the timeline widgets return.

use egui::Modifiers;
use serde::{Deserialize, Serialize};

use super::{
    geometry::{FrameSelection, MAX_FRAME_WIDTH, MIN_FRAME_WIDTH},
    model::LayerId,
};

/// Scroll, zoom and selection of the timeline: UI state the app stores beside the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineViewState {
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// Width of one frame cell in points (the timeline zoom).
    pub frame_width: f32,
    pub row_height: f32,
    pub layer_column_width: f32,
    pub selection: FrameSelection,
    /// Layers highlighted in the layer column (the first is the active layer).
    pub selected_layers: Vec<LayerId>,
    /// Draw sound waveforms inside spans that have them.
    pub show_waveforms: bool,
    /// Keep the playhead in view while playing.
    pub follow_playhead: bool,
}

impl Default for TimelineViewState {
    fn default() -> Self {
        Self {
            scroll_x: 0.0,
            scroll_y: 0.0,
            frame_width: 12.0,
            row_height: 22.0,
            layer_column_width: 230.0,
            selection: FrameSelection::default(),
            selected_layers: Vec::new(),
            show_waveforms: true,
            follow_playhead: true,
        }
    }
}

impl TimelineViewState {
    /// The state after applying a [`TimelineEvent`] that only concerns view state (scroll, zoom, selection, layer
    /// selection). Other events are ignored; the app handles those on its document.
    pub fn apply(&mut self, event: &TimelineEvent) {
        match event {
            TimelineEvent::Scroll { x, y } => {
                self.scroll_x = x.max(0.0);
                self.scroll_y = y.max(0.0);
            }
            TimelineEvent::Zoom { frame_width, scroll_x } => {
                self.frame_width = frame_width.clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
                self.scroll_x = scroll_x.max(0.0);
            }
            TimelineEvent::SelectFrames(sel) => self.selection = sel.clone(),
            TimelineEvent::Layer(LayerEvent::Select { id, modifiers }) => {
                if modifiers.command || modifiers.ctrl {
                    if let Some(pos) = self.selected_layers.iter().position(|l| l == id) {
                        self.selected_layers.remove(pos);
                    } else {
                        self.selected_layers.push(*id);
                    }
                } else {
                    self.selected_layers = vec![*id];
                }
            }
            _ => {}
        }
    }
}

/// An edit on selected frames, from the frame context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FrameAction {
    InsertFrame,
    RemoveFrames,
    InsertKeyframe,
    InsertBlankKeyframe,
    ClearKeyframe,
    CreateClassicTween,
    CreateShapeTween,
    CreateMotionTween,
    RemoveTween,
    ConvertToKeyframes,
    ConvertToBlankKeyframes,
    ReverseFrames,
    CutFrames,
    CopyFrames,
    PasteFrames,
    ClearFrames,
    SelectAllFrames,
}

/// Where a dragged layer lands relative to the row it is dropped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPlacement {
    Before,
    After,
    /// Into the folder dropped on.
    Inside,
}

/// An interaction with the layer column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerEvent {
    Select {
        id: LayerId,
        modifiers: Modifiers,
    },
    ToggleVisible(LayerId),
    ToggleLocked(LayerId),
    ToggleOutline(LayerId),
    /// Expand or collapse a folder.
    ToggleFolder(LayerId),
    /// The name was double-clicked: start renaming.
    RenameRequested(LayerId),
    /// Show or hide every layer.
    SetAllVisible(bool),
    SetAllLocked(bool),
    SetAllOutline(bool),
    /// The "new layer" button or menu item (inserted above the active layer).
    Add,
    AddFolder,
    Delete(LayerId),
    Duplicate(LayerId),
    Properties(LayerId),
    /// A layer was dragged onto another row.
    Move {
        id: LayerId,
        target: LayerId,
        placement: DropPlacement,
    },
}

/// Everything the timeline reports.
#[derive(Debug, Clone, PartialEq)]
pub enum TimelineEvent {
    /// The playhead was scrubbed or a frame was clicked.
    SetFrame(u32),
    /// The frame selection changed.
    SelectFrames(FrameSelection),
    /// Selected frames were dragged: move them by this many frames and layer rows.
    MoveFrames {
        selection: FrameSelection,
        delta_frames: i32,
        delta_rows: i32,
    },
    /// A command from the frame context menu, to apply to the current selection.
    FrameAction(FrameAction),
    Layer(LayerEvent),
    /// New absolute scroll offsets.
    Scroll {
        x: f32,
        y: f32,
    },
    /// New frame width and the scroll offset that keeps the view anchored.
    Zoom {
        frame_width: f32,
        scroll_x: f32,
    },
    /// The onion skin markers were dragged.
    SetOnionRange {
        before: u32,
        after: u32,
    },
    /// The onion skin button was pressed.
    ToggleOnion,
    /// Onion skin outlines were toggled.
    ToggleOnionOutlines,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_updates_view_state_only() {
        let mut s = TimelineViewState::default();
        s.apply(&TimelineEvent::Scroll { x: 40.0, y: -3.0 });
        assert!((s.scroll_x - 40.0).abs() < 1e-6 && s.scroll_y.abs() < 1e-6);
        s.apply(&TimelineEvent::Zoom { frame_width: 1000.0, scroll_x: 5.0 });
        assert!((s.frame_width - MAX_FRAME_WIDTH).abs() < 1e-6);
        s.apply(&TimelineEvent::SetFrame(9));
        s.apply(&TimelineEvent::Layer(LayerEvent::Select { id: 3, modifiers: Modifiers::NONE }));
        assert_eq!(s.selected_layers, vec![3]);
        s.apply(&TimelineEvent::Layer(LayerEvent::Select { id: 4, modifiers: Modifiers::COMMAND }));
        assert_eq!(s.selected_layers, vec![3, 4]);
        s.apply(&TimelineEvent::Layer(LayerEvent::Select { id: 3, modifiers: Modifiers::COMMAND }));
        assert_eq!(s.selected_layers, vec![4]);
    }
}
