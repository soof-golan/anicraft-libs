//! Box-filtered mip levels of bitmaps, built on demand and kept for the duration of one render.
//!
//! Bilinear sampling of an image drawn at less than half size skips most of its pixels and aliases (moire on
//! fine patterns, shimmering when animated). Smoothed bitmaps drawn that small are sampled from the mip level
//! whose scale is between one half and one instead, which averages every source pixel in.

use std::collections::BTreeMap;

use anim_core::num::{f64_to_f32, u32_to_usize};
use anim_display::{Image, ImageId, ImageStore};

use crate::par;

/// Deepest level built (an image would be one pixel wide long before this).
const MAX_LEVEL: u8 = 12;
/// Mip levels held at once; further requests fall back to the full-size image.
const MAX_CACHE_BYTES: usize = 256 * 1024 * 1024;

/// Mip levels for the images of one render.
#[derive(Debug, Default)]
pub struct MipCache {
    levels: BTreeMap<(ImageId, u8), Image>,
    bytes: usize,
}

/// The mip level to sample an image of `width` x `height` drawn at `scale` (image pixels to device pixels, the
/// geometric mean of the axes): 0 unless the image is drawn at less than half size.
pub fn level_for(scale: f64, width: u32, height: u32) -> u8 {
    if !scale.is_finite() || scale <= 0.0 || scale >= 0.5 {
        return 0;
    }
    let wanted = u8::try_from(anim_core::num::floor_f64_to_i32((-scale.log2()).min(f64::from(MAX_LEVEL))).clamp(0, i32::from(MAX_LEVEL))).unwrap_or(0);
    let smallest = width.min(height);
    (0..=wanted).take_while(|level| smallest.checked_shr(u32::from(*level)).is_some_and(|side| side >= 1)).last().unwrap_or(0)
}

/// The factor by which a level is smaller than the full image.
pub fn level_scale(level: u8) -> f64 {
    f64::from(1_u32.checked_shl(u32::from(level)).unwrap_or(u32::MAX))
}

impl MipCache {
    /// Makes sure `level` of `id` exists (building the levels below it as needed). Returns false when the image is
    /// unknown or the cache is full, in which case callers use the full-size image.
    pub fn ensure(&mut self, images: &ImageStore, id: ImageId, level: u8) -> bool {
        if level == 0 {
            return true;
        }
        if self.levels.contains_key(&(id, level)) {
            return true;
        }
        let Some(base) = images.get(id) else { return false };
        for current in 1..=level {
            if self.levels.contains_key(&(id, current)) {
                continue;
            }
            let previous = if current == 1 { Some(base) } else { self.levels.get(&(id, current.saturating_sub(1))) };
            let Some(next) = previous.and_then(downsample) else { return false };
            if self.bytes.saturating_add(next.premultiplied_rgba.len()) > MAX_CACHE_BYTES {
                return false;
            }
            self.bytes = self.bytes.saturating_add(next.premultiplied_rgba.len());
            self.levels.insert((id, current), next);
        }
        true
    }

    /// A built level (`level` 0 is never stored: use the image itself).
    pub fn get(&self, id: ImageId, level: u8) -> Option<&Image> {
        self.levels.get(&(id, level))
    }
}

