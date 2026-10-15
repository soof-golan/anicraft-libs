//! Carets, selections and caret movement.
//!
//! Movement is *visual*: [`Motion::Right`] moves the caret to the right on screen, which in right-to-left text means
//! towards the start of the text and, in mixed text, jumps across the boundary between runs of different direction the
//! way editors do. Parley's cluster-level cursor logic does the per-paragraph work; this module continues across
//! paragraph breaks and implements vertical and line/paragraph/text motions.

use std::ops::Range;

use anim_core::Point;
use parley::editing::Cursor;
use serde::{Deserialize, Serialize};

use crate::layout::{ParaLayout, TextLayout};

/// Which side of a bidi or line-wrap boundary a caret belongs to when one offset has two screen positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Affinity {
    /// Attached to the character after the offset (the usual case).
    #[default]
    Downstream,
    /// Attached to the character before the offset: at a soft line wrap it sits at the end of the line above, and at a
    /// direction change it sits next to the preceding text.
    Upstream,
}

/// A text position: a byte offset into the plain text plus an [`Affinity`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Caret {
    pub offset: usize,
    pub affinity: Affinity,
}

impl Caret {
    /// A caret at `offset` with the default affinity.
    #[must_use]
    pub const fn new(offset: usize) -> Self {
        Self { offset, affinity: Affinity::Downstream }
    }
}

/// An anchor and a focus caret; the focus is the end that moves when the selection is extended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Selection {
    pub anchor: Caret,
    pub focus: Caret,
}

impl Selection {
    /// A selection with no extent at `caret`.
    #[must_use]
    pub const fn collapsed(caret: Caret) -> Self {
        Self { anchor: caret, focus: caret }
    }

    /// The selected characters as a byte range (smaller offset first).
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        let (a, b) = (self.anchor.offset, self.focus.offset);
        a.min(b)..a.max(b)
    }

    #[must_use]
    pub const fn is_collapsed(&self) -> bool {
        self.anchor.offset == self.focus.offset
    }
}

/// A way to move the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Motion {
    /// One character to the left on screen.
    Left,
    /// One character to the right on screen.
    Right,
    /// To the next word boundary on the left on screen.
    WordLeft,
    /// To the next word boundary on the right on screen.
    WordRight,
    /// One line up, keeping the horizontal position (`goal_x`). On the first line: the start of the text.
    Up,
    /// One line down. On the last line: the end of the text.
    Down,
    /// The start of the line in reading order (the right end of a right-to-left line).
    LineStart,
    /// The end of the line in reading order.
    LineEnd,
    ParagraphStart,
    ParagraphEnd,
    TextStart,
    TextEnd,
}

/// Moves `cursor` visually to the next word boundary. This is parley's `next_visual_word`/`previous_visual_word` with a
/// bound on the number of steps: parley loops until the cursor stops moving, and on some malformed layouts (found by
/// fuzzing) the cursor cycles between two places instead.
fn visual_word_step(para: &ParaLayout, cursor: Cursor, right: bool) -> Cursor {
    let layout = &para.layout;
    let mut cur = cursor;
    for _ in 0..para.len.saturating_add(para.prefix).saturating_add(8) {
        let next = if right { cur.next_visual(layout) } else { cur.previous_visual(layout) };
        if next == cur {
            break;
        }
        cur = next;
        let [Some(left), Some(right_cluster)] = cur.visual_clusters(layout) else { break };
        let boundary = if right {
            if left.is_rtl() { left.is_word_boundary() && !left.is_space_or_nbsp() } else { right_cluster.is_word_boundary() && !left.is_space_or_nbsp() }
        } else if left.is_rtl() {
            left.is_word_boundary() && (left.is_space_or_nbsp() || (right_cluster.is_word_boundary() && !right_cluster.is_space_or_nbsp()))
        } else {
            right_cluster.is_word_boundary() && !right_cluster.is_space_or_nbsp()
        };
        if boundary {
            break;
        }
    }
    cur
}

impl TextLayout {
    /// The x coordinate of `caret`, to keep as the `goal_x` of consecutive [`Motion::Up`]/[`Motion::Down`] moves.
    #[must_use]
    pub fn caret_x(&self, caret: Caret) -> Option<f64> {
        self.caret_rect(caret, 0.0).map(|r| r.x0)
    }

