//! Animate shape tweens.
//!
//! [`tween`] blends two [`EdgeShape`]s the way a classic shape tween does:
//!
//! 1. every fill style's region is split into closed contours and every stroke into polylines
//!    (via `anim-shape`'s assembly);
//! 2. contours of the two keyframes are matched by fill style, size (area), position (centroid) and winding, with
//!    the order in the file as the tie-breaker; leftovers shrink to / grow from their centroid;
//! 3. each matched pair is brought to the same number of cubics. [`MorphMode::Distributive`] resamples by arc
//!    length (smooth in-betweens, corners may soften); [`MorphMode::Angular`] splits segments instead, so the
//!    corners of both shapes stay vertices. The start point and direction that minimise the total vertex distance
//!    are chosen;
//! 4. [`ShapeHint`]s (Animate's `a..z` markers) pin points of one keyframe to points of the other: contours are
//!    split there and every piece is resampled on its own;
//! 5. control points, colours, gradient matrices and stops, and stroke widths are interpolated.
//!
//! Matching is the expensive part, so [`MorphPlan`] does it once and [`MorphPlan::at`] samples it for any `t`.
//! Easing is the caller's business: pass the eased `t`. Everything is in shape pixels, like `anim-shape`'s
//! geometry API.

mod align;
mod contour;
mod plan;
pub mod style;

use anim_core::Point;
use anim_shape::EdgeShape;
use serde::{Deserialize, Serialize};

pub use plan::MorphPlan;

/// Animate's blend setting for shape tweens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MorphMode {
    /// Smooth in-betweens: contours are resampled evenly by arc length.
    #[default]
    Distributive,
    /// Keeps corners: segments are split rather than resampled, so every original vertex survives.
    Angular,
}

/// A pair of corresponding points, in shape pixels.
///
/// `start` lies on the first keyframe's outline, `end` on the second's. Points snap to the nearest outline; hints
/// far from every outline are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapeHint {
    pub start: Point,
    pub end: Point,
}

impl ShapeHint {
    #[must_use]
    pub const fn new(start: Point, end: Point) -> Self {
        Self { start, end }
    }
}

/// Everything besides the two shapes that steers a tween.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MorphOptions {
    pub mode: MorphMode,
    pub hints: Vec<ShapeHint>,
}

/// The shape `t` (0 to 1, already eased) of the way from `start` to `end`, distributive blend.
#[must_use]
pub fn tween(start: &EdgeShape, end: &EdgeShape, t: f64, hints: &[ShapeHint]) -> EdgeShape {
    tween_with(start, end, t, &MorphOptions { mode: MorphMode::Distributive, hints: hints.to_vec() })
}

/// Like [`tween`], with a blend mode.
#[must_use]
pub fn tween_with(start: &EdgeShape, end: &EdgeShape, t: f64, options: &MorphOptions) -> EdgeShape {
    MorphPlan::new(start, end, options).at(t)
}
