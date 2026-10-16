//! Filters on a finished group layer: blur, drop shadow, glow and colour matrix.
//!
//! Sizes in a [`Filter`] are in the group's own pixels, so they are multiplied by how much the group's matrix
//! stretches each axis; a render at twice the zoom blurs twice as far. Shadow offsets use the same scale but keep
//! their direction in device space (a rotated object keeps its shadow falling the same way, as in Flash).
//!
//! Blur is a repeated box filter, as Flash defines it: `quality` is the number of passes (1 = box, 3 = close to
//! gaussian). The box has a real-valued width: pixel `i` becomes the mean of the signal over the window
//! `[i + 0.5 - w/2, i + 0.5 + w/2]`, with partial pixels at the ends, which also gives smooth sub-pixel shifts.
//! Rows and columns are independent, so the work runs in parallel with identical results.

use anim_core::num::{ceil_f64_to_i32, f64_to_f32, usize_to_f64};
use anim_core::{Affine, Rgba8};
use anim_display::Filter;
use rayon::prelude::*;

use crate::convert::axis_scales;
use crate::par;
use crate::pixel::{store, unit};
use crate::pool::Layer;

/// Passes used for shadows and glows, whose display-list variants carry no quality field (3 is Animate's "high").
pub const SHADOW_PASSES: u8 = 3;
/// Blur quality above this adds nothing visible and costs time.
const MAX_PASSES: u8 = 8;
/// Largest blur width or shadow offset honoured, in device pixels.
const MAX_EXTENT: f64 = 2048.0;
/// Largest padding a group's filters may add on any side, in device pixels.
const MAX_REACH: i32 = 2048;
/// Layers with more pixels than this are not blurred or shadowed: the float planes cost 12 bytes per pixel.
pub const MAX_FILTER_PIXELS: usize = 64_000_000;

/// How far (device pixels) the filters can spread a group's pixels beyond its content bounds.
pub fn total_reach(filters: &[Filter], matrix: Affine) -> i32 {
    filters.iter().fold(0_i32, |sum, filter| sum.saturating_add(reach(filter, matrix))).min(MAX_REACH)
}

fn reach(filter: &Filter, matrix: Affine) -> i32 {
    match filter {
        Filter::Blur { x, y, quality } => {
            let (wx, wy) = blur_widths(*x, *y, matrix);
            ceil_f64_to_i32(extent(wx, passes(*quality)).max(extent(wy, passes(*quality)))).saturating_add(1)
        }
        Filter::DropShadow { angle_deg, distance, blur_x, blur_y, .. } => {
            let (wx, wy) = blur_widths(*blur_x, *blur_y, matrix);
            let (dx, dy) = offset(*angle_deg, *distance, matrix);
            let (reach_x, reach_y) = (dx.abs() + extent(wx, SHADOW_PASSES), dy.abs() + extent(wy, SHADOW_PASSES));
            ceil_f64_to_i32(reach_x.max(reach_y)).saturating_add(1)
        }
        Filter::Glow { blur_x, blur_y, .. } => {
            let (wx, wy) = blur_widths(*blur_x, *blur_y, matrix);
            ceil_f64_to_i32(extent(wx, SHADOW_PASSES).max(extent(wy, SHADOW_PASSES))).saturating_add(1)
        }
        Filter::ColorMatrix(_) => 0,
    }
}

