//! 9-slice scaling: how an instance of a symbol with scaling guides is drawn once it is scaled.
//!
//! The four guides cut the symbol into nine pieces. When the instance is scaled, the four corners keep their size, the
//! four edges stretch along one axis only and the middle stretches along both, so rounded corners and borders keep
//! their look. Rotation and flips are kept: the matrix is split into its scale and the rest (a rotation, a flip, a
//! skew), the pieces are placed in the scaled space and the rest is applied on top.

use anim_core::{Affine, Rect};

/// How far the outermost pieces reach past the symbol's bounds, so content outside them (a wide stroke) still shows. It
/// is kept modest: the renderer works in single precision, where a clip edge millions of pixels away loses its place.
const REACH: f64 = 1.0e5;
/// Overlap, in output pixels, between neighbouring pieces. Two clipped edges that meet at a fraction of a pixel
/// would otherwise let a hairline of the background through.
const SEAM: f64 = 0.5;
/// A scale this close to 1 draws the symbol as it is.
const UNIT: f64 = 1.0e-6;

/// One of the nine pieces: the part of the symbol it shows and where it goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    /// The part of the symbol this piece shows, in the symbol's own pixels.
    pub source: Rect,
    /// Symbol pixels to the space of the group that holds the pieces.
    pub transform: Affine,
}

/// The nine pieces of one scaled instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Placement of the group that holds the pieces: the instance's matrix without its scale.
    pub outer: Affine,
    /// Row by row, top left first.
    pub pieces: Vec<Piece>,
}

/// One axis: where each of the three bands goes.
#[derive(Debug, Clone, Copy)]
struct Axis {
    /// Source start of each band: bounds start, first guide, second guide.
    from: [f64; 3],
    /// Destination start of each band, in the scaled space.
    to: [f64; 3],
    /// Stretch of each band.
    k: [f64; 3],
}

impl Axis {
    /// Band `i` covers the source from `from[i]` to the next band's start (the last band reaches `REACH`); it maps
    /// `x` to `k * (x - from) + to`.
    fn offset(&self, band: usize) -> (f64, f64) {
        let k = self.k.get(band).copied().unwrap_or(1.0);
        let from = self.from.get(band).copied().unwrap_or(0.0);
        let to = self.to.get(band).copied().unwrap_or(0.0);
        (k, k.mul_add(-from, to))
    }
}

/// Lays out one axis. `lo`..`hi` are the bounds, `g0`..`g1` the guides and `scale` the (positive) stretch of the whole.
fn axis(lo: f64, hi: f64, g0: f64, g1: f64, scale: f64) -> Axis {
    let near = (g0 - lo).max(0.0);
    let far = (hi - g1).max(0.0);
    let middle = (g1 - g0).max(0.0);
    let total = (hi - lo) * scale;
    let fixed = near + far;
    // Squeezed below the size of the corners: they shrink together and the middle disappears.
    let (corner, middle_k) = if total >= fixed {
        (1.0, if middle > 0.0 { (total - fixed) / middle } else { 0.0 })
    } else if fixed > 0.0 {
        (total / fixed, 0.0)
    } else {
        (1.0, 0.0)
    };
    let start = lo * scale;
    let end = hi * scale;
    let first = near.mul_add(corner, start);
    let second = far.mul_add(-corner, end);
    Axis { from: [lo, g0, g1], to: [start, first, second], k: [corner, middle_k, corner] }
}

