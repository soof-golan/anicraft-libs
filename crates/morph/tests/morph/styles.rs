//! Interpolation of fill and stroke styles.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::float_cmp, clippy::missing_const_for_fn)]

use anim_core::{Affine, Rgba8};
use anim_display::GradientStop;
use anim_morph::style::{lerp_affine, lerp_fill, lerp_stops, lerp_stroke};
use anim_shape::{FillStyle, GradientFill, StrokeStyle};

fn stop(offset: f32, color: Rgba8) -> GradientStop {
    GradientStop { offset, color }
}

fn gradient(stops: Vec<GradientStop>, matrix: Affine) -> GradientFill {
    GradientFill { matrix, stops, ..GradientFill::default() }
}

#[test]
fn colour_endpoints_are_exact() {
    let (a, b) = (Rgba8::new(1, 2, 3, 4), Rgba8::new(250, 240, 230, 220));
    assert_eq!(a.lerp(b, 0.0), a);
    assert_eq!(a.lerp(b, 1.0), b);
    assert_eq!(a.lerp(b, 7.0).r, 255, "overshoot saturates instead of wrapping");
}

#[test]
fn affine_blends_every_coefficient() {
    let a = Affine::new([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let b = Affine::new([3.0, 2.0, -2.0, 5.0, 10.0, 20.0]);
    assert_eq!(lerp_affine(a, b, 0.5).as_coeffs(), [2.0, 1.0, -1.0, 3.0, 5.0, 10.0]);
}

#[test]
fn stops_blend_over_the_union_of_offsets() {
    let a = [stop(0.0, Rgba8::BLACK), stop(1.0, Rgba8::WHITE)];
    let b = [stop(0.0, Rgba8::WHITE), stop(0.5, Rgba8::opaque(255, 0, 0)), stop(1.0, Rgba8::BLACK)];
    let mid = lerp_stops(&a, &b, 0.5);
    let offsets: Vec<f32> = mid.iter().map(|s| s.offset).collect();
    assert_eq!(offsets, vec![0.0, 0.5, 1.0]);
    assert_eq!(mid[0].color, Rgba8::opaque(128, 128, 128));
    // At 0.5 the first gradient is mid grey (128, rounded); the second is red; halfway is (192, 64, 64).
    assert_eq!(mid[1].color, Rgba8::opaque(192, 64, 64));
    assert_eq!(mid[2].color, Rgba8::opaque(128, 128, 128));
    assert_eq!(lerp_stops(&a, &b, 0.0).iter().map(|s| s.color).next(), Some(Rgba8::BLACK));
}

#[test]
fn many_stops_are_capped() {
    let many = |n: u16| -> Vec<GradientStop> { (0..n).map(|i| stop(f32::from(i) / f32::from(n), Rgba8::WHITE)).collect() };
    let mid = lerp_stops(&many(200), &many(150), 0.5);
    assert!(mid.len() <= 33);
    assert!(mid.windows(2).all(|w| w[0].offset < w[1].offset));
}

#[test]
fn gradients_blend_matrix_stops_and_focal() {
    let a = FillStyle::RadialGradient(GradientFill { focal: -1.0, ..gradient(vec![stop(0.0, Rgba8::BLACK), stop(1.0, Rgba8::BLACK)], Affine::IDENTITY) });
    let b = FillStyle::RadialGradient(GradientFill { focal: 1.0, ..gradient(vec![stop(0.0, Rgba8::WHITE), stop(1.0, Rgba8::WHITE)], Affine::scale(3.0)) });
    let FillStyle::RadialGradient(mid) = lerp_fill(&a, &b, 0.5) else { panic!("kind changed") };
    assert_eq!(mid.focal, 0.0);
    assert_eq!(mid.matrix.as_coeffs()[0], 2.0);
    assert_eq!(mid.stops.first().map(|s| s.color), Some(Rgba8::opaque(128, 128, 128)));
}

#[test]
fn solid_fades_into_gradient() {
    let solid = FillStyle::Solid(Rgba8::opaque(255, 0, 0));
    let grad = FillStyle::LinearGradient(gradient(vec![stop(0.0, Rgba8::opaque(0, 0, 255)), stop(1.0, Rgba8::opaque(0, 255, 0))], Affine::scale(2.0)));
    let FillStyle::LinearGradient(start) = lerp_fill(&solid, &grad, 0.0) else { panic!("expected a gradient") };
    assert!(start.stops.iter().all(|s| s.color == Rgba8::opaque(255, 0, 0)));
    let FillStyle::LinearGradient(end) = lerp_fill(&solid, &grad, 1.0) else { panic!("expected a gradient") };
    assert_eq!(end.stops.first().map(|s| s.color), Some(Rgba8::opaque(0, 0, 255)));
    assert_eq!(end.matrix, Affine::scale(2.0));
    // The other direction.
    assert!(matches!(lerp_fill(&grad, &solid, 0.5), FillStyle::LinearGradient(_)));
}

#[test]
fn incompatible_fills_switch_at_half() {
    let linear = FillStyle::LinearGradient(gradient(vec![stop(0.0, Rgba8::BLACK)], Affine::IDENTITY));
    let radial = FillStyle::RadialGradient(gradient(vec![stop(0.0, Rgba8::WHITE)], Affine::IDENTITY));
    assert_eq!(lerp_fill(&linear, &radial, 0.49), linear);
    assert_eq!(lerp_fill(&linear, &radial, 0.5), radial);
}

#[test]
fn stroke_weight_and_colour_blend_while_joins_switch() {
    let a = StrokeStyle::plain(2.0, Rgba8::BLACK);
    let b = StrokeStyle::plain(8.0, Rgba8::WHITE);
    let mid = lerp_stroke(&a, &b, 0.25);
    assert!((mid.weight() - 3.5).abs() < 1.0e-9);
    assert_eq!(mid.solid().fill.solid_color(), Some(Rgba8::opaque(64, 64, 64)));
    assert_eq!(lerp_stroke(&a, &b, 1.0).weight(), 8.0);
}
