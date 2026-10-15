//! Premultiplied RGBA8 to the encoders' planar YUV, in BT.709 limited range.
//!
//! The arithmetic is integer fixed point (16 fractional bits) so there are no float-to-int casts. The coefficients are
//! the BT.709 matrix scaled to limited range (219 and 224 steps out of 255); 10-bit planes use the same coefficients
//! times four.
//!
//! Alpha: MP4 has no alpha channel, so premultiplied colour is used as is (it equals the picture composited over black).
//! `ProRes` 4444 with alpha stores straight colour, so the colour is un-premultiplied first.

use filmcraft_prores::Frame;

const KY: [i32; 3] = [11966, 40255, 4064];
const KCB: [i32; 3] = [-6596, -22189, 28784];
const KCR: [i32; 3] = [28784, -26145, -2639];
const ONE: i32 = 65536;

fn rgb(p: [u8; 4]) -> [i32; 3] {
    let [r, g, b, _] = p;
    [i32::from(r), i32::from(g), i32::from(b)]
}

const fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    let [a0, a1, a2] = a;
    let [b0, b1, b2] = b;
    [a0.saturating_add(b0), a1.saturating_add(b1), a2.saturating_add(b2)]
}

fn dot(k: [i32; 3], p: [i32; 3]) -> i32 {
    k.iter().zip(p).fold(0_i32, |acc, (k, p)| acc.saturating_add(k.saturating_mul(p)))
}

/// `(dot * scale / (ONE * n)) + bias`, rounded and clamped to `lo..=hi`; `n` is the number of pixels summed in `dot`.
fn quantise(dot: i32, scale: i32, bias: i32, n: i32, lo: i32, hi: i32) -> i32 {
    let denom = ONE.saturating_mul(n.max(1));
    let num = dot.saturating_mul(scale).saturating_add(bias.saturating_mul(denom)).saturating_add(denom / 2);
    num.checked_div(denom).unwrap_or(bias).clamp(lo, hi)
}

fn to_u8(v: i32) -> u8 {
    u8::try_from(v.clamp(0, 255)).unwrap_or(0)
}

fn to_u16(v: i32) -> u16 {
    u16::try_from(v.clamp(0, 1023)).unwrap_or(0)
}

/// 8-bit 4:2:0 planes. `px` is `width * height` pixels; `width` and `height` are even (the caller guarantees it).
/// Planes are cleared and refilled.
pub fn rgba_to_yuv420(px: &[[u8; 4]], width: usize, y: &mut Vec<u8>, u: &mut Vec<u8>, v: &mut Vec<u8>) {
    y.clear();
    u.clear();
    v.clear();
    y.extend(px.iter().map(|&p| to_u8(quantise(dot(KY, rgb(p)), 1, 16, 1, 1, 254))));
    for rows in px.chunks_exact(width.saturating_mul(2).max(2)) {
        let (top, bottom) = rows.split_at_checked(width).unwrap_or((rows, &[]));
        for (a, b) in top.chunks(2).zip(bottom.chunks(2)) {
            let (sum, n) = a.iter().chain(b).fold(([0_i32; 3], 0_i32), |(s, n), &p| (add(s, rgb(p)), n.saturating_add(1)));
            u.push(to_u8(quantise(dot(KCB, sum), 1, 128, n, 1, 254)));
            v.push(to_u8(quantise(dot(KCR, sum), 1, 128, n, 1, 254)));
        }
    }
}

/// Straight (non-premultiplied) colour from premultiplied: `c * 255 / a`, rounded.
fn unpremultiply(pixel: [u8; 4]) -> [u8; 4] {
    let [red, green, blue, alpha] = pixel;
    if alpha == 255 {
        return pixel;
    }
    if alpha == 0 {
        return [0, 0, 0, 0];
    }
    let un = |c: u8| -> u8 {
        let num = u32::from(c).saturating_mul(255).saturating_add(u32::from(alpha) / 2);
        u8::try_from(num.checked_div(u32::from(alpha)).unwrap_or(0).min(255)).unwrap_or(255)
    };
    [un(red), un(green), un(blue), alpha]
}

