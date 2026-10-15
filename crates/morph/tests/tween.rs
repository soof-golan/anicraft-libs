//! Behaviour of shape tweens on small hand-made shapes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::redundant_closure_for_method_calls
)]

use anim_core::kurbo::{Circle, PathEl, Shape};
use anim_core::{BezPath, Point, Rgba8};
use anim_morph::{MorphMode, MorphOptions, MorphPlan, ShapeHint, tween, tween_with};
use anim_shape::{EdgeShape, FillStyle, PathInput, StrokeStyle};

fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x0, y0));
    p.line_to((x1, y0));
    p.line_to((x1, y1));
    p.line_to((x0, y1));
    p.close_path();
    p
}

fn filled(path: BezPath, color: Rgba8) -> EdgeShape {
    EdgeShape::from_paths(&[PathInput::new(path, Some(FillStyle::Solid(color)), None)])
}

const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
const BLUE: Rgba8 = Rgba8::opaque(0, 0, 255);

fn square() -> EdgeShape {
    filled(rect_path(0.0, 0.0, 100.0, 100.0), RED)
}

fn circle() -> EdgeShape {
    filled(Circle::new((50.0, 50.0), 50.0).to_path(0.01), RED)
}

fn polygon_points(shape: &EdgeShape) -> Vec<Vec<Point>> {
    shape
        .assemble()
        .fills
        .iter()
        .flat_map(|region| region.contours())
        .map(|contour| {
            let mut pts = Vec::new();
            anim_core::kurbo::flatten(contour.iter(), 0.05, |el| match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => pts.push(p),
                _ => {}
            });
            pts
        })
        .collect()
}

fn area(shape: &EdgeShape) -> f64 {
    shape.assemble().fills.iter().flat_map(|r| r.contours()).map(|c| c.area().abs()).sum()
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a - o).cross(b - o)
}

/// True when two non-adjacent edges of the closed polygon properly cross.
fn self_intersects(pts: &[Point]) -> bool {
    let n = pts.len();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (c, d) = (pts[j], pts[(j + 1) % n]);
            let (d1, d2, d3, d4) = (cross(a, b, c), cross(a, b, d), cross(c, d, a), cross(c, d, b));
            if d1 * d2 < -1.0e-6 && d3 * d4 < -1.0e-6 {
                return true;
            }
        }
    }
    false
}

#[test]
fn square_to_square_is_identity() {
    let a = square();
    let b = square();
    for t in [0.0, 0.25, 0.5, 1.0] {
        let mid = tween(&a, &b, t, &[]);
        let bounds = mid.bounds().unwrap();
        assert!((bounds.x0 - 0.0).abs() < 0.05 && (bounds.x1 - 100.0).abs() < 0.05, "t={t}: {bounds:?}");
        assert!((bounds.y0 - 0.0).abs() < 0.05 && (bounds.y1 - 100.0).abs() < 0.05, "t={t}: {bounds:?}");
        assert!((area(&mid) - 10_000.0).abs() < 1.0, "t={t}: area {}", area(&mid));
        assert_eq!(mid.fill_at(Point::new(50.0, 50.0)), Some(1));
    }
}

#[test]
fn endpoints_return_the_keyframes() {
    let (a, b) = (square(), circle());
    assert_eq!(tween(&a, &b, 0.0, &[]), a);
    assert_eq!(tween(&a, &b, 1.0, &[]), b);
    assert_eq!(tween(&a, &b, -3.0, &[]), a);
    assert_eq!(tween(&a, &b, f64::NAN, &[]), a);
}