/// Halves an image with a 2x2 box filter on premultiplied pixels (odd edges repeat their last row or column).
fn downsample(src: &Image) -> Option<Image> {
    let (width, height) = (u32_to_usize(src.width), u32_to_usize(src.height));
    if width == 0 || height == 0 || src.premultiplied_rgba.len() < width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    let (out_width, out_height) = (width.div_ceil(2), height.div_ceil(2));
    let stride = width.checked_mul(4)?;
    let mut out = vec![0_u8; out_width.checked_mul(out_height)?.checked_mul(4)?];
    par::for_each_row_with(
        &mut out,
        out_width.checked_mul(4)?,
        || (),
        |(), row_index, row| {
            let top = row_index.saturating_mul(2).min(height.saturating_sub(1));
            let bottom = row_index.saturating_mul(2).saturating_add(1).min(height.saturating_sub(1));
            let (Some(upper), Some(lower)) = (
                src.premultiplied_rgba.get(top.saturating_mul(stride)..top.saturating_add(1).saturating_mul(stride)),
                src.premultiplied_rgba.get(bottom.saturating_mul(stride)..bottom.saturating_add(1).saturating_mul(stride)),
            ) else {
                return;
            };
            let (upper, lower) = (upper.as_chunks::<4>().0, lower.as_chunks::<4>().0);
            for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let left = x.saturating_mul(2).min(width.saturating_sub(1));
                let right = x.saturating_mul(2).saturating_add(1).min(width.saturating_sub(1));
                let taps = [upper.get(left), upper.get(right), lower.get(left), lower.get(right)];
                for (channel, out_value) in px.iter_mut().enumerate() {
                    let sum: u32 = taps.iter().map(|tap| tap.and_then(|p| p.get(channel)).map_or(0, |v| u32::from(*v))).sum();
                    *out_value = u8::try_from(sum.saturating_add(2) / 4).unwrap_or(u8::MAX);
                }
            }
        },
    );
    Image::new(u32::try_from(out_width).ok()?, u32::try_from(out_height).ok()?, out).ok()
}

/// A level-`level` image extent in full-size pixels as `f32`, for building the rectangle a mipped bitmap covers.
pub fn level_extent(full: u32, level: u8) -> f32 {
    f64_to_f32(f64::from(full) / level_scale(level))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(size: u32) -> Image {
        let mut pixels = Vec::new();
        for y in 0..size {
            for x in 0..size {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        Image::new(size, size, pixels).unwrap()
    }

    #[test]
    fn levels_follow_the_scale_and_the_image_size() {
        assert_eq!(level_for(1.0, 256, 256), 0);
        assert_eq!(level_for(0.5, 256, 256), 0);
        assert_eq!(level_for(0.4, 256, 256), 1);
        assert_eq!(level_for(0.25, 256, 256), 2);
        assert_eq!(level_for(0.2, 256, 256), 2);
        assert_eq!(level_for(1.0 / 64.0, 256, 256), 6);
        assert_eq!(level_for(1.0 / 64.0, 4, 4), 2, "a 4x4 image has no levels below 1x1");
        assert_eq!(level_for(f64::NAN, 4, 4), 0);
        assert_eq!(level_for(0.0, 4, 4), 0);
        assert_eq!(level_for(1e-30, 1 << 20, 1 << 20), MAX_LEVEL);
        assert_eq!(level_scale(3), 8.0);
    }

    #[test]
    fn a_checkerboard_averages_to_grey() {
        let mut store = ImageStore::new();
        let id = store.insert(checker(16));
        let mut cache = MipCache::default();
        assert!(cache.ensure(&store, id, 3));
        for level in 1..=3 {
            let img = cache.get(id, level).unwrap();
            assert_eq!(img.width, 16 >> level);
            assert!(img.premultiplied_rgba.as_chunks::<4>().0.iter().all(|p| (126..=129).contains(&p[0]) && p[3] == 255), "level {level}");
        }
        assert!(cache.get(id, 0).is_none());
    }

    #[test]
    fn odd_sizes_and_unknown_images_are_handled() {
        let mut store = ImageStore::new();
        let id = store.insert(checker(5));
        let mut cache = MipCache::default();
        assert!(cache.ensure(&store, id, 2));
        let level2 = cache.get(id, 2).unwrap();
        assert_eq!((level2.width, level2.height), (2, 2));
        assert!(!cache.ensure(&store, ImageId(99), 1));
        assert!(downsample(&Image { width: 4, height: 4, premultiplied_rgba: vec![0; 3] }).is_none());
    }

    #[test]
    fn premultiplied_alpha_is_averaged_correctly() {
        let mut store = ImageStore::new();
        // Left pixel opaque white, right pixel transparent: the average is 50% alpha with premultiplied 128.
        let id = store.insert(Image::new(2, 1, vec![255, 255, 255, 255, 0, 0, 0, 0]).unwrap());
        let mut cache = MipCache::default();
        assert!(cache.ensure(&store, id, 1));
        assert_eq!(cache.get(id, 1).unwrap().premultiplied_rgba, vec![128, 128, 128, 128]);
    }
}
