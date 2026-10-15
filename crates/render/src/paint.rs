//! Display-list paints to tiny-skia shaders: solid colours, Flash gradients and bitmap patterns.

use anim_core::num::f64_to_f32;
use anim_core::{Affine, Rgba8};
use anim_display::{GRADIENT_EXTENT, Gradient, ImageId, ImageStore, Interpolation, Paint, Spread};
use tiny_skia::{Color, FilterQuality, GradientStop, LinearGradient, Pattern, PixmapRef, RadialGradient, Shader, SpreadMode, Transform};

use crate::convert;
use crate::mip::{self, MipCache};

/// Why a paint could not become a shader. Both outcomes mean "draw nothing".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintIssue {
    /// A bitmap fill refers to an image that is not in the store (or has no pixels).
    MissingImage,
    /// No stops, a non-invertible matrix or non-finite numbers.
    Degenerate,
}

/// Gradients never have more stops than this (Animate allows 15); extra stops are ignored.
const MAX_STOPS: usize = 256;
/// Sub-segments per colour interval when interpolating in linear RGB through sRGB stops.
const LINEAR_RGB_STEPS: u16 = 32;
/// Focal points are kept strictly inside the gradient circle.
const MAX_FOCAL: f64 = 0.99;

/// The pixels a bitmap is sampled from: the image itself, or a mip level when it is drawn much smaller.
#[derive(Debug, Clone, Copy)]
pub struct Sampled<'a> {
    pub pixmap: PixmapRef<'a>,
    /// 0 for the full-size image; each level is half the size of the one before.
    pub level: u8,
}

impl Sampled<'_> {
    /// Maps level pixels to full-size image pixels.
    pub fn level_to_full(self) -> Affine {
        Affine::scale(mip::level_scale(self.level))
    }
}

/// Whether the image has pixels and a buffer big enough for its size.
pub fn is_usable(image: &anim_display::Image) -> bool {
    PixmapRef::from_bytes(&image.premultiplied_rgba, image.width, image.height).is_some()
}

/// Mip level wanted for `image` drawn through `to_layer` (image pixels to layer pixels).
pub fn wanted_level(image: &anim_display::Image, to_layer: Affine, smoothing: bool) -> u8 {
    if !smoothing {
        return 0;
    }
    let (sx, sy) = convert::axis_scales(to_layer);
    mip::level_for((sx * sy).sqrt(), image.width, image.height)
}

/// Picks the pixels to sample for `id` drawn through `to_layer` (image pixels to layer pixels): a mip level from
/// `mips` when the image is smoothed and drawn at under half size and the level was built, else the image.
pub fn sample<'a>(id: ImageId, to_layer: Affine, smoothing: bool, images: &'a ImageStore, mips: &'a MipCache) -> Result<Sampled<'a>, PaintIssue> {
    let image = images.get(id).ok_or(PaintIssue::MissingImage)?;
    let level = wanted_level(image, to_layer, smoothing);
    if level > 0
        && let Some(mip) = mips.get(id, level)
        && let Some(pixmap) = PixmapRef::from_bytes(&mip.premultiplied_rgba, mip.width, mip.height)
    {
        return Ok(Sampled { pixmap, level });
    }
    let pixmap = PixmapRef::from_bytes(&image.premultiplied_rgba, image.width, image.height).ok_or(PaintIssue::MissingImage)?;
    Ok(Sampled { pixmap, level: 0 })
}

/// Builds the shader for `paint`. `space` maps the paint's local space (the path's own coordinates) into the
/// coordinates of the path handed to tiny-skia; `to_layer` maps local space all the way to layer pixels (the two
/// differ for strokes, where tiny-skia applies part of the matrix itself) and decides bitmap mip levels.
pub fn shader<'a>(paint: &Paint, space: Affine, to_layer: Affine, images: &'a ImageStore, mips: &'a MipCache) -> Result<Shader<'a>, PaintIssue> {
    match paint {
        Paint::Solid(c) => Ok(Shader::SolidColor(convert::color(*c))),
        Paint::Linear(g) => linear(g, space),
        Paint::Radial(g) => radial(g, space),
        Paint::Bitmap { image, transform, repeat, smoothing } => bitmap(*image, space * *transform, to_layer * *transform, *repeat, *smoothing, images, mips),
    }
}