#[test]
fn square_to_circle_midpoint_is_sane() {
    let (a, b) = (square(), circle());
    let plan = MorphPlan::new(&a, &b, &MorphOptions::default());
    let mut last_area = area(&a);
    for step in 1..10 {
        let t = f64::from(step) / 10.0;
        let mid = plan.at(t);
        let polys = polygon_points(&mid);
        assert_eq!(polys.len(), 1, "t={t}");
        for poly in &polys {
            assert!(!self_intersects(poly), "self-intersection at t={t}");
        }
        let bounds = mid.bounds().unwrap();
        assert!(bounds.x0 > -1.0 && bounds.x1 < 101.0 && bounds.y0 > -1.0 && bounds.y1 < 101.0, "t={t}: {bounds:?}");
        let now = area(&mid);
        // The circle is smaller than the square, and the tween shrinks monotonically between them.
        assert!(now <= last_area + 1.0, "area grew at t={t}: {now} > {last_area}");
        assert!(now >= area(&b) - 5.0 && now <= area(&a) + 5.0);
        last_area = now;
    }
    // Midway the outline is still around the middle of the box.
    assert_eq!(plan.at(0.5).fill_at(Point::new(50.0, 50.0)), Some(1));
}

#[test]
fn circle_to_square_matches_square_to_circle_reversed() {
    let (a, b) = (square(), circle());
    let forward = tween(&a, &b, 0.3, &[]);
    let backward = tween(&b, &a, 0.7, &[]);
    assert!((area(&forward) - area(&backward)).abs() < 40.0, "{} vs {}", area(&forward), area(&backward));
}

#[test]
fn solid_colours_blend() {
    let a = filled(rect_path(0.0, 0.0, 10.0, 10.0), RED);
    let b = filled(rect_path(0.0, 0.0, 10.0, 10.0), BLUE);
    let mid = tween(&a, &b, 0.5, &[]);
    assert_eq!(mid.fills.len(), 1);
    assert_eq!(mid.fills.first().and_then(FillStyle::solid_color), Some(Rgba8::opaque(128, 0, 128)));
    let quarter = tween(&a, &b, 0.25, &[]);
    assert_eq!(quarter.fills.first().and_then(FillStyle::solid_color), Some(Rgba8::opaque(191, 0, 64)));
}

#[test]
fn alpha_blends_too() {
    let a = filled(rect_path(0.0, 0.0, 10.0, 10.0), Rgba8::new(10, 20, 30, 255));
    let b = filled(rect_path(0.0, 0.0, 10.0, 10.0), Rgba8::new(10, 20, 30, 0));
    let mid = tween(&a, &b, 0.5, &[]);
    assert_eq!(mid.fills.first().and_then(FillStyle::solid_color).map(|c| c.a), Some(128));
}

#[test]
fn hint_pins_corner_to_opposite_corner() {
    let a = filled(rect_path(0.0, 0.0, 100.0, 100.0), RED);
    let b = filled(rect_path(200.0, 0.0, 300.0, 100.0), RED);
    // Without a hint the square just slides: the midpoint is a square at 100..200.
    let slide = tween(&a, &b, 0.5, &[]);
    let bounds = slide.bounds().unwrap();
    assert!((bounds.x0 - 100.0).abs() < 1.0 && (bounds.x1 - 200.0).abs() < 1.0, "{bounds:?}");
    assert!((area(&slide) - 10_000.0).abs() < 50.0);
    // With a hint, A's (0,0) travels to B's (300,100): that corner is at (150,50) midway.
    let hint = ShapeHint::new(Point::new(0.0, 0.0), Point::new(300.0, 100.0));
    let pinned = tween(&a, &b, 0.5, &[hint]);
    let near = pinned.nearest_segment(Point::new(150.0, 50.0), 1.0);
    assert!(near.is_some(), "the hinted point must stay on the outline");
    // And the pinned tween differs from the plain slide.
    assert!(area(&pinned) < area(&slide) - 500.0, "{} vs {}", area(&pinned), area(&slide));
}

#[test]
fn hint_collapse_on_identical_rectangles() {
    let a = filled(rect_path(0.0, 0.0, 200.0, 100.0), RED);
    let plain = tween(&a, &a, 0.5, &[]);
    let hint = ShapeHint::new(Point::new(0.0, 0.0), Point::new(200.0, 100.0));
    let pinned = tween(&a, &a, 0.5, &[hint]);
    assert!(area(&plain) > 19_000.0);
    assert!(area(&pinned) < 4_000.0, "pinned area {}", area(&pinned));
}