    /// Moves `caret` by `motion`. `goal_x` is the horizontal position vertical motions aim for (see
    /// [`caret_x`](Self::caret_x)); it defaults to the caret's own x.
    #[must_use]
    pub fn move_caret(&self, caret: Caret, motion: Motion, goal_x: Option<f64>) -> Caret {
        let caret = self.normalize(caret);
        match motion {
            Motion::Left => self.step(caret, false, false),
            Motion::Right => self.step(caret, true, false),
            Motion::WordLeft => self.step(caret, false, true),
            Motion::WordRight => self.step(caret, true, true),
            Motion::Up => self.vertical(caret, true, goal_x),
            Motion::Down => self.vertical(caret, false, goal_x),
            Motion::LineStart => self.line_edge(caret, false),
            Motion::LineEnd => self.line_edge(caret, true),
            Motion::ParagraphStart => Caret::new(self.paragraph_range_at(caret.offset).start),
            Motion::ParagraphEnd => Caret::new(self.paragraph_range_at(caret.offset).end),
            Motion::TextStart => Caret::new(0),
            Motion::TextEnd => Caret::new(self.text().len()),
        }
    }

    /// Moves a selection's focus by `motion`. Without `extend` the selection collapses: onto the focus's new place, or
    /// onto the matching edge of the selection when it had extent and the motion is a plain left or right.
    #[must_use]
    pub fn move_selection(&self, selection: Selection, motion: Motion, extend: bool, goal_x: Option<f64>) -> Selection {
        if !extend && !selection.is_collapsed() && matches!(motion, Motion::Left | Motion::Right) {
            let x = |c: Caret| self.caret_x(c).unwrap_or(0.0);
            let (a, b) = (selection.anchor, selection.focus);
            let a_is_left = match x(a).total_cmp(&x(b)) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Equal => a.offset <= b.offset,
                std::cmp::Ordering::Greater => false,
            };
            let (left, right) = if a_is_left { (a, b) } else { (b, a) };
            return Selection::collapsed(if motion == Motion::Left { left } else { right });
        }
        let focus = self.move_caret(selection.focus, motion, goal_x);
        if extend { Selection { anchor: selection.anchor, focus } } else { Selection::collapsed(focus) }
    }

    /// `caret` with its offset clamped and snapped to a position parley can place a caret at.
    fn normalize(&self, caret: Caret) -> Caret {
        let caret = Caret { offset: self.caret_at(caret.offset).offset, affinity: caret.affinity };
        self.para_for_offset(caret.offset).map_or(caret, |para| para.caret(para.cursor(caret)))
    }

    /// One visual step (a character, or a word) within the paragraph; at the paragraph's edge, into the neighbour.
    fn step(&self, caret: Caret, right: bool, word: bool) -> Caret {
        let Some(pi) = self.para_index_for_offset(caret.offset) else { return caret };
        let Some(para) = self.paras.get(pi) else { return caret };
        let mut cursor = para.cursor(caret);
        for _ in 0..3 {
            let next = if word {
                visual_word_step(para, cursor, right)
            } else if right {
                cursor.next_visual(&para.layout)
            } else {
                cursor.previous_visual(&para.layout)
            };
            let moved = para.caret(next);
            if moved != caret {
                return moved;
            }
            if next == cursor {
                break;
            }
            cursor = next;
        }
        // At the visual edge of the paragraph's text: continue in reading order into the neighbouring paragraph.
        let towards_end = right != para.rtl;
        if towards_end {
            self.paras.get(pi.saturating_add(1)).map_or(caret, |next| Caret::new(next.start))
        } else {
            pi.checked_sub(1).and_then(|i| self.paras.get(i)).map_or(caret, |prev| Caret::new(prev.start.saturating_add(prev.len)))
        }
    }

    fn vertical(&self, caret: Caret, up: bool, goal_x: Option<f64>) -> Caret {
        let (Some(rect), Some(line)) = (self.caret_rect(caret, 0.0), self.line_of(caret)) else { return caret };
        let x = goal_x.unwrap_or(rect.x0);
        let target = if up { line.checked_sub(1) } else { line.checked_add(1) };
        match target.and_then(|t| self.lines.get(t)) {
            Some(l) => self.hit_test_caret(Point::new(x, l.rect.center().y)),
            None if up => Caret::new(0),
            None => Caret::new(self.text().len()),
        }
    }

    fn line_edge(&self, caret: Caret, end: bool) -> Caret {
        let Some(line) = self.line_of(caret).and_then(|i| self.lines.get(i)) else { return caret };
        if !end {
            return Caret::new(line.range.start);
        }
        let paragraph_end = self.paragraph_range_at(line.range.start).end;
        let text = self.text();
        let mut offset = line.range.end.min(paragraph_end);
        let ends_with = |at: usize, c: char| at > line.range.start && text.get(..at).is_some_and(|t| t.ends_with(c));
        if ends_with(offset, '\n') {
            // A forced break ends the line before its own character.
            offset = offset.saturating_sub(1);
        } else if offset < paragraph_end {
            // A soft wrap leaves its trailing spaces hanging at the end of the line.
            while ends_with(offset, ' ') {
                offset = offset.saturating_sub(1);
            }
        }
        Caret { offset, affinity: Affinity::Upstream }
    }
}