/// Applies one filter to the layer's pixels in place. Returns false, leaving the layer as it was, when the layer
/// is too big to filter within the memory budget ([`MAX_FILTER_PIXELS`]); colour matrices always run.
pub fn apply(layer: &mut Layer, filter: &Filter, matrix: Affine) -> bool {
    if !within_budget(layer.rect.area(), filter) {
        return false;
    }
    match filter {
        Filter::Blur { x, y, quality } => {
            let (wx, wy) = blur_widths(*x, *y, matrix);
            blur_rgba(layer, wx, wy, passes(*quality));
        }
        Filter::DropShadow { color, angle_deg, distance, blur_x, blur_y, strength, inner, knockout, hide_object } => {
            let (wx, wy) = blur_widths(*blur_x, *blur_y, matrix);
            let (dx, dy) = offset(*angle_deg, *distance, matrix);
            let look = Look { color: *color, strength: *strength, inner: *inner, knockout: *knockout, hide: *hide_object };
            shadow(layer, &look, Axis { width: wx, shift: dx }, Axis { width: wy, shift: dy });
        }
        Filter::Glow { color, blur_x, blur_y, strength, inner, knockout } => {
            let (wx, wy) = blur_widths(*blur_x, *blur_y, matrix);
            let look = Look { color: *color, strength: *strength, inner: *inner, knockout: *knockout, hide: false };
            shadow(layer, &look, Axis { width: wx, shift: 0.0 }, Axis { width: wy, shift: 0.0 });
        }
        Filter::ColorMatrix(coefficients) => color_matrix(layer, coefficients),
    }
    true
}

/// Whether a layer of `area` pixels may be run through `filter`.
const fn within_budget(area: usize, filter: &Filter) -> bool {
    area <= MAX_FILTER_PIXELS || matches!(filter, Filter::ColorMatrix(_))
}

fn passes(quality: u8) -> u8 {
    quality.clamp(1, MAX_PASSES)
}

fn sanitize(value: f64) -> f64 {
    if value.is_finite() { value.clamp(-MAX_EXTENT, MAX_EXTENT) } else { 0.0 }
}

fn blur_widths(x: f64, y: f64, matrix: Affine) -> (f64, f64) {
    let (sx, sy) = axis_scales(matrix);
    (sanitize(x.abs() * sx).abs(), sanitize(y.abs() * sy).abs())
}

fn offset(angle_deg: f64, distance: f64, matrix: Affine) -> (f64, f64) {
    let (sx, sy) = axis_scales(matrix);
    let angle = if angle_deg.is_finite() { angle_deg.to_radians() } else { 0.0 };
    (sanitize(angle.cos() * distance * sx), sanitize(angle.sin() * distance * sy))
}

/// How far `passes` box passes of `width` spread a pixel in each direction.
fn extent(width: f64, passes: u8) -> f64 {
    if width <= 1.0 { 0.0 } else { width * f64::from(passes) / 2.0 }
}

/// One axis of a blur: the box width and an optional sub-pixel shift (positive = towards +x / +y).
#[derive(Debug, Clone, Copy)]
struct Axis {
    width: f64,
    shift: f64,
}

impl Axis {
    /// A box no wider than a pixel with no shift changes nothing.
    fn is_noop(self) -> bool {
        self.width <= 1.0 && self.shift.abs() < 1e-6
    }

    const fn effective_width(self) -> f64 {
        self.width.max(1.0)
    }
}

/// Scratch buffers for one row of a box blur.
#[derive(Default)]
struct RowScratch {
    prefix: Vec<f64>,
    original: Vec<f32>,
}

/// Truncates towards zero and saturates into `i32`.
#[expect(clippy::as_conversions, clippy::cast_possible_truncation, reason = "float-to-int `as` saturates and maps NaN to 0")]
const fn trunc_to_i32(v: f64) -> i32 {
    v as i32
}

/// Box-filters one row of a single-channel plane in place.
fn box_row(row: &mut [f32], scratch: &mut RowScratch, width: f64, shift: f64) {
    let count = row.len();
    scratch.original.clear();
    scratch.original.extend_from_slice(row);
    scratch.prefix.clear();
    scratch.prefix.push(0.0);
    let mut running = 0.0_f64;
    for value in row.iter() {
        running += f64::from(*value);
        scratch.prefix.push(running);
    }
    let count_f = usize_to_f64(count);
    let half = width / 2.0;
    let (prefix, original) = (&scratch.prefix, &scratch.original);
    let total = prefix.get(count).copied().unwrap_or(0.0);
    // The integral of the piecewise-constant signal from 0 to `t`.
    let cumulative = |t: f64| -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= count_f {
            return total;
        }
        // `t` is positive here, so truncating is flooring, without the library call `floor` costs on baseline x86.
        let cell = trunc_to_i32(t);
        let index = usize::try_from(cell).unwrap_or(0);
        let whole = prefix.get(index).copied().unwrap_or(0.0);
        let part = original.get(index).copied().map_or(0.0, f64::from);
        (t - f64::from(cell)).mul_add(part, whole)
    };
    for (i, out) in row.iter_mut().enumerate() {
        let centre = usize_to_f64(i) + 0.5 - shift;
        *out = f64_to_f32((cumulative(centre + half) - cumulative(centre - half)) / width);
    }
}

