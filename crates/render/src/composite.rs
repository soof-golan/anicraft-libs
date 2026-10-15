//! Compositing a finished layer onto its parent: Flash blend modes and colour transforms.
//!
//! Blend modes that tiny-skia has are mapped one to one. Flash's `Subtract`, `Invert`, `Alpha` and `Erase` are
//! implemented here on premultiplied pixels with the W3C general compositing formula, where the blend function
//! `B(backdrop, source)` is `max(backdrop - source, 0)` for subtract and `1 - backdrop` for invert; alpha and
//! erase scale the backdrop by the source alpha or its complement.

use anim_core::num::u32_to_usize;
use anim_core::{BlendMode, ColorTransform, PremulRgba};
use tiny_skia::{FilterQuality, PixmapPaint, Transform};

use crate::par;
use crate::pixel::{scale, store, unit};
use crate::pool::Layer;

/// How a group's pixels are put on its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composite {
    /// A mode tiny-skia implements.
    Skia(tiny_skia::BlendMode),
    /// Flash subtract: backdrop minus source, floored at 0.
    Subtract,
    /// Flash invert: inverts the backdrop where the source is opaque.
    Invert,
    /// Flash alpha: the parent layer keeps only what the source covers.
    Alpha,
    /// Flash erase: the parent layer loses what the source covers.
    Erase,
}

impl Composite {
    /// The compositing operation for a display-list blend mode.
    pub const fn of(mode: BlendMode) -> Self {
        use tiny_skia::BlendMode as Skia;
        match mode {
            BlendMode::Normal | BlendMode::PassThrough | BlendMode::Layer => Self::Skia(Skia::SourceOver),
            BlendMode::Multiply => Self::Skia(Skia::Multiply),
            BlendMode::Screen => Self::Skia(Skia::Screen),
            BlendMode::Overlay => Self::Skia(Skia::Overlay),
            BlendMode::Darken => Self::Skia(Skia::Darken),
            BlendMode::Lighten => Self::Skia(Skia::Lighten),
            BlendMode::ColorDodge => Self::Skia(Skia::ColorDodge),
            BlendMode::ColorBurn => Self::Skia(Skia::ColorBurn),
            BlendMode::HardLight => Self::Skia(Skia::HardLight),
            BlendMode::SoftLight => Self::Skia(Skia::SoftLight),
            BlendMode::Difference => Self::Skia(Skia::Difference),
            BlendMode::Exclusion => Self::Skia(Skia::Exclusion),
            BlendMode::Hue => Self::Skia(Skia::Hue),
            BlendMode::Saturation => Self::Skia(Skia::Saturation),
            BlendMode::Color => Self::Skia(Skia::Color),
            BlendMode::Luminosity => Self::Skia(Skia::Luminosity),
            BlendMode::Add => Self::Skia(Skia::Plus),
            BlendMode::Subtract => Self::Subtract,
            BlendMode::Invert => Self::Invert,
            BlendMode::Alpha => Self::Alpha,
            BlendMode::Erase => Self::Erase,
        }
    }
}

/// Whether a group with this blend mode must be composited as a unit. `Normal` and `PassThrough` groups draw
/// their children straight onto the parent (source-over is associative, so the result is the same).
pub const fn needs_isolation(mode: BlendMode) -> bool {
    !matches!(mode, BlendMode::Normal | BlendMode::PassThrough)
}

/// Puts `src` on `dst` (both in the same device space; each layer knows where it sits) with `how`.
pub fn composite(dst: &mut Layer, src: &Layer, how: Composite) {
    match how {
        Composite::Skia(blend_mode) => {
            if dst.rect.intersect(&src.rect).is_none() {
                return;
            }
            let paint = PixmapPaint { opacity: 1.0, blend_mode, quality: FilterQuality::Nearest };
            let (dx, dy) = (src.rect.x0.saturating_sub(dst.rect.x0), src.rect.y0.saturating_sub(dst.rect.y0));
            dst.pm.draw_pixmap(dx, dy, src.pm.as_ref(), &paint, Transform::identity(), None);
        }
        Composite::Subtract => blend_overlap(dst, src, subtract_pixel),
        Composite::Invert => blend_overlap(dst, src, invert_pixel),
        Composite::Alpha => keep_covered(dst, src),
        Composite::Erase => blend_overlap(dst, src, erase_pixel),
    }
}

