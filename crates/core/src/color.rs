//! Colours: [`Rgba8`] as files store them (straight alpha) and [`PremulRgba`] as renderers compute with them.

use serde::{Deserialize, Serialize};

use crate::num::unit_f32_to_u8;

/// An 8-bit sRGB colour with straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Error parsing a `#RRGGBB` / `#RRGGBBAA` / `#RGB` colour string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid colour {0:?}: expected #RGB, #RRGGBB or #RRGGBBAA")]
pub struct ParseColorError(pub String);

impl Rgba8 {
    pub const TRANSPARENT: Self = Self { r: 0, g: 0, b: 0, a: 0 };
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };

    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[must_use]
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Replaces alpha with `alpha` (0.0 ..= 1.0, clamped), the way XFL stores `color` and `alpha` separately.
    #[must_use]
    pub fn with_alpha_f32(self, alpha: f32) -> Self {
        Self { a: unit_f32_to_u8(alpha), ..self }
    }

    /// Alpha as 0.0 ..= 1.0.
    #[must_use]
    pub fn alpha_f32(self) -> f32 {
        f32::from(self.a) / 255.0
    }

    /// Parses `#RGB`, `#RRGGBB` or `#RRGGBBAA` (the leading `#` is optional; case-insensitive).
    ///
    /// # Errors
    /// Returns [`ParseColorError`] when the string is not one of those forms.
    pub fn from_hex(s: &str) -> Result<Self, ParseColorError> {
        let err = || ParseColorError(s.to_owned());
        let hex = s.trim().trim_start_matches('#');
        let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok())).collect::<Option<_>>().ok_or_else(err)?;
        let pair = |hi: u8, lo: u8| hi.checked_mul(16).and_then(|h| h.checked_add(lo));
        match digits.as_slice() {
            [r, g, b] => {
                let dup = |d: u8| pair(d, d);
                Ok(Self::opaque(dup(*r).ok_or_else(err)?, dup(*g).ok_or_else(err)?, dup(*b).ok_or_else(err)?))
            }
            [r1, r0, g1, g0, b1, b0] => Ok(Self::opaque(pair(*r1, *r0).ok_or_else(err)?, pair(*g1, *g0).ok_or_else(err)?, pair(*b1, *b0).ok_or_else(err)?)),
            [r1, r0, g1, g0, b1, b0, a1, a0] => Ok(Self::new(
                pair(*r1, *r0).ok_or_else(err)?,
                pair(*g1, *g0).ok_or_else(err)?,
                pair(*b1, *b0).ok_or_else(err)?,
                pair(*a1, *a0).ok_or_else(err)?,
            )),
            _ => Err(err()),
        }
    }

    /// `#RRGGBB` in upper case (alpha is not included; XFL writes it as a separate attribute).
    #[must_use]
    pub fn to_hex_rgb(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Converts to premultiplied `f32` (0.0 ..= 1.0 per channel, still in sRGB encoding).
    #[must_use]
    pub fn premultiply(self) -> PremulRgba {
        let a = self.alpha_f32();
        PremulRgba { r: f32::from(self.r) / 255.0 * a, g: f32::from(self.g) / 255.0 * a, b: f32::from(self.b) / 255.0 * a, a }
    }
}

/// A premultiplied-alpha colour with `f32` channels in 0.0 ..= 1.0 (sRGB encoded).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PremulRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl PremulRgba {
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    /// Converts back to straight-alpha 8-bit, clamping out-of-range channels.
    #[must_use]
    pub fn unpremultiply(self) -> Rgba8 {
        if self.a <= 0.0 || self.a.is_nan() {
            return Rgba8::TRANSPARENT;
        }
        let a = self.a.min(1.0);
        Rgba8::new(unit_f32_to_u8(self.r / a), unit_f32_to_u8(self.g / a), unit_f32_to_u8(self.b / a), unit_f32_to_u8(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(Rgba8::from_hex("#FF0080").unwrap(), Rgba8::opaque(255, 0, 128));
        assert_eq!(Rgba8::from_hex("ff008040").unwrap(), Rgba8::new(255, 0, 128, 64));
        assert_eq!(Rgba8::from_hex("#f0a").unwrap(), Rgba8::opaque(255, 0, 170));
        assert!(Rgba8::from_hex("#12345").is_err());
        assert!(Rgba8::from_hex("#GG0000").is_err());
        assert!(Rgba8::from_hex("").is_err());
    }

    #[test]
    fn hex_round_trip() {
        let c = Rgba8::opaque(1, 171, 255);
        assert_eq!(Rgba8::from_hex(&c.to_hex_rgb()).unwrap(), c);
    }

    #[test]
    fn premultiply_round_trip() {
        let c = Rgba8::new(200, 100, 50, 128);
        let back = c.premultiply().unpremultiply();
        assert!(back.r.abs_diff(c.r) <= 1 && back.g.abs_diff(c.g) <= 1 && back.b.abs_diff(c.b) <= 1 && back.a == c.a);
        assert_eq!(Rgba8::new(9, 9, 9, 0).premultiply().unpremultiply(), Rgba8::TRANSPARENT);
    }
}
