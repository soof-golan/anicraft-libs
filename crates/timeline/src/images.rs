//! Bitmap decoding hook. Decoding is the format crate's job; the evaluator only asks a provider for pixels.

use std::io::Cursor;

use anim_display::Image;

/// Largest bitmap (in pixels) the built-in decoder accepts.
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// Turns the stored bytes of a library bitmap into pixels.
///
/// Anything the evaluator cannot decode is drawn as a grey placeholder, so a provider just returns `None`.
pub trait ImageProvider {
    /// Decodes the library item `name` from its stored `bytes`. The result is premultiplied RGBA8.
    fn decode(&mut self, name: &str, bytes: &[u8]) -> Option<Image>;
}

/// A provider that decodes nothing; bitmaps become placeholders unless images were added with
/// [`Evaluator::add_image`](crate::Evaluator::add_image).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDecoder;

impl ImageProvider for NoDecoder {
    fn decode(&mut self, _name: &str, _bytes: &[u8]) -> Option<Image> {
        None
    }
}

/// A provider for PNG data, with a pixel cap. Other formats return `None`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PngDecoder;

impl ImageProvider for PngDecoder {
    fn decode(&mut self, _name: &str, bytes: &[u8]) -> Option<Image> {
        decode_png(bytes)
    }
}

/// Decodes a PNG into premultiplied RGBA8. `None` for anything that is not a sane PNG.
#[must_use]
pub fn decode_png(bytes: &[u8]) -> Option<Image> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let (w, h) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if u64::from(w).checked_mul(u64::from(h))? > MAX_PIXELS {
        return None;
    }
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let data = buf.get(..frame.buffer_size())?;
    let mut out = Vec::with_capacity(Image::byte_len(w, h)?);
    match frame.color_type {
        png::ColorType::Rgba => {
            for &[r, g, b, a] in data.as_chunks::<4>().0 {
                out.extend_from_slice(&[premul(r, a), premul(g, a), premul(b, a), a]);
            }
        }
        png::ColorType::Rgb => {
            for &[r, g, b] in data.as_chunks::<3>().0 {
                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for &[v, a] in data.as_chunks::<2>().0 {
                let p = premul(v, a);
                out.extend_from_slice(&[p, p, p, a]);
            }
        }
        png::ColorType::Grayscale => {
            for &v in data {
                out.extend_from_slice(&[v, v, v, 255]);
            }
        }
        png::ColorType::Indexed => return None,
    }
    Image::new(w, h, out).ok()
}

/// `c * a / 255`, rounded.
fn premul(c: u8, a: u8) -> u8 {
    let v = u16::from(c).saturating_mul(u16::from(a)).saturating_add(127) / 255;
    u8::try_from(v).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, width, height);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn rgba_is_premultiplied() {
        let bytes = encode(1, 2, png::ColorType::Rgba, &[255, 0, 0, 128, 10, 20, 30, 255]);
        let img = decode_png(&bytes).unwrap();
        assert_eq!((img.width, img.height), (1, 2));
        assert_eq!(img.premultiplied_rgba, vec![128, 0, 0, 128, 10, 20, 30, 255]);
    }

    #[test]
    fn rgb_and_gray_decode() {
        let rgb = decode_png(&encode(1, 1, png::ColorType::Rgb, &[1, 2, 3])).unwrap();
        assert_eq!(rgb.premultiplied_rgba, vec![1, 2, 3, 255]);
        let gray = decode_png(&encode(1, 1, png::ColorType::Grayscale, &[9])).unwrap();
        assert_eq!(gray.premultiplied_rgba, vec![9, 9, 9, 255]);
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(decode_png(b"not a png").is_none());
        assert!(decode_png(&[]).is_none());
        let mut bytes = encode(1, 1, png::ColorType::Rgba, &[1, 2, 3, 4]);
        bytes.truncate(bytes.len() / 2);
        assert!(decode_png(&bytes).is_none());
    }
}