/// Runs `blend(dst_pixel, src_pixel)` over the pixels where the two layers overlap.
fn blend_overlap(dst: &mut Layer, src: &Layer, blend: impl Fn(&mut [u8; 4], [u8; 4])) {
    let Some(overlap) = dst.rect.intersect(&src.rect) else { return };
    let dst_stride = u32_to_usize(dst.rect.width()).saturating_mul(4);
    let src_stride = u32_to_usize(src.rect.width()).saturating_mul(4);
    let len = u32_to_usize(overlap.width()).saturating_mul(4);
    let dst_skip = byte_offset(overlap.x0, dst.rect.x0);
    let src_skip = byte_offset(overlap.x0, src.rect.x0);
    let rows = u32_to_usize(overlap.height());
    let dst_rows = dst.pm.data_mut().chunks_exact_mut(dst_stride).skip(row_index(overlap.y0, dst.rect.y0)).take(rows);
    let src_rows = src.pm.data().chunks_exact(src_stride).skip(row_index(overlap.y0, src.rect.y0)).take(rows);
    for (dst_row, src_row) in dst_rows.zip(src_rows) {
        let (Some(dst_span), Some(src_span)) = (dst_row.get_mut(dst_skip..dst_skip.saturating_add(len)), src_row.get(src_skip..src_skip.saturating_add(len)))
        else {
            continue;
        };
        for (dst_px, src_px) in dst_span.as_chunks_mut::<4>().0.iter_mut().zip(src_span.as_chunks::<4>().0) {
            blend(dst_px, *src_px);
        }
    }
}

/// Flash alpha: pixels of `dst` the source does not cover become transparent, covered pixels are scaled by the
/// source alpha. Unlike the other modes this touches the whole destination.
fn keep_covered(dst: &mut Layer, src: &Layer) {
    let dst_rect = dst.rect;
    let src_stride = u32_to_usize(src.rect.width()).saturating_mul(4);
    let src_height = u32_to_usize(src.rect.height());
    let dst_stride = u32_to_usize(dst_rect.width()).saturating_mul(4);
    for (row, dst_row) in dst.pm.data_mut().chunks_exact_mut(dst_stride).enumerate() {
        let src_y = i32::try_from(row).unwrap_or(i32::MAX).saturating_add(dst_rect.y0).saturating_sub(src.rect.y0);
        let src_row = usize::try_from(src_y).ok().filter(|r| *r < src_height).and_then(|r| {
            let start = r.saturating_mul(src_stride);
            src.pm.data().get(start..start.saturating_add(src_stride))
        });
        for (col, dst_px) in dst_row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let src_x = i32::try_from(col).unwrap_or(i32::MAX).saturating_add(dst_rect.x0).saturating_sub(src.rect.x0);
            let src_alpha = usize::try_from(src_x).ok().and_then(|c| src_row?.get(c.saturating_mul(4).saturating_add(3)).copied());
            match src_alpha {
                Some(alpha) => scale(dst_px, alpha),
                None => *dst_px = [0; 4],
            }
        }
    }
}

/// Byte offset of device column `x` inside a layer whose first column is `origin`.
fn byte_offset(x: i32, origin: i32) -> usize {
    usize::try_from(x.saturating_sub(origin)).unwrap_or(0).saturating_mul(4)
}

fn row_index(y: i32, origin: i32) -> usize {
    usize::try_from(y.saturating_sub(origin)).unwrap_or(0)
}

/// W3C compositing with a separable blend function, on premultiplied colour. `term(backdrop, source, src_alpha,
/// dst_alpha)` returns `src_alpha * dst_alpha * B(Cb, Cs)` for one channel, given the premultiplied channel
/// values.
fn general(dst: &mut [u8; 4], src: [u8; 4], term: impl Fn(f32, f32, f32, f32) -> f32) {
    let src_alpha = unit(src[3]);
    if src_alpha <= 0.0 {
        return;
    }
    let dst_alpha = unit(dst[3]);
    let channel = |backdrop: u8, source: u8| {
        let (backdrop, source) = (unit(backdrop), unit(source));
        source.mul_add(1.0 - dst_alpha, backdrop.mul_add(1.0 - src_alpha, term(backdrop, source, src_alpha, dst_alpha)))
    };
    let out_alpha = src_alpha.mul_add(-dst_alpha, src_alpha + dst_alpha);
    store(dst, channel(dst[0], src[0]), channel(dst[1], src[1]), channel(dst[2], src[2]), out_alpha);
}

fn subtract_pixel(dst: &mut [u8; 4], src: [u8; 4]) {
    general(dst, src, |backdrop, source, src_alpha, dst_alpha| src_alpha.mul_add(backdrop, -(dst_alpha * source)).max(0.0));
}

fn invert_pixel(dst: &mut [u8; 4], src: [u8; 4]) {
    general(dst, src, |backdrop, _source, src_alpha, dst_alpha| src_alpha * (dst_alpha - backdrop));
}

fn erase_pixel(dst: &mut [u8; 4], src: [u8; 4]) {
    scale(dst, u8::MAX.saturating_sub(src[3]));
}