/// A bilinear/nearest pattern over the pixels of `image`, mapped by `to_space` (image pixels to path space).
pub fn bitmap<'a>(
    image: ImageId,
    to_space: Affine,
    to_layer: Affine,
    repeat: bool,
    smoothing: bool,
    images: &'a ImageStore,
    mips: &'a MipCache,
) -> Result<Shader<'a>, PaintIssue> {
    let sampled = sample(image, to_layer, smoothing, images, mips)?;
    let ts = invertible(to_space * sampled.level_to_full())?;
    let spread = if repeat { SpreadMode::Repeat } else { SpreadMode::Pad };
    let quality = if smoothing { FilterQuality::Bilinear } else { FilterQuality::Nearest };
    Ok(Pattern::new(sampled.pixmap, spread, quality, 1.0, ts))
}

const fn spread_mode(spread: Spread) -> SpreadMode {
    match spread {
        Spread::Pad => SpreadMode::Pad,
        Spread::Reflect => SpreadMode::Reflect,
        Spread::Repeat => SpreadMode::Repeat,
    }
}

/// The matrix as a tiny-skia transform, refusing ones that cannot be inverted (a shader through such a matrix
/// would silently draw nothing).
fn invertible(matrix: Affine) -> Result<Transform, PaintIssue> {
    // tiny-skia's own inversion accepts a zero scale (it yields infinities), so judge conditioning here.
    let conditioned = matrix.determinant().abs() > 1e-12 * matrix.frobenius_norm_squared();
    convert::transform(matrix).filter(|ts| conditioned && ts.invert().is_some_and(|inverse| inverse.is_finite())).ok_or(PaintIssue::Degenerate)
}

fn linear(g: &Gradient, space: Affine) -> Result<Shader<'static>, PaintIssue> {
    let stops = stops(g);
    let ts = invertible(space * g.transform)?;
    let extent = f64_to_f32(GRADIENT_EXTENT);
    let start = tiny_skia::Point::from_xy(-extent, 0.0);
    let end = tiny_skia::Point::from_xy(extent, 0.0);
    LinearGradient::new(start, end, stops, spread_mode(g.spread), ts).ok_or(PaintIssue::Degenerate)
}

fn radial(g: &Gradient, space: Affine) -> Result<Shader<'static>, PaintIssue> {
    let stops = stops(g);
    let ts = invertible(space * g.transform)?;
    let extent = f64_to_f32(GRADIENT_EXTENT);
    let focal = if g.focal.is_finite() { g.focal.clamp(-MAX_FOCAL, MAX_FOCAL) } else { 0.0 };
    let start = tiny_skia::Point::from_xy(f64_to_f32(focal * GRADIENT_EXTENT), 0.0);
    let end = tiny_skia::Point::from_xy(0.0, 0.0);
    RadialGradient::new(start, 0.0, end, extent, stops, spread_mode(g.spread), ts).ok_or(PaintIssue::Degenerate)
}

/// Sorted, clamped stops; expanded into short segments when the gradient interpolates in linear RGB, because
/// tiny-skia interpolates in the encoding of the stops it is given.
fn stops(g: &Gradient) -> Vec<GradientStop> {
    let mut raw: Vec<(f32, Rgba8)> = g.stops.iter().filter(|s| s.offset.is_finite()).take(MAX_STOPS).map(|s| (s.offset.clamp(0.0, 1.0), s.color)).collect();
    raw.sort_by(|a, b| a.0.total_cmp(&b.0));
    match g.interpolation {
        Interpolation::Rgb => raw.into_iter().map(|(o, c)| GradientStop::new(o, convert::color(c))).collect(),
        Interpolation::LinearRgb => linear_rgb_stops(&raw),
    }
}

fn linear_rgb_stops(raw: &[(f32, Rgba8)]) -> Vec<GradientStop> {
    let mut out = Vec::with_capacity(raw.len().saturating_mul(usize::from(LINEAR_RGB_STEPS)));
    for (i, &(offset, color)) in raw.iter().enumerate() {
        out.push(GradientStop::new(offset, convert::color(color)));
        let Some(&(next_offset, next)) = raw.get(i.saturating_add(1)) else { continue };
        if next_offset <= offset {
            continue;
        }
        for k in 1..LINEAR_RGB_STEPS {
            // Sub-stops cluster towards both ends of the interval (sin^2 spacing): the sRGB encoding bends
            // sharply near black, where evenly spaced stops would leave a visible error.
            let wave = (std::f32::consts::FRAC_PI_2 * f32::from(k) / f32::from(LINEAR_RGB_STEPS)).sin();
            let t = wave * wave;
            let o = (next_offset - offset).mul_add(t, offset);
            out.push(GradientStop::new(o, lerp_linear(color, next, t)));
        }
    }
    out
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055_f32.mul_add(v.powf(1.0 / 2.4), -0.055) }
}