/// Swaps rows and columns of a `width` x `height` plane.
fn transpose(src: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0_f32; src.len()];
    if width == 0 {
        return out;
    }
    for (y, row) in src.chunks_exact(width).enumerate() {
        for (x, value) in row.iter().enumerate() {
            if let Some(target) = out.get_mut(x.saturating_mul(height).saturating_add(y)) {
                *target = *value;
            }
        }
    }
    out
}

fn run_rows(data: &mut [f32], row_pixels: usize, axis: Axis, passes: u8) {
    let width = axis.effective_width();
    for pass in 0..passes {
        let shift = if pass == 0 { axis.shift } else { 0.0 };
        par::for_each_row_with(data, row_pixels, RowScratch::default, |scratch, _, row| box_row(row, scratch, width, shift));
    }
}

/// Blurs a `width` x `height` plane with the given horizontal and vertical box.
fn blur(data: &mut Vec<f32>, width: usize, height: usize, horizontal: Axis, vertical: Axis, passes: u8) {
    if !horizontal.is_noop() {
        run_rows(data, width, horizontal, passes);
    }
    if !vertical.is_noop() {
        let mut columns = transpose(data, width, height);
        run_rows(&mut columns, height, vertical, passes);
        *data = transpose(&columns, height, width);
    }
}

fn dimensions(layer: &Layer) -> (usize, usize) {
    (anim_core::num::u32_to_usize(layer.pm.width()), anim_core::num::u32_to_usize(layer.pm.height()))
}

/// One channel of the layer as unit-range floats.
fn channel_plane(layer: &Layer, channel: usize) -> Vec<f32> {
    layer.pm.data().iter().skip(channel).step_by(4).map(|byte| unit(*byte)).collect()
}

/// Blurs the four channels one after another (12 bytes per pixel of scratch instead of 48). Alpha goes first so
/// each colour channel can be clamped to the final alpha, which keeps the premultiplied invariant.
fn blur_rgba(layer: &mut Layer, wx: f64, wy: f64, passes: u8) {
    let (width, height) = dimensions(layer);
    let (horizontal, vertical) = (Axis { width: wx, shift: 0.0 }, Axis { width: wy, shift: 0.0 });
    if horizontal.is_noop() && vertical.is_noop() {
        return;
    }
    for channel in [3, 0, 1, 2] {
        let mut plane = channel_plane(layer, channel);
        blur(&mut plane, width, height, horizontal, vertical, passes);
        for (px, value) in layer.pm.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(&plane) {
            let blurred = anim_core::num::unit_f32_to_u8(*value);
            let alpha = px[3];
            if let Some(slot) = px.get_mut(channel) {
                *slot = if channel == 3 { blurred } else { blurred.min(alpha) };
            }
        }
    }
}

/// Colour, strength and mode shared by drop shadows and glows.
#[derive(Debug, Clone, Copy)]
struct Look {
    color: Rgba8,
    strength: f64,
    inner: bool,
    knockout: bool,
    hide: bool,
}

