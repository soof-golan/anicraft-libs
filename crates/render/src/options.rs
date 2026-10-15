//! Render options: output size, view transform, background override and anti-aliasing.

use anim_core::num::{ceil_f64_to_i32, f64_to_f32};
use anim_core::{Affine, Rgba8, Size, Vec2};

/// Where the backdrop colour comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Background {
    /// Use [`anim_display::DisplayList::background`] (transparent when it is `None`).
    #[default]
    FromDisplayList,
    /// Transparent, whatever the display list says (exporters with alpha, onion-skin compositing).
    Transparent,
    /// This colour, whatever the display list says (preview checkerboards, "export on white").
    Solid(Rgba8),
}

/// How to rasterise one display list.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Maps stage coordinates to output pixels (y down both ways). Identity renders the stage 1:1 from the
    /// top-left corner.
    pub view: Affine,
    /// Backdrop colour.
    pub background: Background,
    /// Anti-alias path edges. Turn off for pixel-exact tests and hard-edged export.
    pub antialias: bool,
}

impl RenderOptions {
    /// A 1:1 render of `width` x `height` pixels with the display list's own background and anti-aliasing on.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height, view: Affine::IDENTITY, background: Background::FromDisplayList, antialias: true }
    }

    /// A 1:1 render of a stage of `size` (rounded up to whole pixels).
    #[must_use]
    pub fn for_stage(size: Size) -> Self {
        Self::scaled_stage(size, 1.0)
    }

    /// A render of a stage of `size` scaled by `scale` (output rounded up to whole pixels, at least 1x1).
    #[must_use]
    pub fn scaled_stage(size: Size, scale: f64) -> Self {
        let pixels = |extent: f64| u32::try_from(ceil_f64_to_i32((extent * scale).max(1.0))).unwrap_or(1).max(1);
        Self { view: Affine::scale(scale), ..Self::new(pixels(size.width), pixels(size.height)) }
    }

    /// A render that fits the whole stage inside `width` x `height` pixels, centred, keeping the aspect ratio.
    #[must_use]
    pub fn fit_stage(size: Size, width: u32, height: u32) -> Self {
        let (avail_w, avail_h) = (f64::from(width), f64::from(height));
        let scale = if size.width > 0.0 && size.height > 0.0 { (avail_w / size.width).min(avail_h / size.height) } else { 1.0 };
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let offset = Vec2::new(size.width.mul_add(-scale, avail_w) / 2.0, size.height.mul_add(-scale, avail_h) / 2.0);
        Self { view: Affine::translate(offset) * Affine::scale(scale), ..Self::new(width, height) }
    }

    #[must_use]
    pub const fn with_view(mut self, view: Affine) -> Self {
        self.view = view;
        self
    }

    #[must_use]
    pub const fn with_background(mut self, background: Background) -> Self {
        self.background = background;
        self
    }

    #[must_use]
    pub const fn with_antialias(mut self, antialias: bool) -> Self {
        self.antialias = antialias;
        self
    }

    /// The view transform as `f32` coefficients, for callers that hand it to a GPU or to tiny-skia.
    #[must_use]
    pub fn view_f32(&self) -> [f32; 6] {
        self.view.as_coeffs().map(f64_to_f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_core::Point;

    #[test]
    fn stage_sizes_round_up() {
        let o = RenderOptions::for_stage(Size::new(550.4, 400.0));
        assert_eq!((o.width, o.height), (551, 400));
        let o = RenderOptions::scaled_stage(Size::new(100.0, 50.0), 0.5);
        assert_eq!((o.width, o.height), (50, 25));
    }

    #[test]
    fn fit_stage_centres_with_letterbox() {
        let o = RenderOptions::fit_stage(Size::new(100.0, 50.0), 200, 200);
        assert_eq!((o.width, o.height), (200, 200));
        let top_left = o.view * Point::new(0.0, 0.0);
        let bottom_right = o.view * Point::new(100.0, 50.0);
        assert!((top_left.x - 0.0).abs() < 1e-9 && (top_left.y - 50.0).abs() < 1e-9);
        assert!((bottom_right.x - 200.0).abs() < 1e-9 && (bottom_right.y - 150.0).abs() < 1e-9);
    }

    #[test]
    fn degenerate_stage_still_gives_a_valid_size() {
        let o = RenderOptions::fit_stage(Size::new(0.0, 0.0), 10, 10);
        assert!(o.view.is_finite());
        let o = RenderOptions::scaled_stage(Size::new(f64::NAN, 5.0), 1.0);
        assert!(o.width >= 1);
    }
}