/// Applies a Flash colour transform to every non-transparent pixel of the layer.
///
/// Fully transparent pixels stay transparent whatever the alpha offset: otherwise the result would depend on how
/// big the renderer happened to make the offscreen layer.
pub fn apply_color_transform(layer: &mut Layer, ct: &ColorTransform) {
    if ct.is_identity() {
        return;
    }
    let ct = *ct;
    par::for_each_pixel(layer.pm.data_mut(), move |px| {
        let [red, green, blue, alpha] = *px;
        if alpha == 0 {
            return;
        }
        let out = ct.apply(PremulRgba { r: unit(red), g: unit(green), b: unit(blue), a: unit(alpha) });
        store(px, out.r, out.g, out.b, out.a);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::Pool;
    use crate::px::PxRect;

    fn solid(pool: &mut Pool, rect: PxRect, px: [u8; 4]) -> Layer {
        let mut layer = pool.take(rect).unwrap();
        layer.pm.data_mut().as_chunks_mut::<4>().0.iter_mut().for_each(|p| *p = px);
        layer
    }

    fn pixel(layer: &Layer, x: u32, y: u32) -> [u8; 4] {
        let p = layer.pm.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue(), p.alpha()]
    }

    #[test]
    fn subtract_floors_at_zero_and_keeps_alpha() {
        let mut pool = Pool::default();
        let mut dst = solid(&mut pool, PxRect::new(0, 0, 2, 2), [200, 100, 50, 255]);
        let src = solid(&mut pool, PxRect::new(0, 0, 2, 2), [100, 150, 50, 255]);
        composite(&mut dst, &src, Composite::Subtract);
        assert_eq!(pixel(&dst, 0, 0), [100, 0, 0, 255]);
    }

    #[test]
    fn invert_flips_the_backdrop_under_opaque_source() {
        let mut pool = Pool::default();
        let mut dst = solid(&mut pool, PxRect::new(0, 0, 2, 2), [255, 0, 100, 255]);
        let src = solid(&mut pool, PxRect::new(0, 0, 1, 1), [10, 10, 10, 255]);
        composite(&mut dst, &src, Composite::Invert);
        assert_eq!(pixel(&dst, 0, 0), [0, 255, 155, 255]);
        assert_eq!(pixel(&dst, 1, 1), [255, 0, 100, 255], "outside the source nothing changes");
    }

    #[test]
    fn alpha_keeps_only_the_covered_part_and_erase_removes_it() {
        let mut pool = Pool::default();
        let mut dst = solid(&mut pool, PxRect::new(0, 0, 4, 1), [255, 0, 0, 255]);
        let src = solid(&mut pool, PxRect::new(1, 0, 3, 1), [0, 0, 0, 255]);
        composite(&mut dst, &src, Composite::Alpha);
        assert_eq!((pixel(&dst, 0, 0)[3], pixel(&dst, 1, 0), pixel(&dst, 3, 0)[3]), (0, [255, 0, 0, 255], 0));
        let mut dst = solid(&mut pool, PxRect::new(0, 0, 4, 1), [255, 0, 0, 255]);
        composite(&mut dst, &src, Composite::Erase);
        assert_eq!((pixel(&dst, 0, 0)[3], pixel(&dst, 1, 0)[3], pixel(&dst, 3, 0)[3]), (255, 0, 255));
    }

    #[test]
    fn half_covered_alpha_scales() {
        let mut pool = Pool::default();
        let mut dst = solid(&mut pool, PxRect::new(0, 0, 1, 1), [200, 100, 0, 255]);
        let src = solid(&mut pool, PxRect::new(0, 0, 1, 1), [0, 0, 0, 128]);
        composite(&mut dst, &src, Composite::Alpha);
        assert_eq!(pixel(&dst, 0, 0), [100, 50, 0, 128]);
    }

    #[test]
    fn colour_transform_skips_transparent_pixels_and_scales_the_rest() {
        let mut pool = Pool::default();
        let mut layer = solid(&mut pool, PxRect::new(0, 0, 2, 1), [255, 0, 0, 255]);
        if let Some(p) = layer.pm.pixels_mut().get_mut(1) {
            *p = tiny_skia::PremultipliedColorU8::TRANSPARENT;
        }
        apply_color_transform(&mut layer, &ColorTransform { mul: [0.5, 1.0, 1.0, 1.0], add: [0.0, 0.0, 0.0, 0.5] });
        assert_eq!(pixel(&layer, 0, 0), [128, 0, 0, 255]);
        assert_eq!(pixel(&layer, 1, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn skia_modes_honour_layer_offsets() {
        let mut pool = Pool::default();
        let mut dst = solid(&mut pool, PxRect::new(10, 10, 14, 14), [255, 255, 255, 255]);
        let src = solid(&mut pool, PxRect::new(12, 12, 20, 20), [255, 0, 0, 255]);
        composite(&mut dst, &src, Composite::of(BlendMode::Multiply));
        assert_eq!(pixel(&dst, 1, 1), [255, 255, 255, 255]);
        assert_eq!(pixel(&dst, 2, 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&dst, 3, 3), [255, 0, 0, 255]);
    }
}
