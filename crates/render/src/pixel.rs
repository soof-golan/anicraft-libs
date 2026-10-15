//! Premultiplied RGBA8 pixel helpers shared by compositing and filters.

use anim_core::num::unit_f32_to_u8;

/// `0..=255` to `0.0..=1.0`.
pub fn unit(value: u8) -> f32 {
    f32::from(value) / 255.0
}

/// `a * b / 255`, rounded.
pub fn mul255(first: u8, second: u8) -> u8 {
    u8::try_from(u32::from(first).saturating_mul(u32::from(second)).saturating_add(127) / 255).unwrap_or(u8::MAX)
}

/// Multiplies every channel of a premultiplied pixel by `factor / 255`.
pub fn scale(px: &mut [u8; 4], factor: u8) {
    for channel in px.iter_mut() {
        *channel = mul255(*channel, factor);
    }
}

/// Stores premultiplied unit channels, keeping the premultiplied invariant `channel <= alpha` after rounding.
pub fn store(px: &mut [u8; 4], red: f32, green: f32, blue: f32, alpha: f32) {
    let alpha = unit_f32_to_u8(alpha);
    *px = [unit_f32_to_u8(red).min(alpha), unit_f32_to_u8(green).min(alpha), unit_f32_to_u8(blue).min(alpha), alpha];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplication_rounds_and_covers_the_range() {
        assert_eq!(mul255(255, 255), 255);
        assert_eq!(mul255(255, 128), 128);
        assert_eq!(mul255(0, 200), 0);
        assert_eq!(mul255(100, 100), 39);
    }

    #[test]
    fn store_keeps_the_premultiplied_invariant() {
        let mut px = [0; 4];
        store(&mut px, 1.0, 0.5, 0.2, 0.5);
        assert_eq!(px, [128, 128, 51, 128]);
        scale(&mut px, 0);
        assert_eq!(px, [0; 4]);
    }
}
