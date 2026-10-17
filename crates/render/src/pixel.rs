//! Premultiplied RGBA8 pixel helpers shared by compositing and filters.

use anim_core::num::{mul255, unit_f32_to_u8};

/// `0..=255` to `0.0..=1.0`.
pub fn unit(value: u8) -> f32 {
    f32::from(value) / 255.0
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
    fn store_keeps_the_premultiplied_invariant() {
        let mut px = [0; 4];
        store(&mut px, 1.0, 0.5, 0.2, 0.5);
        assert_eq!(px, [128, 128, 51, 128]);
        scale(&mut px, 0);
        assert_eq!(px, [0; 4]);
    }
}