/// Plans the nine pieces of an instance placed by `matrix`, for a symbol with the guides `grid` and the content
/// `bounds`. `None` when the instance is not scaled (draw it as it is) or its matrix cannot be split.
#[must_use]
pub fn plan(matrix: Affine, grid: Rect, bounds: Rect) -> Option<Plan> {
    let [xx, xy, yx, yy, tx, ty] = matrix.as_coeffs();
    let (sx, sy) = (xx.hypot(xy), yx.hypot(yy));
    if !(sx.is_finite() && sy.is_finite()) || sx < 1.0e-9 || sy < 1.0e-9 {
        return None;
    }
    if (sx - 1.0).abs() < UNIT && (sy - 1.0).abs() < UNIT {
        return None;
    }
    if !(bounds.width() > 0.0 && bounds.height() > 0.0) {
        return None;
    }
    // The guides cannot lie outside the content.
    let guides = Rect::new(
        grid.x0.clamp(bounds.x0, bounds.x1),
        grid.y0.clamp(bounds.y0, bounds.y1),
        grid.x1.clamp(bounds.x0, bounds.x1).max(grid.x0.clamp(bounds.x0, bounds.x1)),
        grid.y1.clamp(bounds.y0, bounds.y1).max(grid.y0.clamp(bounds.y0, bounds.y1)),
    );
    let outer = Affine::new([xx / sx, xy / sx, yx / sy, yy / sy, tx, ty]);
    let across = axis(bounds.x0, bounds.x1, guides.x0, guides.x1, sx);
    let down = axis(bounds.y0, bounds.y1, guides.y0, guides.y1, sy);
    let edges_x = [-REACH, guides.x0, guides.x1, REACH];
    let edges_y = [-REACH, guides.y0, guides.y1, REACH];
    let mut pieces = Vec::with_capacity(9);
    for row in 0..3_usize {
        for col in 0..3_usize {
            let (kx, ox) = across.offset(col);
            let (ky, oy) = down.offset(row);
            // Grow the piece into its neighbours by half an output pixel, so the seams close.
            let seam_x = if kx > 1.0e-9 { SEAM / kx } else { 0.0 };
            let seam_y = if ky > 1.0e-9 { SEAM / ky } else { 0.0 };
            let left = edges_x.get(col).copied().unwrap_or(0.0) - if col > 0 { seam_x } else { 0.0 };
            let right = edges_x.get(col.saturating_add(1)).copied().unwrap_or(0.0) + if col < 2 { seam_x } else { 0.0 };
            let top = edges_y.get(row).copied().unwrap_or(0.0) - if row > 0 { seam_y } else { 0.0 };
            let bottom = edges_y.get(row.saturating_add(1)).copied().unwrap_or(0.0) + if row < 2 { seam_y } else { 0.0 };
            pieces.push(Piece { source: Rect::new(left, top, right, bottom), transform: Affine::new([kx, 0.0, 0.0, ky, ox, oy]) });
        }
    }
    Some(Plan { outer, pieces })
}

#[cfg(test)]
mod tests {
    use anim_core::Point;

    use super::*;

    fn square() -> (Rect, Rect) {
        (Rect::new(20.0, 20.0, 80.0, 80.0), Rect::new(0.0, 0.0, 100.0, 100.0))
    }

    /// Where `p` of the symbol lands: the piece that shows it, then the placement.
    fn land(plan: &Plan, p: Point) -> Point {
        let piece = plan
            .pieces
            .iter()
            .filter(|piece| piece.source.contains(p))
            .min_by(|a, b| (a.source.area()).total_cmp(&b.source.area()))
            .expect("a piece shows every point");
        plan.outer * (piece.transform * p)
    }

    #[test]
    fn an_unscaled_instance_needs_no_slicing() {
        let (grid, bounds) = square();
        assert!(plan(Affine::translate((5.0, 5.0)), grid, bounds).is_none());
        assert!(plan(Affine::rotate(0.7), grid, bounds).is_none(), "a rotation is not a scale");
    }

    #[test]
    fn corners_keep_their_size_and_the_middle_takes_the_stretch() {
        let (grid, bounds) = square();
        let plan = plan(Affine::scale_non_uniform(3.0, 2.0), grid, bounds).unwrap();
        assert_eq!(plan.pieces.len(), 9);
        // The top left corner is where it was.
        assert_eq!(land(&plan, Point::new(10.0, 10.0)), Point::new(10.0, 10.0));
        // The bottom right corner keeps its 20 pixels against the scaled edge (300, 200).
        assert_eq!(land(&plan, Point::new(90.0, 90.0)), Point::new(290.0, 190.0));
        // The middle runs from 20 to 280 across and from 20 to 180 down.
        assert_eq!(land(&plan, Point::new(20.5, 20.5)).x.round(), 22.0);
        let centre = land(&plan, Point::new(50.0, 50.0));
        assert!((centre.x - 150.0).abs() < 1e-9 && (centre.y - 100.0).abs() < 1e-9, "{centre:?}");
        // The top edge stretches across only.
        let top = land(&plan, Point::new(50.0, 10.0));
        assert!((top.x - 150.0).abs() < 1e-9 && (top.y - 10.0).abs() < 1e-9, "{top:?}");
    }