/// Fills a `ProRes` frame (10-bit; 4:4:4 when `frame.chroma` is 4:4:4, otherwise 4:2:2) from `px`.
/// With `alpha`, colour is un-premultiplied and the alpha plane is written (the frame must have one).
pub fn rgba_to_prores(px: &[[u8; 4]], width: usize, alpha: bool, full_chroma: bool, frame: &mut Frame) {
    let straight: Vec<[u8; 4]>;
    let px = if alpha {
        straight = px.iter().copied().map(unpremultiply).collect();
        &straight
    } else {
        px
    };
    for (d, &p) in frame.y.iter_mut().zip(px) {
        *d = to_u16(quantise(dot(KY, rgb(p)), 4, 64, 1, 4, 1019));
    }
    let group = if full_chroma { 1 } else { 2 };
    let chroma = px.chunks_exact(width.max(1)).flat_map(|row| row.chunks(group));
    for ((cb, cr), c) in frame.cb.iter_mut().zip(frame.cr.iter_mut()).zip(chroma) {
        let (sum, n) = c.iter().fold(([0_i32; 3], 0_i32), |(s, n), &p| (add(s, rgb(p)), n.saturating_add(1)));
        *cb = to_u16(quantise(dot(KCB, sum), 4, 512, n, 4, 1019));
        *cr = to_u16(quantise(dot(KCR, sum), 4, 512, n, 4, 1019));
    }
    if let Some(plane) = frame.alpha.as_mut() {
        for (d, p) in plane.iter_mut().zip(px) {
            let [_, _, _, a] = *p;
            *d = to_u16((i32::from(a).saturating_mul(1023).saturating_add(127)) / 255);
        }
    }
}

#[cfg(test)]
mod tests {
    use filmcraft_prores::ChromaFormat;

    use super::*;

    const PX: [[u8; 4]; 4] = [[255, 255, 255, 255], [0, 0, 0, 255], [255, 0, 0, 255], [0, 255, 0, 255]];

    #[test]
    fn yuv420_of_primaries() {
        let (mut y, mut u, mut v) = (vec![], vec![], vec![]);
        rgba_to_yuv420(&PX, 2, &mut y, &mut u, &mut v);
        assert_eq!(y[0], 235);
        assert_eq!(y[1], 16);
        assert_eq!((u.len(), v.len()), (1, 1));
    }

    #[test]
    fn yuv420_grey_is_neutral_chroma() {
        let grey = vec![[128, 128, 128, 255]; 16];
        let (mut y, mut u, mut v) = (vec![], vec![], vec![]);
        rgba_to_yuv420(&grey, 4, &mut y, &mut u, &mut v);
        assert_eq!((u.len(), v.len(), y.len()), (4, 4, 16));
        assert!(u.iter().chain(&v).all(|c| (127..=129).contains(c)));
    }

    #[test]
    fn prores_444_matches_reference_values() {
        let mut f = Frame::new(2, 2, ChromaFormat::Yuv444, 10, true);
        rgba_to_prores(&PX, 2, true, true, &mut f);
        assert_eq!(f.y[0], 940);
        assert_eq!(f.y[1], 64);
        assert_eq!(f.cb[0], 512);
        assert!(f.cr[2] > 900, "red has high Cr: {}", f.cr[2]);
        assert_eq!(f.alpha.as_ref().map(|a| a[0]), Some(1023));
    }

    #[test]
    fn prores_422_halves_chroma_width_and_handles_odd_widths() {
        let px = vec![[10, 20, 30, 255]; 3 * 2];
        let mut f = Frame::new(3, 2, ChromaFormat::Yuv422, 10, false);
        assert_eq!(f.cb.len(), 4);
        rgba_to_prores(&px, 3, false, false, &mut f);
        assert!(f.cb.iter().all(|&c| c != 0));
    }

    #[test]
    fn alpha_is_unpremultiplied() {
        assert_eq!(unpremultiply([64, 0, 0, 128]), [128, 0, 0, 128]);
        assert_eq!(unpremultiply([9, 9, 9, 0]), [0, 0, 0, 0]);
        assert_eq!(unpremultiply([1, 2, 3, 255]), [1, 2, 3, 255]);
        // Invalid premultiplied input (colour above alpha) saturates instead of wrapping.
        assert_eq!(unpremultiply([255, 0, 0, 10]), [255, 0, 0, 10]);
    }
}
