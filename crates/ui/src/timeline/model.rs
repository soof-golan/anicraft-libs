//! The plain view model the timeline draws. It knows nothing about the document model: the app builds it from
//! whatever it edits.

use anim_core::Rgba8;
use serde::{Deserialize, Serialize};

/// Stable identity of a layer row (the app chooses it; indices change when layers move).
pub type LayerId = u64;

/// What kind of layer a row is (decides its icon and whether it can hold frames).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LayerKind {
    #[default]
    Normal,
    Guide,
    Guided,
    Mask,
    Masked,
    Folder,
    Camera,
}

/// How a span is tweened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TweenKind {
    #[default]
    None,
    /// Classic tween: purple span with an arrow.
    Classic,
    /// Shape tween: green span with an arrow.
    Shape,
    /// Motion tween: blue span.
    Motion,
}

/// Peaks of a sound across a span (0.0 to 1.0, evenly spaced over the span's length).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Waveform {
    pub peaks: Vec<f32>,
}

/// What a frame label is: a name scripts can jump to, a comment (only shown), or a named anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LabelKind {
    #[default]
    Name,
    Comment,
    Anchor,
}

/// A box of a keyframe's content, in stage fractions (0 to 1 across the stage), for the cell previews.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PreviewBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Rgba8,
}

/// A run of frames that starts at a keyframe and lasts `len` frames (until the next keyframe).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// Index of the keyframe (0-based).
    pub start: u32,
    /// Number of frames covered, including the keyframe (at least 1).
    pub len: u32,
    /// A blank keyframe (no content): drawn as a hollow dot.
    pub blank: bool,
    pub tween: TweenKind,
    /// A tween whose end keyframe is missing or incompatible: drawn dashed.
    pub tween_broken: bool,
    pub label: Option<String>,
    /// How `label` is drawn.
    #[serde(default)]
    pub label_kind: LabelKind,
    pub has_script: bool,
    pub sound: Option<Waveform>,
    /// The content boxes the Frame View previews draw; empty for a blank keyframe.
    #[serde(default)]
    pub preview: Vec<PreviewBox>,
}

impl Span {
    /// A plain keyframe span.
    #[must_use]
    pub const fn key(start: u32, len: u32) -> Self {
        Self {
            start,
            len,
            blank: false,
            tween: TweenKind::None,
            tween_broken: false,
            label: None,
            label_kind: LabelKind::Name,
            has_script: false,
            sound: None,
            preview: Vec::new(),
        }
    }

    /// A blank keyframe span.
    #[must_use]
    pub fn blank(start: u32, len: u32) -> Self {
        Self { blank: true, ..Self::key(start, len) }
    }

    /// A tweened keyframe span.
    #[must_use]
    pub fn tweened(start: u32, len: u32, tween: TweenKind) -> Self {
        Self { tween, ..Self::key(start, len) }
    }

    /// One past the last frame the span covers.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.start.saturating_add(if self.len == 0 { 1 } else { self.len })
    }

    /// Whether `frame` falls inside the span.
    #[must_use]
    pub const fn contains(&self, frame: u32) -> bool {
        frame >= self.start && frame < self.end()
    }
}

/// One layer row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerRow {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,
    /// Nesting depth (0 for top-level layers and folders; children of a folder are one deeper).
    pub depth: u32,
    /// For folders: whether the children are shown.
    pub expanded: bool,
    /// The layer's outline colour (shown on the outline toggle and the selection box).
    pub color: Rgba8,
    pub visible: bool,
    pub locked: bool,
    pub outline: bool,
    /// Keyframe spans, sorted by `start`, non-overlapping.
    pub spans: Vec<Span>,
}

impl LayerRow {
    /// A normal, visible, unlocked layer with no frames.
    #[must_use]
    pub fn new(id: LayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: LayerKind::Normal,
            depth: 0,
            expanded: true,
            color: Rgba8 { r: 0x4f, g: 0x80, b: 0xff, a: 255 },
            visible: true,
            locked: false,
            outline: false,
            spans: Vec::new(),
        }
    }

    /// The span covering `frame`, if any.
    #[must_use]
    pub fn span_at(&self, frame: u32) -> Option<&Span> {
        // Spans are sorted: binary search for the last span starting at or before `frame`.
        let idx = self.spans.partition_point(|s| s.start <= frame);
        let span = self.spans.get(idx.checked_sub(1)?)?;
        span.contains(frame).then_some(span)
    }

    /// Whether the row is a folder.
    #[must_use]
    pub fn is_folder(&self) -> bool {
        self.kind == LayerKind::Folder
    }
}

