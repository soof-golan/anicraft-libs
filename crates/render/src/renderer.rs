//! The public entry points: [`render`] and the reusable [`Renderer`].

use anim_core::{Affine, Rect, Rgba8};
use anim_display::{DisplayList, Image, ImageStore};

use crate::draw::{self, Ctx};
use crate::error::RenderError;
use crate::mip::MipCache;
use crate::options::{Background, RenderOptions};
use crate::pool::Pool;
use crate::px::PxRect;
use crate::stats::RenderStats;
use crate::{bounds, convert};

/// Longest output side in pixels.
pub const MAX_DIMENSION: u32 = 16_384;
/// Most output pixels (width times height).
pub const MAX_PIXELS: usize = 100_000_000;

/// Renders `list` with the pixels in `images`. A one-shot convenience over [`Renderer::render`].
///
/// # Errors
/// [`RenderError`] when the output size is zero or over budget, or the view transform is not finite. Problems
/// inside the display list are skipped, not reported as errors.
pub fn render(list: &DisplayList, images: &ImageStore, options: &RenderOptions) -> Result<Image, RenderError> {
    Renderer::new().render(list, images, options)
}

/// Device-space bounds of everything `list` draws under `view` (filters and stroke widths included), or `None`
/// when it draws nothing. Useful for "zoom to content" and for sizing exports.
#[must_use]
pub fn content_bounds(list: &DisplayList, images: &ImageStore, view: Affine) -> Option<Rect> {
    bounds::nodes(&list.nodes, view, images, 0)
}

/// A rasteriser that keeps its scratch buffers between renders. Not `Sync`-shared: give each worker thread its
/// own, or keep one inside an actor.
#[derive(Debug, Default)]
pub struct Renderer {
    pool: Pool,
    stats: RenderStats,
}

impl Renderer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Renders `list` into a new premultiplied RGBA8 image of `options.width` x `options.height` pixels.
    ///
    /// # Errors
    /// See [`render`].
    pub fn render(&mut self, list: &DisplayList, images: &ImageStore, options: &RenderOptions) -> Result<Image, RenderError> {
        let (width, height) = (options.width, options.height);
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(RenderError::InvalidSize { width, height, max: MAX_DIMENSION });
        }
        let rect = PxRect::from_size(width, height);
        if rect.area() > MAX_PIXELS {
            return Err(RenderError::TooManyPixels { width, height, max: MAX_PIXELS });
        }
        if !options.view.is_finite() {
            return Err(RenderError::InvalidView);
        }
        self.stats = RenderStats::default();
        self.pool.reset_peak();
        let mut root = self.pool.take(rect).ok_or(RenderError::Allocation { width, height })?;
        let root_bytes = self.pool.live_bytes();
        if let Some(color) = backdrop(list, options.background) {
            root.pm.fill(convert::color(color));
        }
        let mut cx = Ctx { images, antialias: options.antialias, pool: &mut self.pool, stats: &mut self.stats, mips: MipCache::default() };
        draw::nodes(&mut cx, &mut root, &list.nodes, options.view, 0);
        self.stats.peak_layer_bytes = self.pool.peak_bytes().saturating_sub(root_bytes);
        let bytes = self.pool.release(root);
        Ok(Image::new(width, height, bytes)?)
    }

    /// Hands a finished image's buffer back so the next render can reuse it instead of allocating.
    pub fn recycle(&mut self, image: Image) {
        self.pool.keep(image.premultiplied_rgba);
    }

    /// What the last [`Renderer::render`] did.
    #[must_use]
    pub const fn stats(&self) -> RenderStats {
        self.stats
    }
}

fn backdrop(list: &DisplayList, background: Background) -> Option<Rgba8> {
    match background {
        Background::FromDisplayList => list.background,
        Background::Transparent => None,
        Background::Solid(color) => Some(color),
    }
    .filter(|color| color.a > 0)
}