fn shadow(layer: &mut Layer, look: &Look, horizontal: Axis, vertical: Axis) {
    let (width, height) = dimensions(layer);
    let mut plane = channel_plane(layer, 3);
    blur(&mut plane, width, height, horizontal, vertical, SHADOW_PASSES);
    let strength = f64_to_f32(look.strength.clamp(0.0, 255.0));
    let rgb = [unit(look.color.r), unit(look.color.g), unit(look.color.b)];
    let color_alpha = unit(look.color.a);
    let (inner, cut) = (look.inner, look.knockout || look.hide);
    let knockout = look.knockout;
    layer.pm.data_mut().par_chunks_exact_mut(4).zip(plane.par_iter()).for_each(|(px, blurred)| {
        let Some(&[red, green, blue, alpha]) = px.first_chunk::<4>() else { return };
        let src_alpha = unit(alpha);
        let cover = if inner { 1.0 - blurred } else { *blurred };
        let a = (cover * strength).clamp(0.0, 1.0) * color_alpha;
        let [sr, sg, sb] = [unit(red), unit(green), unit(blue)];
        let [cr, cg, cb] = rgb;
        let out = if inner {
            let inside = a * src_alpha;
            if cut {
                [cr * inside, cg * inside, cb * inside, inside]
            } else {
                [sr.mul_add(1.0 - a, cr * inside), sg.mul_add(1.0 - a, cg * inside), sb.mul_add(1.0 - a, cb * inside), src_alpha]
            }
        } else {
            let visible = if knockout { a * (1.0 - src_alpha) } else { a };
            if cut {
                [cr * visible, cg * visible, cb * visible, visible]
            } else {
                let under = a * (1.0 - src_alpha);
                [cr.mul_add(under, sr), cg.mul_add(under, sg), cb.mul_add(under, sb), src_alpha + under]
            }
        };
        if let Some(target) = px.first_chunk_mut::<4>() {
            store(target, out[0], out[1], out[2], out[3]);
        }
    });
}