/// Onion skin settings shown on the ruler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnionSkin {
    pub enabled: bool,
    pub outlines: bool,
    /// Frames shown before the playhead.
    pub before: u32,
    /// Frames shown after the playhead.
    pub after: u32,
}

impl Default for OnionSkin {
    fn default() -> Self {
        Self { enabled: false, outlines: false, before: 1, after: 1 }
    }
}

/// What the timeline shows. Layers are listed top first, as Animate stacks them.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TimelineModel {
    pub layers: Vec<LayerRow>,
    /// Length of the timeline in frames (the grid always shows at least this many, plus slack).
    pub frame_count: u32,
    pub current_frame: u32,
    pub fps: f32,
    pub onion: OnionSkin,
    /// Playback is running: the timeline then scrolls to keep the playhead visible.
    pub playing: bool,
}

impl TimelineModel {
    /// Indices into `layers` of the rows currently visible (those not hidden inside a collapsed folder).
    #[must_use]
    pub fn visible_rows(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.layers.len());
        // Depth below which rows are hidden because an ancestor folder is collapsed.
        let mut hidden_below: Option<u32> = None;
        for (i, row) in self.layers.iter().enumerate() {
            if let Some(depth) = hidden_below {
                if row.depth > depth {
                    continue;
                }
                hidden_below = None;
            }
            out.push(i);
            if row.is_folder() && !row.expanded {
                hidden_below = Some(row.depth);
            }
        }
        out
    }

    /// The row with `id`.
    #[must_use]
    pub fn layer(&self, id: LayerId) -> Option<&LayerRow> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// The last frame any layer uses, plus one (0 for an empty timeline).
    #[must_use]
    pub fn used_frames(&self) -> u32 {
        self.layers.iter().filter_map(|l| l.spans.last()).map(Span::end).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: u64, depth: u32, expanded: bool) -> LayerRow {
        LayerRow { kind: LayerKind::Folder, depth, expanded, ..LayerRow::new(id, "f") }
    }

    fn child(id: u64, depth: u32) -> LayerRow {
        LayerRow { depth, ..LayerRow::new(id, "c") }
    }

    #[test]
    fn collapsed_folders_hide_their_descendants() {
        let m = TimelineModel { layers: vec![folder(1, 0, false), child(2, 1), folder(3, 1, true), child(4, 2), child(5, 0)], ..Default::default() };
        assert_eq!(m.visible_rows(), vec![0, 4]);
        let mut open = m;
        if let Some(f) = open.layers.get_mut(0) {
            f.expanded = true;
        }
        assert_eq!(open.visible_rows(), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn nested_collapse_only_hides_its_own_children() {
        let m = TimelineModel { layers: vec![folder(1, 0, true), folder(2, 1, false), child(3, 2), child(4, 1), child(5, 0)], ..Default::default() };
        assert_eq!(m.visible_rows(), vec![0, 1, 3, 4]);
    }

    #[test]
    fn span_lookup() {
        let mut row = LayerRow::new(1, "a");
        row.spans = vec![Span::key(0, 3), Span::blank(5, 2), Span::key(9, 1)];
        assert_eq!(row.span_at(0).map(|s| s.start), Some(0));
        assert_eq!(row.span_at(2).map(|s| s.start), Some(0));
        assert!(row.span_at(3).is_none());
        assert!(row.span_at(4).is_none());
        assert_eq!(row.span_at(6).map(|s| s.start), Some(5));
        assert_eq!(row.span_at(9).map(|s| s.start), Some(9));
        assert!(row.span_at(10).is_none());
    }

    #[test]
    fn zero_length_span_still_covers_its_keyframe() {
        let s = Span::key(4, 0);
        assert!(s.contains(4));
        assert_eq!(s.end(), 5);
    }

    #[test]
    fn used_frames_is_end_of_last_span() {
        let mut a = LayerRow::new(1, "a");
        a.spans = vec![Span::key(0, 10)];
        let mut b = LayerRow::new(2, "b");
        b.spans = vec![Span::key(0, 4), Span::key(4, 20)];
        let m = TimelineModel { layers: vec![a, b], ..Default::default() };
        assert_eq!(m.used_frames(), 24);
    }
}
