//! Pure timeline geometry: frame and row hit testing, scroll and zoom maths. No egui input or painting here.

use egui::{Pos2, Rect, Vec2, pos2};
use serde::{Deserialize, Serialize};

use super::model::LayerId;
use crate::util::{f32_from_u32, f32_from_usize, floor_u32};

/// Narrowest and widest a frame cell may be, in points.
pub const MIN_FRAME_WIDTH: f32 = 3.0;
pub const MAX_FRAME_WIDTH: f32 = 64.0;

/// A rectangular block of frames over a run of layer rows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FrameSelection {
    /// Layers covered, in display order (contiguous).
    pub layers: Vec<LayerId>,
    /// First selected frame.
    pub first: u32,
    /// Last selected frame (inclusive).
    pub last: u32,
}

impl FrameSelection {
    /// A single frame on a single layer.
    #[must_use]
    pub fn single(layer: LayerId, frame: u32) -> Self {
        Self { layers: vec![layer], first: frame, last: frame }
    }

    /// Whether the selection covers `frame` on `layer`.
    #[must_use]
    pub fn contains(&self, layer: LayerId, frame: u32) -> bool {
        self.layers.contains(&layer) && frame >= self.first && frame <= self.last
    }

    /// Number of frames wide.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.last.saturating_sub(self.first).saturating_add(1)
    }

    /// Whether nothing is selected.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

/// Where the pointer is, relative to the frame grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Over the ruler at this frame.
    Ruler(u32),
    /// Over a cell: visible row index and frame.
    Cell {
        row: usize,
        frame: u32,
    },
    /// Over the grid below the last row.
    Empty {
        frame: u32,
    },
    Outside,
}

/// Screen geometry of the grid area (the part right of the layer column and below the ruler).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineGeometry {
    /// Rectangle of the ruler strip (frame numbers).
    pub ruler: Rect,
    /// Rectangle of the cells.
    pub grid: Rect,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub frame_width: f32,
    pub row_height: f32,
    /// Number of visible rows.
    pub rows: usize,
}

impl TimelineGeometry {
    /// Left edge of `frame` on screen.
    #[must_use]
    pub fn x_of_frame(&self, frame: u32) -> f32 {
        f32_from_u32(frame).mul_add(self.frame_width, self.grid.left()) - self.scroll_x
    }

    /// Centre of `frame` on screen.
    #[must_use]
    pub fn x_center_of_frame(&self, frame: u32) -> f32 {
        self.frame_width.mul_add(0.5, self.x_of_frame(frame))
    }

    /// Top edge of visible row `row` on screen.
    #[must_use]
    pub fn y_of_row(&self, row: usize) -> f32 {
        f32_from_usize(row).mul_add(self.row_height, self.grid.top()) - self.scroll_y
    }

    /// The cell rectangle for `row` and `frames` (first, last inclusive).
    #[must_use]
    pub fn cell_rect(&self, row: usize, first: u32, last: u32) -> Rect {
        let top = self.y_of_row(row);
        Rect::from_min_max(pos2(self.x_of_frame(first), top), pos2(self.x_of_frame(last.saturating_add(1)), top + self.row_height))
    }

    /// The frame under screen x, clamped at 0 on the left (no upper bound).
    #[must_use]
    pub fn frame_at_x(&self, x: f32) -> u32 {
        floor_u32((x - self.grid.left() + self.scroll_x) / self.frame_width)
    }

    /// The visible row under screen y, if it is within `rows`.
    #[must_use]
    pub fn row_at_y(&self, y: f32) -> Option<usize> {
        let v = (y - self.grid.top() + self.scroll_y) / self.row_height;
        if v < 0.0 {
            return None;
        }
        let row = usize::try_from(floor_u32(v)).ok()?;
        (row < self.rows).then_some(row)
    }

    /// Classifies a pointer position.
    #[must_use]
    pub fn hit(&self, p: Pos2) -> Hit {
        if self.ruler.contains(p) {
            return Hit::Ruler(self.frame_at_x(p.x));
        }
        if !self.grid.contains(p) {
            return Hit::Outside;
        }
        let frame = self.frame_at_x(p.x);
        self.row_at_y(p.y).map_or(Hit::Empty { frame }, |row| Hit::Cell { row, frame })
    }