fn lerp_linear(a: Rgba8, b: Rgba8, t: f32) -> Color {
    let mix = |x: u8, y: u8| {
        let (lx, ly) = (srgb_to_linear(f32::from(x) / 255.0), srgb_to_linear(f32::from(y) / 255.0));
        linear_to_srgb((ly - lx).mul_add(t, lx)).clamp(0.0, 1.0)
    };
    let alpha = (f32::from(b.a) - f32::from(a.a)).mul_add(t, f32::from(a.a)) / 255.0;
    Color::from_rgba(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b), alpha.clamp(0.0, 1.0)).unwrap_or(Color::TRANSPARENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_display::GradientStop as Stop;

    fn gradient(interp: Interpolation) -> Gradient {
        Gradient {
            transform: Affine::IDENTITY,
            stops: vec![Stop { offset: 0.0, color: Rgba8::BLACK }, Stop { offset: 1.0, color: Rgba8::WHITE }],
            spread: Spread::Pad,
            interpolation: interp,
            focal: 0.0,
        }
    }

    #[test]
    fn linear_rgb_expands_stops_and_keeps_endpoints() {
        let rgb = stops(&gradient(Interpolation::Rgb));
        assert_eq!(rgb.len(), 2);
        let lin = stops(&gradient(Interpolation::LinearRgb));
        assert_eq!(lin.len(), usize::from(LINEAR_RGB_STEPS) + 1);
    }

    #[test]
    fn linear_rgb_midpoint_is_brighter_than_srgb_midpoint() {
        let mid = lerp_linear(Rgba8::BLACK, Rgba8::WHITE, 0.5);
        assert!(mid.red() > 0.7 && mid.red() < 0.8, "linear-light 50% grey encodes to about 0.735, got {}", mid.red());
    }

    #[test]
    fn missing_image_is_reported() {
        let store = ImageStore::new();
        let paint = Paint::Bitmap { image: ImageId(3), transform: Affine::IDENTITY, repeat: false, smoothing: true };
        assert_eq!(shader(&paint, Affine::IDENTITY, Affine::IDENTITY, &store, &MipCache::default()).unwrap_err(), PaintIssue::MissingImage);
    }

    #[test]
    fn degenerate_gradients_are_refused() {
        let store = ImageStore::new();
        let mips = MipCache::default();
        let mut g = gradient(Interpolation::Rgb);
        g.transform = Affine::scale(0.0);
        assert_eq!(shader(&Paint::Linear(g.clone()), Affine::IDENTITY, Affine::IDENTITY, &store, &mips).unwrap_err(), PaintIssue::Degenerate);
        g.stops.clear();
        assert_eq!(shader(&Paint::Radial(g), Affine::IDENTITY, Affine::IDENTITY, &store, &mips).unwrap_err(), PaintIssue::Degenerate);
    }

    #[test]
    fn small_smoothed_bitmaps_pick_a_mip_level() {
        let mut store = ImageStore::new();
        let id = store.insert(anim_display::Image::new(8, 8, vec![255; 8 * 8 * 4]).unwrap());
        let mut mips = MipCache::default();
        let image = store.get(id).unwrap();
        assert_eq!(wanted_level(image, Affine::scale(0.2), true), 2);
        assert_eq!(wanted_level(image, Affine::scale(0.2), false), 0, "nearest sampling never uses mips");
        assert_eq!(sample(id, Affine::scale(0.2), true, &store, &mips).unwrap().level, 0, "an unbuilt level falls back to the image");
        assert!(mips.ensure(&store, id, 2));
        let sampled = sample(id, Affine::scale(0.2), true, &store, &mips).unwrap();
        assert_eq!((sampled.level, sampled.pixmap.width()), (2, 2));
        assert_eq!(sampled.level_to_full(), Affine::scale(4.0));
    }
}
