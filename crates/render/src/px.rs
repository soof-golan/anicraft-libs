//! Integer pixel rectangles and the checked conversions the rasteriser needs on top of [`anim_core::num`].

use anim_core::Rect;
use anim_core::num::{ceil_f64_to_i32, floor_f64_to_i32, u32_to_usize};

/// A half-open integer rectangle `[x0, x1) x [y0, y1)` in device pixels. May be empty; never wider than `i32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PxRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl PxRect {
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// The rectangle `[0, width) x [0, height)`.
    pub fn from_size(width: u32, height: u32) -> Self {
        Self::new(0, 0, i32::try_from(width).unwrap_or(i32::MAX), i32::try_from(height).unwrap_or(i32::MAX))
    }

    /// The smallest pixel rectangle that contains `rect`, or `None` when `rect` is not finite.
    pub const fn enclosing(rect: Rect) -> Option<Self> {
        if !(rect.x0.is_finite() && rect.y0.is_finite() && rect.x1.is_finite() && rect.y1.is_finite()) {
            return None;
        }
        Some(Self::new(
            floor_f64_to_i32(rect.x0.min(rect.x1)),
            floor_f64_to_i32(rect.y0.min(rect.y1)),
            ceil_f64_to_i32(rect.x0.max(rect.x1)),
            ceil_f64_to_i32(rect.y0.max(rect.y1)),
        ))
    }

    pub fn width(&self) -> u32 {
        u32::try_from(i64::from(self.x1).saturating_sub(i64::from(self.x0)).max(0)).unwrap_or(u32::MAX)
    }

    pub fn height(&self) -> u32 {
        u32::try_from(i64::from(self.y1).saturating_sub(i64::from(self.y0)).max(0)).unwrap_or(u32::MAX)
    }

    pub const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// Number of pixels (saturating).
    pub fn area(&self) -> usize {
        u32_to_usize(self.width()).saturating_mul(u32_to_usize(self.height()))
    }

    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let r = Self::new(self.x0.max(other.x0), self.y0.max(other.y0), self.x1.min(other.x1), self.y1.min(other.y1));
        (!r.is_empty()).then_some(r)
    }

    /// Grows the rectangle by `d` pixels on every side (saturating).
    pub const fn inflate(&self, d: i32) -> Self {
        Self::new(self.x0.saturating_sub(d), self.y0.saturating_sub(d), self.x1.saturating_add(d), self.y1.saturating_add(d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enclosing_rounds_outwards_and_rejects_nan() {
        let r = PxRect::enclosing(Rect::new(0.2, -0.5, 3.0, 4.1)).unwrap();
        assert_eq!(r, PxRect::new(0, -1, 3, 5));
        assert!(PxRect::enclosing(Rect::new(f64::NAN, 0.0, 1.0, 1.0)).is_none());
        assert!(PxRect::enclosing(Rect::new(0.0, 0.0, f64::INFINITY, 1.0)).is_none());
    }

    #[test]
    fn intersect_union_inflate() {
        let a = PxRect::new(0, 0, 10, 10);
        let b = PxRect::new(5, 5, 20, 20);
        assert_eq!(a.intersect(&b), Some(PxRect::new(5, 5, 10, 10)));
        assert_eq!(a.intersect(&PxRect::new(10, 0, 12, 10)), None);
        assert_eq!(a.inflate(2), PxRect::new(-2, -2, 12, 12));
        assert_eq!(PxRect::new(i32::MIN, 0, i32::MAX, 1).width(), u32::MAX);
        assert_eq!(a.area(), 100);
    }
}
