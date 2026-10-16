//! Pixel helpers shared by the importers.

use anim_core::{Affine, Point, num};
use anim_display::Image;

/// `c * a / 255`, rounded.
pub fn mul255(c: u8, a: u8) -> u8 {
    let v = u32::from(c).saturating_mul(u32::from(a)).saturating_add(127) / 255;
    u8::try_from(v).unwrap_or(255)
}

/// Premultiplies straight RGBA8 in place.
pub fn premultiply(rgba: &mut [u8]) {
    for [r, g, b, a] in rgba.as_chunks_mut::<4>().0 {
        if *a != 255 {
            *r = mul255(*r, *a);
            *g = mul255(*g, *a);
            *b = mul255(*b, *a);
        }
    }
}

/// Resamples a straight-alpha RGBA8 image through `xf` (image pixels to canvas) onto the canvas pixel grid,
/// bilinear, returning premultiplied pixels and the top-left corner. `None` when the result would be empty,
/// degenerate, or larger than `max_pixels`.
pub fn resample(src: &[u8], width: u32, height: u32, xf: Affine, max_pixels: u64) -> Option<(Image, Point)> {
    let rect = xf.transform_rect_bbox(anim_core::Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
    let inv = if xf.determinant().abs() > 1e-12 { xf.inverse() } else { return None };
    if !rect.is_finite() {
        return None;
    }
    let x0 = num::floor_f64_to_i32(rect.x0);
    let y0 = num::floor_f64_to_i32(rect.y0);
    let x1 = num::ceil_f64_to_i32(rect.x1);
    let y1 = num::ceil_f64_to_i32(rect.y1);
    let out_w = u32::try_from(x1.checked_sub(x0)?).ok().filter(|w| *w > 0)?;
    let out_h = u32::try_from(y1.checked_sub(y0)?).ok().filter(|h| *h > 0)?;
    if u64::from(out_w).checked_mul(u64::from(out_h))? > max_pixels {
        return None;
    }
    let (sw, sh) = (num::u32_to_usize(width), num::u32_to_usize(height));
    let mut out = Vec::with_capacity(Image::byte_len(out_w, out_h)?);
    let fetch = |ix: i64, iy: i64| -> [f32; 4] {
        let (Ok(ix), Ok(iy)) = (usize::try_from(ix), usize::try_from(iy)) else { return [0.0; 4] };
        if ix >= sw || iy >= sh {
            return [0.0; 4];
        }
        let at = iy.saturating_mul(sw).saturating_add(ix).saturating_mul(4);
        let Some(px) = src.get(at..at.saturating_add(4)) else { return [0.0; 4] };
        let a = f32::from(px.get(3).copied().unwrap_or(0)) / 255.0;
        let ch = |i: usize| f32::from(px.get(i).copied().unwrap_or(0)) / 255.0 * a;
        [ch(0), ch(1), ch(2), a]
    };
    for oy in 0..out_h {
        for ox in 0..out_w {
            let p = inv * Point::new(f64::from(x0) + f64::from(ox) + 0.5, f64::from(y0) + f64::from(oy) + 0.5);
            let (fx, fy) = (p.x - 0.5, p.y - 0.5);
            let (bx, by) = (fx.floor(), fy.floor());
            let (tx, ty) = (num::f64_to_f32(fx - bx), num::f64_to_f32(fy - by));
            let (ix, iy) = (num::round_f64_to_i64(bx), num::round_f64_to_i64(by));
            let c00 = fetch(ix, iy);
            let c10 = fetch(ix.saturating_add(1), iy);
            let c01 = fetch(ix, iy.saturating_add(1));
            let c11 = fetch(ix.saturating_add(1), iy.saturating_add(1));
            let mut px = [0_u8; 4];
            for (i, slot) in px.iter_mut().enumerate() {
                let g = |c: &[f32; 4]| c.get(i).copied().unwrap_or(0.0);
                let lerp = |a: f32, b: f32, t: f32| (b - a).mul_add(t, a);
                let top = lerp(g(&c00), g(&c10), tx);
                let bottom = lerp(g(&c01), g(&c11), tx);
                *slot = num::unit_f32_to_u8(lerp(top, bottom, ty));
            }
            out.extend_from_slice(&px);
        }
    }
    let image = Image::new(out_w, out_h, out).ok()?;
    Some((image, Point::new(f64::from(x0), f64::from(y0))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_scales_colour_by_alpha() {
        let mut px = [200, 100, 50, 128];
        premultiply(&mut px);
        assert_eq!(px, [100, 50, 25, 128]);
        let mut opaque = [9, 8, 7, 255];
        premultiply(&mut opaque);
        assert_eq!(opaque, [9, 8, 7, 255]);
    }

    #[test]
    fn identity_resample_keeps_pixels() {
        let src = [255, 0, 0, 255, 0, 255, 0, 255];
        let (img, at) = resample(&src, 2, 1, Affine::translate((3.0, 4.0)), 100).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(at, Point::new(3.0, 4.0));
        assert_eq!(img.premultiplied_rgba, src);
    }

    #[test]
    fn degenerate_or_huge_transforms_are_refused() {
        let src = [0, 0, 0, 255];
        assert!(resample(&src, 1, 1, Affine::scale(0.0), 100).is_none());
        assert!(resample(&src, 1, 1, Affine::scale(1e6), 100).is_none());
        assert!(resample(&src, 1, 1, Affine::scale(f64::NAN), 100).is_none());
    }
}
