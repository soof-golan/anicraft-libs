//! Random and degenerate shapes must never panic and always give finite geometry.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::suboptimal_flops)]

use anim_core::{BezPath, Point, Rgba8};
use anim_morph::{MorphMode, MorphOptions, MorphPlan, ShapeHint, tween};
use anim_shape::{EdgeShape, FillStyle, PathInput, StrokeStyle};
use proptest::prelude::*;

fn polygon(points: &[(f64, f64)], close: bool) -> BezPath {
    let mut p = BezPath::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        if i == 0 {
            p.move_to((x, y));
        } else {
            p.line_to((x, y));
        }
    }
    if close {
        p.close_path();
    }
    p
}

fn shape(points: &[(f64, f64)], color: Rgba8) -> EdgeShape {
    EdgeShape::from_paths(&[PathInput::new(polygon(points, true), Some(FillStyle::Solid(color)), Some(StrokeStyle::plain(2.0, Rgba8::BLACK)))])
}

fn finite(shape: &EdgeShape) -> bool {
    shape.bounds_with_strokes().is_none_or(|b| b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite())
}

fn points() -> impl Strategy<Value = Vec<(f64, f64)>> {
    proptest::collection::vec((-500.0..500.0f64, -500.0..500.0f64), 3..14)
}

proptest! {
    #[test]
    fn random_polygons_tween_without_panicking(a in points(), b in points(), t in 0.0..1.0f64, angular in any::<bool>(), hint in (-500.0..500.0f64, -500.0..500.0f64, -500.0..500.0f64, -500.0..500.0f64)) {
        let (sa, sb) = (shape(&a, Rgba8::opaque(255, 0, 0)), shape(&b, Rgba8::opaque(0, 0, 255)));
        let options = MorphOptions {
            mode: if angular { MorphMode::Angular } else { MorphMode::Distributive },
            hints: vec![ShapeHint::new(Point::new(hint.0, hint.1), Point::new(hint.2, hint.3))],
        };
        let mid = MorphPlan::new(&sa, &sb, &options).at(t);
        prop_assert!(finite(&mid));
    }

    #[test]
    fn hints_at_polygon_vertices_never_panic(a in points(), b in points(), i in 0usize..14, j in 0usize..14, t in 0.0..1.0f64) {
        let (sa, sb) = (shape(&a, Rgba8::BLACK), shape(&b, Rgba8::WHITE));
        let pa = a[i % a.len()];
        let pb = b[j % b.len()];
        let hint = ShapeHint::new(Point::new(pa.0, pa.1), Point::new(pb.0, pb.1));
        let mid = tween(&sa, &sb, t, &[hint, hint]);
        prop_assert!(finite(&mid));
    }
}

#[test]
fn degenerate_inputs() {
    let empty = EdgeShape::default();
    let dot = shape(&[(5.0, 5.0), (5.0, 5.0), (5.0, 5.0)], Rgba8::BLACK);
    let line = shape(&[(0.0, 0.0), (100.0, 0.0), (50.0, 0.0)], Rgba8::BLACK);
    let sq = shape(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], Rgba8::WHITE);
    for (a, b) in [(&empty, &dot), (&dot, &sq), (&line, &sq), (&sq, &line), (&line, &line), (&empty, &empty)] {
        for t in [0.0, 0.3, 1.0, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(finite(&tween(a, b, t, &[])));
        }
    }
}

#[test]
fn many_contours_use_the_fallback_matcher() {
    let grid = |offset: f64| {
        let inputs: Vec<PathInput> = (0..40)
            .flat_map(|i| (0..40).map(move |j| (i, j)))
            .map(|(i, j)| {
                let (x, y) = (f64::from(i) * 12.0 + offset, f64::from(j) * 12.0);
                PathInput::new(polygon(&[(x, y), (x + 8.0, y), (x + 8.0, y + 8.0), (x, y + 8.0)], true), Some(FillStyle::Solid(Rgba8::BLACK)), None)
            })
            .collect();
        EdgeShape::from_paths(&inputs)
    };
    let (a, b) = (grid(0.0), grid(3.0));
    let plan = MorphPlan::new(&a, &b, &MorphOptions::default());
    assert_eq!(plan.fill_contours(), 1600);
    assert!(finite(&plan.at(0.5)));
}
