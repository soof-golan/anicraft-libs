//! Timeline view state and the events the timeline widgets return.

use egui::Modifiers;
use serde::{Deserialize, Serialize};

use super::{
    geometry::{FrameSelection, MAX_FRAME_WIDTH, MIN_FRAME_WIDTH},
    model::LayerId,
};

/// The frame cell sizes of the Frame View menu (Animate: Tiny to Large).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FrameSize {
    Tiny,
    Small,
    #[default]
    Normal,
    Medium,
    Large,
}

impl FrameSize {
    /// Every size, smallest first.
    pub const ALL: [Self; 5] = [Self::Tiny, Self::Small, Self::Normal, Self::Medium, Self::Large];

    /// Width of one frame cell in points.
    #[must_use]
    pub const fn width(self) -> f32 {
        match self {
            Self::Tiny => 5.0,
            Self::Small => 8.0,
            Self::Normal => 12.0,
            Self::Medium => 18.0,
            Self::Large => 26.0,
        }
    }

    /// The menu label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tiny => "Tiny",
            Self::Small => "Small",
            Self::Normal => "Normal",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }
}

/// Height of a row when the Frame View menu's "Short" is on.
pub const ROW_HEIGHT_SHORT: f32 = 15.0;
/// Height of a row at the default size.
pub const ROW_HEIGHT_NORMAL: f32 = 22.0;
/// Height of a row when the cells show previews.
pub const ROW_HEIGHT_PREVIEW: f32 = 44.0;

/// What the keyframe cells show besides the keyframe mark (Frame View: Preview, Preview in Context).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PreviewMode {
    #[default]
    Off,
    /// Each keyframe's content, scaled to fill its cell.
    Preview,
    /// Each keyframe's content at the position it has on the stage.
    InContext,
}

/// Scroll, zoom and selection of the timeline: UI state the app stores beside the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
    /// The Frame View size preset the widths were derived from.
    pub frame_size: FrameSize,
    /// Frame View > Short: rows are shorter.
    pub short_rows: bool,
    /// Frame View > Tinted Frames: frame spans are tinted by their kind.
    pub tinted_frames: bool,
    pub preview: PreviewMode,
    /// Edit Multiple Frames: every frame between the onion markers can be edited at once.
    pub edit_multiple: bool,
    /// Auto keyframe: changing a held frame makes it a keyframe first.
    pub auto_keyframe: bool,
}

impl Default for TimelineViewState {
    fn default() -> Self {
        Self {
            scroll_x: 0.0,
            scroll_y: 0.0,
            frame_width: FrameSize::Normal.width(),
            row_height: ROW_HEIGHT_NORMAL,
            layer_column_width: 230.0,
            selection: FrameSelection::default(),
            selected_layers: Vec::new(),
            show_waveforms: true,
            follow_playhead: true,
            frame_size: FrameSize::Normal,
            short_rows: false,
            tinted_frames: true,
            preview: PreviewMode::Off,
            edit_multiple: false,
            auto_keyframe: true,
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
            TimelineEvent::SetFrameView { size, short, tinted, preview } => {
                self.frame_size = *size;
                self.frame_width = size.width();
                self.short_rows = *short;
                self.row_height = match (*preview, *short) {
                    (PreviewMode::Off, true) => ROW_HEIGHT_SHORT,
                    (PreviewMode::Off, false) => ROW_HEIGHT_NORMAL,
                    (_, _) => ROW_HEIGHT_PREVIEW,
                };
                self.tinted_frames = *tinted;
                self.preview = *preview;
            }
            TimelineEvent::ToggleEditMultiple => self.edit_multiple = !self.edit_multiple,
            TimelineEvent::ToggleAutoKeyframe => self.auto_keyframe = !self.auto_keyframe,
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
    /// Paste and Overwrite Frames: the copied frames replace what is there instead of pushing it right.
    PasteOverwrite,
    ClearFrames,
    SelectAllFrames,
    /// Frame properties (label, tween, sound, script) of the selected keyframe.
    Properties,
    /// Add or edit the label (name, comment or anchor) of the selected keyframe.
    EditLabel,
}

/// Where a dragged layer lands relative to the row it is dropped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DropPlacement {
    Before,
    After,
    /// Into the folder dropped on.
    Inside,
}

/// An interaction with the layer column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// A new guide or mask layer (Insert > Timeline, the layer menu).
    AddKind(super::model::LayerKind),
    /// A classic motion guide layer for this layer: a guide layer is added above it and the layer is guided by it.
    AddMotionGuide(LayerId),
    /// Change the type of this layer: normal, guide, mask or folder.
    Convert {
        id: LayerId,
        kind: super::model::LayerKind,
    },
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// "Add Marker" in the ruler's context menu, for the frame that was right-clicked.
    MarkerAdd {
        frame: u32,
    },
    /// A marker's flag was dragged to this frame (reported each time the frame changes while dragging).
    MarkerMoved {
        id: u64,
        frame: u32,
    },
    /// "Rename" in a marker's context menu: the app shows its own editor.
    MarkerRename {
        id: u64,
    },
    /// "Delete" in a marker's context menu.
    MarkerRemove {
        id: u64,
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
    /// Edit Multiple Frames was toggled.
    ToggleEditMultiple,
    /// Auto keyframe was toggled.
    ToggleAutoKeyframe,
    /// A choice in the Frame View menu.
    SetFrameView {
        size: FrameSize,
        short: bool,
        tinted: bool,
        preview: PreviewMode,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_view_choices_set_widths_and_row_heights() {
        let mut s = TimelineViewState::default();
        s.apply(&TimelineEvent::SetFrameView { size: FrameSize::Large, short: true, tinted: false, preview: PreviewMode::Off });
        assert!((s.frame_width - FrameSize::Large.width()).abs() < 1e-6);
        assert!((s.row_height - ROW_HEIGHT_SHORT).abs() < 1e-6);
        assert!(!s.tinted_frames);
        s.apply(&TimelineEvent::SetFrameView { size: FrameSize::Tiny, short: false, tinted: true, preview: PreviewMode::Preview });
        assert!((s.frame_width - FrameSize::Tiny.width()).abs() < 1e-6);
        assert!((s.row_height - ROW_HEIGHT_PREVIEW).abs() < 1e-6, "previews need taller rows");
        s.apply(&TimelineEvent::SetFrameView { size: FrameSize::Normal, short: false, tinted: true, preview: PreviewMode::Off });
        assert!((s.row_height - ROW_HEIGHT_NORMAL).abs() < 1e-6);
    }

    #[test]
    fn mode_toggles_flip_view_flags() {
        let mut s = TimelineViewState::default();
        assert!(s.auto_keyframe && !s.edit_multiple);
        s.apply(&TimelineEvent::ToggleEditMultiple);
        s.apply(&TimelineEvent::ToggleAutoKeyframe);
        assert!(s.edit_multiple && !s.auto_keyframe);
    }

    #[test]
    fn old_saved_state_without_the_new_fields_still_loads() {
        let s: TimelineViewState = serde_json::from_str(r#"{"scroll_x":3.0,"frame_width":9.0}"#).expect("defaults fill the rest");
        assert!((s.scroll_x - 3.0).abs() < 1e-6 && (s.frame_width - 9.0).abs() < 1e-6);
        assert!(s.auto_keyframe);
    }

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