    #[test]
    fn content_beyond_the_bounds_is_still_shown() {
        let (grid, bounds) = square();
        let plan = plan(Affine::scale(2.0), grid, bounds).unwrap();
        assert!(plan.pieces.iter().any(|p| p.source.contains(Point::new(-500.0, -500.0))));
        assert!(plan.pieces.iter().any(|p| p.source.contains(Point::new(900.0, 900.0))));
        assert_eq!(land(&plan, Point::new(-5.0, -5.0)), Point::new(-5.0, -5.0), "a stroke outside the corner keeps its offset");
    }

    #[test]
    fn rotation_and_translation_stay_on_the_outside() {
        let (grid, bounds) = square();
        let m = Affine::translate((10.0, 20.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale_non_uniform(2.0, 1.0);
        let plan = plan(m, grid, bounds).unwrap();
        // The point (90, 50) is in the right edge: 20 from the scaled right side (200), then turned a quarter.
        let at = land(&plan, Point::new(90.0, 50.0));
        let expected = Affine::translate((10.0, 20.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Point::new(190.0, 50.0);
        assert!((at.x - expected.x).abs() < 1e-9 && (at.y - expected.y).abs() < 1e-9, "{at:?} {expected:?}");
    }

    #[test]
    fn a_flip_survives() {
        let (grid, bounds) = square();
        let plan = plan(Affine::scale_non_uniform(2.0, -1.0), grid, bounds).unwrap();
        let at = land(&plan, Point::new(10.0, 10.0));
        assert!((at.x - 10.0).abs() < 1e-9 && (at.y + 10.0).abs() < 1e-9, "{at:?}");
    }

    #[test]
    fn shrinking_below_the_corners_squeezes_them_together() {
        let (grid, bounds) = square();
        // The corners need 40 pixels across; the instance is 20 wide.
        let plan = plan(Affine::scale_non_uniform(0.2, 1.0), grid, bounds).unwrap();
        let left = land(&plan, Point::new(10.0, 50.0));
        let right = land(&plan, Point::new(90.0, 50.0));
        assert!(left.x >= 0.0 && right.x <= 20.0 + 1e-9, "{left:?} {right:?}");
        assert!(right.x > left.x);
    }

    #[test]
    fn pieces_overlap_by_a_seam_and_reach_everywhere() {
        let (grid, bounds) = square();
        let plan = plan(Affine::scale_non_uniform(2.0, 2.0), grid, bounds).unwrap();
        let centre = plan.pieces.get(4).unwrap();
        assert!(centre.source.x0 < 20.0 && centre.source.x1 > 80.0, "the middle reaches into its neighbours: {:?}", centre.source);
        let corner = plan.pieces.first().unwrap();
        assert!(corner.source.x0 < -1.0e4 && corner.source.x1 > 20.0);
    }

    #[test]
    fn guides_outside_the_content_and_degenerate_matrices_are_harmless() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        let wild = Rect::new(-50.0, 120.0, 400.0, 10.0);
        assert!(plan(Affine::scale(2.0), wild, bounds).is_some());
        assert!(plan(Affine::scale_non_uniform(0.0, 2.0), bounds, bounds).is_none());
        assert!(plan(Affine::scale(2.0), bounds, Rect::new(0.0, 0.0, 0.0, 0.0)).is_none());
        assert!(plan(Affine::scale(f64::NAN), bounds, bounds).is_none());
    }
}
