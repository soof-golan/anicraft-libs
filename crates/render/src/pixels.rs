//! Converting between straight-alpha RGBA8 (what image decoders and encoders deal in) and the premultiplied RGBA8
//! that [`anim_display::Image`] stores and the renderer produces.

use anim_core::num::{mul255, unmul255};
use anim_display::{Image, ImageError};

use crate::par;

/// An [`Image`] from straight-alpha RGBA8 bytes (`width * height * 4` of them), premultiplying every pixel.
///
/// # Errors
/// [`ImageError`] when the buffer length does not match the size.
pub fn image_from_straight_rgba(width: u32, height: u32, straight: &[u8]) -> Result<Image, ImageError> {
    let expected = Image::byte_len(width, height).ok_or(ImageError::TooLarge { width, height })?;
    if straight.len() != expected {
        return Err(ImageError::SizeMismatch { width, height, expected, actual: straight.len() });
    }
    let mut bytes = straight.to_vec();
    par::for_each_pixel(&mut bytes, |px| {
        let [_, _, _, alpha] = *px;
        if alpha != u8::MAX {
            for channel in px.iter_mut().take(3) {
                *channel = mul255(*channel, alpha);
            }
        }
    });
    Image::new(width, height, bytes)
}

/// The image as straight-alpha RGBA8 (for PNG, GIF or a texture that expects straight alpha). Fully transparent
/// pixels become `(0, 0, 0, 0)`; other colours are rounded to nearest.
#[must_use]
pub fn straight_rgba(image: &Image) -> Vec<u8> {
    let mut bytes = image.premultiplied_rgba.clone();
    let wanted = Image::byte_len(image.width, image.height).unwrap_or(0).min(bytes.len());
    bytes.truncate(wanted);
    par::for_each_pixel(&mut bytes, |px| {
        let [red, green, blue, alpha] = *px;
        *px = [unmul255(red, alpha), unmul255(green, alpha), unmul255(blue, alpha), alpha];
    });
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_pixels_pass_through_unchanged() {
        let straight = vec![10, 20, 30, 255, 200, 100, 0, 255];
        let image = image_from_straight_rgba(2, 1, &straight).unwrap();
        assert_eq!(image.premultiplied_rgba, straight);
        assert_eq!(straight_rgba(&image), straight);
    }

    #[test]
    fn translucent_pixels_are_premultiplied_and_restored() {
        let image = image_from_straight_rgba(1, 1, &[200, 100, 50, 128]).unwrap();
        assert_eq!(image.premultiplied_rgba, vec![100, 50, 25, 128]);
        let back = straight_rgba(&image);
        assert!(back.iter().zip([200_u8, 100, 50, 128]).all(|(a, b)| a.abs_diff(b) <= 2), "{back:?}");
        let clear = image_from_straight_rgba(1, 1, &[9, 9, 9, 0]).unwrap();
        assert_eq!(clear.premultiplied_rgba, vec![0, 0, 0, 0]);
        assert_eq!(straight_rgba(&clear), vec![0, 0, 0, 0]);
    }

    #[test]
    fn wrong_lengths_are_errors() {
        assert!(matches!(image_from_straight_rgba(2, 2, &[0; 15]), Err(ImageError::SizeMismatch { .. })));
        assert!(matches!(image_from_straight_rgba(u32::MAX, u32::MAX, &[]), Err(ImageError::TooLarge { .. })));
    }

    #[test]
    fn every_straight_colour_round_trips_within_one_for_high_alpha() {
        for alpha in [255_u8, 240, 200] {
            for value in (0..=255_u8).step_by(5) {
                let image = image_from_straight_rgba(1, 1, &[value, value, value, alpha]).unwrap();
                let back = straight_rgba(&image);
                let tolerance = if alpha == 255 { 0 } else { 2 };
                assert!(back.first().is_some_and(|b| b.abs_diff(value) <= tolerance), "alpha {alpha} value {value} -> {back:?}");
            }
        }
    }
}