#[test]
fn far_away_hints_are_ignored() {
    let (a, b) = (square(), circle());
    let hint = ShapeHint::new(Point::new(5000.0, 5000.0), Point::new(-5000.0, 40.0));
    assert_eq!(tween(&a, &b, 0.5, &[hint]), tween(&a, &b, 0.5, &[]));
}

#[test]
fn contradicting_hints_do_not_break_the_tween() {
    let a = square();
    let hints = [
        ShapeHint::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0)),
        ShapeHint::new(Point::new(100.0, 0.0), Point::new(100.0, 100.0)),
        ShapeHint::new(Point::new(100.0, 100.0), Point::new(100.0, 0.0)),
    ];
    let mid = tween(&a, &a, 0.5, &hints);
    assert!(mid.bounds().is_some());
}

#[test]
fn angular_keeps_corners_and_distributive_resamples() {
    let a = filled(rect_path(0.0, 0.0, 100.0, 100.0), RED);
    let b = filled(rect_path(0.0, 0.0, 200.0, 50.0), RED);
    let angular = tween_with(&a, &b, 0.5, &MorphOptions { mode: MorphMode::Angular, hints: Vec::new() });
    let segments: usize = angular.edges.iter().map(|e| e.segments.len()).sum();
    assert_eq!(segments, 4, "angular blend keeps the four corners");
    let bounds = angular.bounds().unwrap();
    assert!((bounds.x1 - 150.0).abs() < 0.1 && (bounds.y1 - 75.0).abs() < 0.1, "{bounds:?}");
    let distributive = tween_with(&a, &b, 0.5, &MorphOptions::default());
    let more: usize = distributive.edges.iter().map(|e| e.segments.len()).sum();
    assert!(more > 4);
    // Resampling by arc length lets the corners drift, so the two blends differ.
    assert!((area(&distributive) - area(&angular)).abs() > 1.0);
}

#[test]
fn unmatched_contours_shrink_and_grow() {
    let one = filled(rect_path(0.0, 0.0, 100.0, 100.0), RED);
    let mut two_path = rect_path(0.0, 0.0, 100.0, 100.0);
    for el in rect_path(300.0, 0.0, 400.0, 100.0).elements() {
        two_path.push(*el);
    }
    let two = filled(two_path, RED);
    let plan = MorphPlan::new(&one, &two, &MorphOptions::default());
    assert_eq!(plan.unmatched_fills(), (0, 1));
    let mid = plan.at(0.5);
    let polys = polygon_points(&mid);
    assert_eq!(polys.len(), 2);
    let mut areas: Vec<f64> = mid.assemble().fills.iter().flat_map(|r| r.contours()).map(|c| c.area().abs()).collect();
    areas.sort_by(f64::total_cmp);
    // The new square has grown to a quarter of its area (half the size), the old one is unchanged.
    assert!((areas[1] - 10_000.0).abs() < 50.0, "{areas:?}");
    assert!((areas[0] - 2_500.0).abs() < 100.0, "{areas:?}");
    // Backwards the extra square shrinks.
    let plan_back = MorphPlan::new(&two, &one, &MorphOptions::default());
    assert_eq!(plan_back.unmatched_fills(), (1, 0));
}

#[test]
fn tween_from_nothing_grows_from_the_centre() {
    let empty = EdgeShape::default();
    let b = square();
    let mid = tween(&empty, &b, 0.5, &[]);
    let bounds = mid.bounds().unwrap();
    assert!((bounds.x0 - 25.0).abs() < 1.0 && (bounds.x1 - 75.0).abs() < 1.0, "{bounds:?}");
    assert!(tween(&empty, &empty, 0.5, &[]).is_empty());
}