    /// The range of frames (first, one past last) that intersect the grid.
    #[must_use]
    pub fn visible_frames(&self) -> (u32, u32) {
        let first = self.frame_at_x(self.grid.left());
        let last = floor_u32((self.grid.right() - self.grid.left() + self.scroll_x) / self.frame_width).saturating_add(1);
        (first, last)
    }

    /// The range of visible rows (first, one past last) intersecting the grid.
    #[must_use]
    pub fn visible_rows(&self) -> (usize, usize) {
        let first = usize::try_from(floor_u32(self.scroll_y / self.row_height)).unwrap_or(0);
        let last = usize::try_from(floor_u32((self.grid.height() + self.scroll_y) / self.row_height).saturating_add(1)).unwrap_or(0);
        (first.min(self.rows), last.min(self.rows))
    }

    /// Total content size for `frames` frames and the current rows.
    #[must_use]
    pub fn content_size(&self, frames: u32) -> Vec2 {
        egui::vec2(f32_from_u32(frames) * self.frame_width, f32_from_usize(self.rows) * self.row_height)
    }

    /// The `scroll_x` that keeps `frame` (fractional) under screen x `anchor_x` when the frame width becomes `new_width`.
    #[must_use]
    pub fn zoom_about(&self, anchor_x: f32, new_width: f32) -> (f32, f32) {
        let width = new_width.clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
        let frame_f = (anchor_x - self.grid.left() + self.scroll_x) / self.frame_width;
        let scroll = frame_f.mul_add(width, -(anchor_x - self.grid.left())).max(0.0);
        (width, scroll)
    }

    /// The `scroll_x` that puts `frame` in the middle of the grid, or `None` when it is already visible with a
    /// little margin. Revealing centres the frame instead of pinning it to an edge, so there is room on both sides
    /// to see where it is going (and playback pages half a screen at a time rather than every few frames).
    #[must_use]
    pub fn scroll_to_reveal(&self, frame: u32) -> Option<f32> {
        let left = f32_from_u32(frame) * self.frame_width;
        let right = left + self.frame_width;
        let margin = self.frame_width * 2.0;
        if left - margin < self.scroll_x || right + margin > self.scroll_x + self.grid.width() {
            let centred = (left + right).mul_add(0.5, -(self.grid.width() * 0.5));
            Some(centred.max(0.0))
        } else {
            None
        }
    }

    /// Clamps scroll offsets to the content (`frames` wide) so the grid never scrolls past its end.
    #[must_use]
    pub fn clamp_scroll(&self, frames: u32, scroll: Vec2) -> Vec2 {
        let content = self.content_size(frames);
        egui::vec2(scroll.x.clamp(0.0, (content.x - self.grid.width()).max(0.0)), scroll.y.clamp(0.0, (content.y - self.grid.height()).max(0.0)))
    }
}