fn color_matrix(layer: &mut Layer, coefficients: &[f32; 20]) {
    let matrix = *coefficients;
    par::for_each_pixel(layer.pm.data_mut(), move |px| {
        let [premul_red, premul_green, premul_blue, alpha] = px.map(f32::from);
        if alpha <= 0.0 {
            return;
        }
        // Straight alpha in 0..=255, as Flash's ColorMatrixFilter defines it.
        let input = [premul_red * 255.0 / alpha, premul_green * 255.0 / alpha, premul_blue * 255.0 / alpha, alpha];
        let channel = |first: usize| -> f32 {
            let weights = matrix.get(first..first.saturating_add(5)).unwrap_or(&[0.0; 5]);
            let sum = weights.iter().zip(input).fold(weights.get(4).copied().unwrap_or(0.0), |acc, (weight, value)| weight.mul_add(value, acc));
            sum.clamp(0.0, 255.0) / 255.0
        };
        let (red, green, blue, out_alpha) = (channel(0), channel(5), channel(10), channel(15));
        store(px, red * out_alpha, green * out_alpha, blue * out_alpha, out_alpha);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::Pool;
    use crate::px::PxRect;

    fn layer_with_box(pool: &mut Pool, size: i32, lo: i32, hi: i32, px: [u8; 4]) -> Layer {
        let mut layer = pool.take(PxRect::new(0, 0, size, size)).unwrap();
        let width = usize::try_from(size).unwrap();
        for y in lo..hi {
            for x in lo..hi {
                let at = usize::try_from(y).unwrap() * width + usize::try_from(x).unwrap();
                layer.pm.pixels_mut()[at] = tiny_skia::PremultipliedColorU8::from_rgba(px[0], px[1], px[2], px[3]).unwrap();
            }
        }
        layer
    }

    fn alpha_at(layer: &Layer, x: u32, y: u32) -> u8 {
        layer.pm.pixel(x, y).unwrap().alpha()
    }

    fn total_alpha(layer: &Layer) -> u64 {
        layer.pm.pixels().iter().map(|p| u64::from(p.alpha())).sum()
    }

    #[test]
    fn box_blur_conserves_energy_and_is_symmetric() {
        let mut pool = Pool::default();
        let mut layer = layer_with_box(&mut pool, 41, 18, 23, [255, 255, 255, 255]);
        let before = total_alpha(&layer);
        blur_rgba(&mut layer, 6.0, 6.0, 2);
        let after = total_alpha(&layer);
        assert!(before.abs_diff(after) < before / 100, "energy {before} -> {after}");
        assert_eq!(alpha_at(&layer, 20, 14), alpha_at(&layer, 20, 26));
        assert_eq!(alpha_at(&layer, 14, 20), alpha_at(&layer, 26, 20));
        assert!(alpha_at(&layer, 20, 20) < 255 && alpha_at(&layer, 20, 20) > 100);
        assert!(alpha_at(&layer, 20, 12) > 0);
    }

    #[test]
    fn widths_up_to_one_pixel_do_nothing() {
        let mut pool = Pool::default();
        let mut layer = layer_with_box(&mut pool, 9, 3, 6, [255, 0, 0, 255]);
        let before = layer.pm.data().to_vec();
        blur_rgba(&mut layer, 1.0, 0.5, 3);
        assert_eq!(layer.pm.data(), before.as_slice());
    }

    #[test]
    fn fractional_box_width_is_a_weighted_mean() {
        let mut row = vec![0.0, 0.0, 1.0, 0.0, 0.0];
        let mut scratch = RowScratch::default();
        box_row(&mut row, &mut scratch, 2.5, 0.0);
        // Window of 2.5 around pixel 2 covers it fully (1/2.5); its neighbours see 0.75 of it (0.75/2.5).
        for (got, want) in row.iter().zip([0.0_f32, 0.3, 0.4, 0.3, 0.0]) {
            assert!((got - want).abs() < 1e-5, "{row:?}");
        }
    }

    #[test]
    fn sub_pixel_shift_interpolates() {
        let mut row = vec![0.0, 1.0, 0.0, 0.0];
        let mut scratch = RowScratch::default();
        box_row(&mut row, &mut scratch, 1.0, 0.5);
        assert!((row[1] - 0.5).abs() < 1e-6 && (row[2] - 0.5).abs() < 1e-6 && row[0].abs() < 1e-6, "{row:?}");
    }

    #[test]
    fn drop_shadow_offsets_and_tints() {
        let mut pool = Pool::default();
        let mut layer = layer_with_box(&mut pool, 40, 5, 10, [255, 255, 255, 255]);
        let filter = Filter::DropShadow {
            color: Rgba8::new(0, 0, 255, 255),
            angle_deg: 0.0,
            distance: 20.0,
            blur_x: 0.0,
            blur_y: 0.0,
            strength: 1.0,
            inner: false,
            knockout: false,
            hide_object: false,
        };
        apply(&mut layer, &filter, Affine::IDENTITY);
        let object = layer.pm.pixel(7, 7).unwrap();
        assert_eq!((object.red(), object.alpha()), (255, 255), "the object stays on top");
        let shadow = layer.pm.pixel(27, 7).unwrap();
        assert_eq!((shadow.red(), shadow.green(), shadow.blue(), shadow.alpha()), (0, 0, 255, 255));
        assert_eq!(alpha_at(&layer, 17, 7), 0);
    }

    #[test]
    fn hide_object_and_knockout_leave_only_the_shadow() {
        let mut pool = Pool::default();
        for (knockout, hide) in [(true, false), (false, true)] {
            let mut layer = layer_with_box(&mut pool, 40, 5, 10, [255, 255, 255, 255]);
            let filter = Filter::DropShadow {
                color: Rgba8::BLACK,
                angle_deg: 0.0,
                distance: 3.0,
                blur_x: 0.0,
                blur_y: 0.0,
                strength: 1.0,
                inner: false,
                knockout,
                hide_object: hide,
            };
            apply(&mut layer, &filter, Affine::IDENTITY);
            let overlap = layer.pm.pixel(9, 7).unwrap();
            assert_eq!(overlap.alpha(), if knockout { 0 } else { 255 }, "object and shadow overlap here");
            assert_eq!(overlap.red(), 0, "no white object left (knockout={knockout}, hide={hide})");
            assert_eq!(alpha_at(&layer, 6, 7), 0, "inside the object but beyond the shadow");
            assert_eq!(alpha_at(&layer, 11, 7), 255, "shadow outside the object");
        }
    }

    #[test]
    fn glow_surrounds_the_object() {
        let mut pool = Pool::default();
        let mut layer = layer_with_box(&mut pool, 41, 18, 23, [0, 0, 0, 255]);
        apply(
            &mut layer,
            &Filter::Glow { color: Rgba8::new(255, 255, 0, 255), blur_x: 8.0, blur_y: 8.0, strength: 3.0, inner: false, knockout: false },
            Affine::IDENTITY,
        );
        let ring = layer.pm.pixel(16, 20).unwrap();
        assert!(ring.alpha() > 100 && ring.red() > 100 && ring.blue() == 0);
        assert_eq!(layer.pm.pixel(20, 20).unwrap().red(), 0, "object colour intact");
    }

    #[test]
    fn inner_shadow_stays_inside_the_shape() {
        let mut pool = Pool::default();
        let mut layer = layer_with_box(&mut pool, 40, 10, 30, [255, 255, 255, 255]);
        let filter = Filter::DropShadow {
            color: Rgba8::BLACK,
            angle_deg: 45.0,
            distance: 4.0,
            blur_x: 4.0,
            blur_y: 4.0,
            strength: 1.0,
            inner: true,
            knockout: false,
            hide_object: false,
        };
        apply(&mut layer, &filter, Affine::IDENTITY);
        assert_eq!(alpha_at(&layer, 5, 5), 0, "nothing outside");
        assert_eq!(alpha_at(&layer, 20, 20), 255);
        assert_eq!(layer.pm.pixel(20, 20).unwrap().red(), 255, "centre untouched");
        assert!(layer.pm.pixel(11, 11).unwrap().red() < 200, "top-left inner edge is shaded");
    }

    #[test]
    fn colour_matrix_inverts_straight_colour() {
        let mut pool = Pool::default();
        let mut layer = layer_with_box(&mut pool, 4, 0, 4, [200, 100, 0, 255]);
        #[rustfmt::skip]
        let invert = [
            -1.0, 0.0, 0.0, 0.0, 255.0,
            0.0, -1.0, 0.0, 0.0, 255.0,
            0.0, 0.0, -1.0, 0.0, 255.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ];
        apply(&mut layer, &Filter::ColorMatrix(invert), Affine::IDENTITY);
        let px = layer.pm.pixel(1, 1).unwrap();
        assert_eq!((px.red(), px.green(), px.blue(), px.alpha()), (55, 155, 255, 255));
    }

    #[test]
    fn reach_grows_with_blur_scale() {
        let blur = [Filter::Blur { x: 10.0, y: 4.0, quality: 3 }];
        let small = total_reach(&blur, Affine::IDENTITY);
        let big = total_reach(&blur, Affine::scale(4.0));
        assert!(small >= 15 && big >= 4 * 15 - 4, "{small} {big}");
        assert_eq!(total_reach(&[Filter::ColorMatrix([0.0; 20])], Affine::IDENTITY), 0);
        assert!(total_reach(&[Filter::Blur { x: f64::NAN, y: 1e300, quality: 255 }], Affine::IDENTITY) <= MAX_REACH);
    }

    #[test]
    fn oversized_layers_skip_blurs_but_not_colour_matrices() {
        let blur = Filter::Blur { x: 4.0, y: 4.0, quality: 1 };
        assert!(within_budget(MAX_FILTER_PIXELS, &blur));
        assert!(!within_budget(MAX_FILTER_PIXELS + 1, &blur));
        assert!(within_budget(usize::MAX, &Filter::ColorMatrix([0.0; 20])));
    }

    #[test]
    fn rgba_blur_keeps_pixels_premultiplied() {
        let mut pool = Pool::default();
        let mut layer = layer_with_box(&mut pool, 33, 12, 21, [200, 100, 50, 200]);
        blur_rgba(&mut layer, 7.0, 3.0, 3);
        assert!(layer.pm.data().as_chunks::<4>().0.iter().all(|px| px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]));
        let centre = layer.pm.pixel(16, 16).unwrap();
        assert!(centre.alpha() > 150 && centre.red() > 120, "{centre:?}");
    }
}