#[test]
fn donuts_keep_their_holes() {
    let donut = |outer: f64, inner: f64, cx: f64| {
        let mut path = Circle::new((cx, 50.0), outer).to_path(0.01);
        for el in Circle::new((cx, 50.0), inner).to_path(0.01).elements() {
            path.push(*el);
        }
        filled(path, RED)
    };
    let a = donut(50.0, 20.0, 50.0);
    let b = donut(40.0, 10.0, 150.0);
    let mid = tween(&a, &b, 0.5, &[]);
    assert_eq!(polygon_points(&mid).len(), 2);
    // Midway: centre (100,50), outer radius 45, inner radius 15.
    assert_eq!(mid.fill_at(Point::new(100.0, 50.0)), None, "hole stays open");
    assert_eq!(mid.fill_at(Point::new(100.0 + 30.0, 50.0)), Some(1));
    assert_eq!(mid.fill_at(Point::new(100.0 + 60.0, 50.0)), None);
}

#[test]
fn strokes_interpolate_weight_position_and_colour() {
    let line = |y: f64, weight: f64, color: Rgba8| {
        let mut p = BezPath::new();
        p.move_to((0.0, y));
        p.line_to((100.0, y));
        EdgeShape::from_paths(&[PathInput::new(p, None, Some(StrokeStyle::plain(weight, color)))])
    };
    let a = line(0.0, 2.0, RED);
    let b = line(50.0, 10.0, BLUE);
    let mid = tween(&a, &b, 0.5, &[]);
    let stroke = mid.strokes.first().map(StrokeStyle::solid).expect("a stroke");
    assert!((stroke.weight - 6.0).abs() < 1.0e-9);
    assert_eq!(stroke.fill.solid_color(), Some(Rgba8::opaque(128, 0, 128)));
    let bounds = mid.bounds().unwrap();
    assert!((bounds.y0 - 25.0).abs() < 0.1 && (bounds.y1 - 25.0).abs() < 0.1, "{bounds:?}");
    assert!((bounds.x1 - 100.0).abs() < 0.1);
}

#[test]
fn closed_stroke_outline_morphs_with_its_fill() {
    let styled = |path: BezPath| EdgeShape::from_paths(&[PathInput::new(path, Some(FillStyle::Solid(RED)), Some(StrokeStyle::plain(4.0, BLUE)))]);
    let a = styled(rect_path(0.0, 0.0, 100.0, 100.0));
    let b = styled(Circle::new((50.0, 50.0), 50.0).to_path(0.01));
    let plan = MorphPlan::new(&a, &b, &MorphOptions::default());
    assert_eq!((plan.fill_contours(), plan.stroke_contours()), (1, 1));
    let mid = plan.at(0.5);
    assert_eq!(mid.fills.len(), 1);
    assert_eq!(mid.strokes.len(), 1);
    assert!(mid.hit_test(Point::new(50.0, 50.0), 1.0).is_some());
}

#[test]
fn different_fill_styles_match_on_style_when_geometry_is_alike() {
    // Red left, blue right; the tween target has the colours swapped in position: matching by position wins over
    // colour, so each square keeps its place and its colour fades.
    let both = |left: Rgba8, right: Rgba8| {
        EdgeShape::from_paths(&[
            PathInput::new(rect_path(0.0, 0.0, 50.0, 50.0), Some(FillStyle::Solid(left)), None),
            PathInput::new(rect_path(100.0, 0.0, 150.0, 50.0), Some(FillStyle::Solid(right)), None),
        ])
    };
    let a = both(RED, BLUE);
    let b = both(BLUE, RED);
    let mid = tween(&a, &b, 0.5, &[]);
    let colours: Vec<Rgba8> = mid.fills.iter().filter_map(FillStyle::solid_color).collect();
    assert_eq!(colours, vec![Rgba8::opaque(128, 0, 128)]);
    assert_eq!(polygon_points(&mid).len(), 2);
}

#[test]
fn plan_is_reusable_and_deterministic() {
    let (a, b) = (square(), circle());
    let plan = MorphPlan::new(&a, &b, &MorphOptions::default());
    assert_eq!(plan.at(0.4), plan.at(0.4));
    assert_eq!(plan.at(0.4), tween(&a, &b, 0.4, &[]));
}
