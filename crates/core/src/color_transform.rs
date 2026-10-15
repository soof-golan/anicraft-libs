//! Flash-style colour transforms: `out = clamp(in * multiplier + offset)` per channel.
//!
//! Animate's "colour effect" styles (brightness, tint, alpha, advanced) all reduce to this. Offsets are stored in
//! unit space (an XFL `redOffset="255"` is `1.0` here) so the transform composes with premultiplied `f32` colour.

use serde::{Deserialize, Serialize};

use crate::color::{PremulRgba, Rgba8};

/// Per-channel multiply then offset, applied to straight-alpha colour (as Flash defines it).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorTransform {
    /// Multipliers for r, g, b, a.
    pub mul: [f32; 4],
    /// Offsets for r, g, b, a in unit space (-1.0 ..= 1.0 covers XFL's -255 ..= 255).
    pub add: [f32; 4],
}

impl Default for ColorTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl ColorTransform {
    pub const IDENTITY: Self = Self { mul: [1.0; 4], add: [0.0; 4] };

    /// Alpha-only transform (Animate "Alpha" style), `alpha` in 0.0 ..= 1.0.
    #[must_use]
    pub const fn alpha(alpha: f32) -> Self {
        Self { mul: [1.0, 1.0, 1.0, alpha], add: [0.0; 4] }
    }

    /// Brightness style, `brightness` in -1.0 ..= 1.0 (towards black or white).
    #[must_use]
    pub fn brightness(brightness: f32) -> Self {
        let b = brightness.clamp(-1.0, 1.0);
        let k = 1.0 - b.abs();
        let add = if b > 0.0 { b } else { 0.0 };
        Self { mul: [k, k, k, 1.0], add: [add, add, add, 0.0] }
    }

    /// Tint style: blend towards `color` by `amount` (0.0 ..= 1.0).
    #[must_use]
    pub fn tint(color: Rgba8, amount: f32) -> Self {
        let t = amount.clamp(0.0, 1.0);
        let k = 1.0 - t;
        let ch = |c: u8| f32::from(c) / 255.0 * t;
        Self { mul: [k, k, k, 1.0], add: [ch(color.r), ch(color.g), ch(color.b), 0.0] }
    }

    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.mul.iter().all(|m| (m - 1.0).abs() < f32::EPSILON) && self.add.iter().all(|a| a.abs() < f32::EPSILON)
    }

    /// `self` applied after `inner` (a child's transform `inner` inside a parent `self`).
    #[must_use]
    pub fn then(&self, outer: &Self) -> Self {
        let mut mul = [1.0; 4];
        let mut add = [0.0; 4];
        for (((m, a), (sm, sa)), (om, oa)) in
            mul.iter_mut().zip(add.iter_mut()).zip(self.mul.iter().zip(self.add.iter())).zip(outer.mul.iter().zip(outer.add.iter()))
        {
            *m = sm * om;
            *a = sa.mul_add(*om, *oa);
        }
        Self { mul, add }
    }

    /// Linear interpolation between two transforms (used by tweens), `t` in 0.0 ..= 1.0.
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let mut out = Self::IDENTITY;
        for (((o, a), b), _) in out.mul.iter_mut().zip(self.mul.iter()).zip(other.mul.iter()).zip(0..4) {
            *o = (b - a).mul_add(t, *a);
        }
        for (((o, a), b), _) in out.add.iter_mut().zip(self.add.iter()).zip(other.add.iter()).zip(0..4) {
            *o = (b - a).mul_add(t, *a);
        }
        out
    }

    /// Applies the transform to a premultiplied colour (un-premultiplies, transforms, re-premultiplies).
    #[must_use]
    pub fn apply(&self, c: PremulRgba) -> PremulRgba {
        if c.a <= 0.0 && self.add[3] <= 0.0 {
            return PremulRgba::TRANSPARENT;
        }
        let inv = if c.a > 0.0 { 1.0 / c.a } else { 0.0 };
        let [mul_r, mul_g, mul_b, mul_a] = self.mul;
        let [add_r, add_g, add_b, add_a] = self.add;
        let alpha = mul_a.mul_add(c.a, add_a).clamp(0.0, 1.0);
        let red = (c.r * inv).mul_add(mul_r, add_r).clamp(0.0, 1.0);
        let green = (c.g * inv).mul_add(mul_g, add_g).clamp(0.0, 1.0);
        let blue = (c.b * inv).mul_add(mul_b, add_b).clamp(0.0, 1.0);
        PremulRgba { r: red * alpha, g: green * alpha, b: blue * alpha, a: alpha }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn identity_is_a_no_op() {
        let c = Rgba8::new(10, 20, 30, 200).premultiply();
        let out = ColorTransform::IDENTITY.apply(c);
        assert!(close(out.r, c.r) && close(out.a, c.a));
        assert!(ColorTransform::default().is_identity());
    }

    #[test]
    fn alpha_and_brightness() {
        let white = Rgba8::WHITE.premultiply();
        assert!(close(ColorTransform::alpha(0.5).apply(white).a, 0.5));
        let black = Rgba8::BLACK.premultiply();
        assert!(close(ColorTransform::brightness(1.0).apply(black).r, 1.0));
        assert!(close(ColorTransform::brightness(-1.0).apply(white).r, 0.0));
    }

    #[test]
    fn composition_matches_sequential_application() {
        let inner = ColorTransform::tint(Rgba8::opaque(255, 0, 0), 0.5);
        let outer = ColorTransform::alpha(0.25);
        let c = Rgba8::opaque(0, 128, 255).premultiply();
        let seq = outer.apply(inner.apply(c));
        let comp = inner.then(&outer).apply(c);
        assert!(close(seq.r, comp.r) && close(seq.g, comp.g) && close(seq.b, comp.b) && close(seq.a, comp.a));
    }

    #[test]
    fn lerp_endpoints() {
        let a = ColorTransform::IDENTITY;
        let b = ColorTransform::alpha(0.0);
        assert_eq!(a.lerp(&b, 0.0), a);
        assert_eq!(a.lerp(&b, 1.0), b);
    }
}