/// Builds the contiguous layer run between two rows (inclusive, in either order) from the visible `ids`.
#[must_use]
pub fn layers_between(ids: &[LayerId], a: usize, b: usize) -> Vec<LayerId> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    ids.iter().skip(lo).take(hi.saturating_sub(lo).saturating_add(1)).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn geo() -> TimelineGeometry {
        TimelineGeometry {
            ruler: Rect::from_min_size(pos2(200.0, 0.0), vec2(600.0, 20.0)),
            grid: Rect::from_min_size(pos2(200.0, 20.0), vec2(600.0, 200.0)),
            scroll_x: 0.0,
            scroll_y: 0.0,
            frame_width: 10.0,
            row_height: 20.0,
            rows: 6,
        }
    }

    #[test]
    fn frame_and_row_hit_testing() {
        let g = geo();
        assert_eq!(g.hit(pos2(205.0, 25.0)), Hit::Cell { row: 0, frame: 0 });
        assert_eq!(g.hit(pos2(399.0, 59.0)), Hit::Cell { row: 1, frame: 19 });
        assert_eq!(g.hit(pos2(399.0, 150.0)), Hit::Empty { frame: 19 });
        assert_eq!(g.hit(pos2(250.0, 10.0)), Hit::Ruler(5));
        assert_eq!(g.hit(pos2(100.0, 50.0)), Hit::Outside);
    }

    #[test]
    fn scrolling_shifts_hits() {
        let mut g = geo();
        g.scroll_x = 55.0;
        g.scroll_y = 30.0;
        assert_eq!(g.frame_at_x(200.0), 5);
        assert_eq!(g.row_at_y(20.0), Some(1));
        assert!((g.x_of_frame(5) - 195.0).abs() < 1e-3);
        assert!((g.y_of_row(1) - 10.0).abs() < 1e-3);
    }

    #[test]
    fn x_and_frame_are_inverse() {
        let g = geo();
        for f in [0u32, 1, 7, 59] {
            assert_eq!(g.frame_at_x(g.x_center_of_frame(f)), f);
        }
    }

    #[test]
    fn visible_ranges() {
        let mut g = geo();
        g.scroll_x = 100.0;
        let (a, b) = g.visible_frames();
        assert_eq!(a, 10);
        assert_eq!(b, 71);
        assert_eq!(g.visible_rows(), (0, 6));
    }

    #[test]
    fn zoom_keeps_frame_under_cursor() {
        let mut g = geo();
        g.scroll_x = 120.0;
        let anchor = 400.0;
        let before = (anchor - g.grid.left() + g.scroll_x) / g.frame_width;
        let (w, s) = g.zoom_about(anchor, 25.0);
        let after = (anchor - g.grid.left() + s) / w;
        assert!((before - after).abs() < 1e-3);
        assert!((w - 25.0).abs() < 1e-5);
        assert!((g.zoom_about(anchor, 1000.0).0 - MAX_FRAME_WIDTH).abs() < 1e-5);
        assert!((g.zoom_about(anchor, 0.1).0 - MIN_FRAME_WIDTH).abs() < 1e-5);
    }

    #[test]
    fn reveal_scrolls_only_when_needed() {
        let mut g = geo();
        assert_eq!(g.scroll_to_reveal(10), None);
        assert!(g.scroll_to_reveal(100).is_some_and(|s| s > 0.0));
        g.scroll_x = 500.0;
        assert_eq!(g.scroll_to_reveal(0), Some(0.0));
    }

    #[test]
    fn reveal_centres_the_frame() {
        let mut g = geo();
        // Frame 100 spans x 1000..1010; the 600 wide grid centred on it starts at 1005 - 300.
        let scroll = g.scroll_to_reveal(100).unwrap();
        assert!((scroll - 705.0).abs() < 1e-3);
        g.scroll_x = scroll;
        let centre = g.x_center_of_frame(100);
        assert!((centre - g.grid.center().x).abs() < 1e-3, "the playhead is in the middle, not at an edge");
        assert!(g.scroll_to_reveal(100).is_none());
        // A frame left of the view is centred too, clamped at the start of the timeline.
        assert!((g.scroll_to_reveal(20).unwrap() - 0.0).abs() < 1e-3);
        assert!((g.scroll_to_reveal(60).unwrap() - 305.0).abs() < 1e-3);
        assert!(g.scroll_to_reveal(90).is_none());
    }

    #[test]
    fn clamp_scroll_stops_at_content_end() {
        let g = geo();
        let c = g.clamp_scroll(100, vec2(10_000.0, 10_000.0));
        assert!((c.x - (1000.0 - 600.0)).abs() < 1e-3);
        assert!((c.y - 0.0).abs() < 1e-3);
        assert_eq!(g.clamp_scroll(100, vec2(-5.0, -5.0)), Vec2::ZERO);
    }

    #[test]
    fn selection_contains() {
        let s = FrameSelection { layers: vec![3, 4], first: 2, last: 6 };
        assert!(s.contains(3, 2) && s.contains(4, 6));
        assert!(!s.contains(5, 3) && !s.contains(3, 7));
        assert_eq!(s.width(), 5);
    }

    #[test]
    fn layer_runs() {
        let ids = [10, 11, 12, 13, 14];
        assert_eq!(layers_between(&ids, 3, 1), vec![11, 12, 13]);
        assert_eq!(layers_between(&ids, 2, 2), vec![12]);
        assert_eq!(layers_between(&ids, 4, 9), vec![14]);
    }
}
