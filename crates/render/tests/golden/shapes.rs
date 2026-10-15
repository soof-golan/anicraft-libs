//! Fills and strokes: coverage, fill rules, caps, joins, dashes, hairlines and stroke-scale modes.

use anim_core::{Affine, BezPath, Rgba8};
use anim_display::{Cap, Fill, FillRule, Group, Join, Node, Paint, StrokeScale, StrokeStyle};
use anim_render::RenderOptions;

use crate::common::*;

fn style(width: f64, cap: Cap, join: Join) -> StrokeStyle {
    StrokeStyle { width, cap, join, ..StrokeStyle::default() }
}

fn is_red(px: [u8; 4]) -> bool {
    px == [255, 0, 0, 255]
}

fn not_white(px: [u8; 4]) -> bool {
    px != [255, 255, 255, 255]
}

/// Content with no mirror symmetry of its own: a lopsided glyph-like shape, a gradient, a stroke and a bitmap.
fn lopsided_content(images: &mut anim_display::ImageStore) -> Vec<Node> {
    let mut glyph = BezPath::new();
    glyph.move_to((4.0, 4.0));
    glyph.line_to((20.0, 4.0));
    glyph.curve_to((26.0, 4.0), (26.0, 14.0), (20.0, 14.0));
    glyph.line_to((10.0, 14.0));
    glyph.line_to((10.0, 28.0));
    glyph.line_to((4.0, 28.0));
    glyph.close_path();
    let tile = images.insert(anim_display::Image::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap());
    vec![
        fill(glyph, Paint::Linear(gradient(4.0, 26.0, RED, BLUE))),
        stroke(line_path(14.0, 20.0, 36.0, 26.0), style(3.0, Cap::Round, Join::Round), BLACK),
        Node::Bitmap(anim_display::Bitmap { transform: Affine::translate((28.0, 4.0)) * Affine::scale(4.0), image: tile, smoothing: false }),
    ]
}

#[test]
fn mirrored_transforms_flip_everything_consistently() {
    // Right-to-left layouts mirror whole runs of glyph outlines, so negative-determinant matrices must work for
    // fills, gradients, strokes and bitmaps alike.
    let mut plain = Scene::new(40, 32);
    let content = lopsided_content(&mut plain.images);
    for node in content.clone() {
        plain.add(node);
    }
    let mut mirrored = Scene::new(40, 32);
    mirrored.images = plain.images.clone();
    mirrored.add(Node::Group(Group {
        transform: Affine::translate((40.0, 0.0)) * Affine::scale_non_uniform(-1.0, 1.0),
        children: content,
        ..Group::default()
    }));
    let (a, b) = (plain.render(), mirrored.render());
    let mut worst = 0_u32;
    let mut differing = 0_u32;
    for y in 0..32 {
        for x in 0..40 {
            let (left, right) = (pixel(&a, x, y), pixel(&b, 39 - x, y));
            let diff = left.iter().zip(right).map(|(p, q)| u32::from(p.abs_diff(q))).max().unwrap_or(0);
            worst = worst.max(diff);
            differing += u32::from(diff > 12);
        }
    }
    assert!(differing <= 6, "{differing} pixels differ by more than 12 (worst {worst})");
    save("shapes_mirrored_a", &a);
    save("shapes_mirrored_b", &b);
}

#[test]
fn path_direction_does_not_change_a_non_zero_fill() {
    let forward = rect_path(4.0, 4.0, 16.0, 12.0);
    let mut reversed = BezPath::new();
    reversed.move_to((4.0, 4.0));
    reversed.line_to((4.0, 12.0));
    reversed.line_to((16.0, 12.0));
    reversed.line_to((16.0, 4.0));
    reversed.close_path();
    let render = |path: BezPath| {
        let mut scene = Scene::new(20, 16);
        scene.add(fill(path, Paint::Solid(RED)));
        scene.render()
    };
    assert_eq!(render(forward), render(reversed));
}

#[test]
fn translucent_backgrounds_composite_and_stay_premultiplied() {
    let mut scene = Scene::with_background(4, 4, None);
    scene.add(solid_rect(0.0, 0.0, 2.0, 4.0, Rgba8::new(0, 0, 255, 128)));
    let options = RenderOptions::for_stage(scene.list.size).with_background(anim_render::Background::Solid(Rgba8::new(255, 0, 0, 128)));
    let img = scene.render_with(&options);
    assert_px(&img, 3, 1, [128, 0, 0, 128], 1);
    // Blue at 50% over red at 50%: alpha 0.75, premultiplied red 0.25, blue 0.5.
    assert_px(&img, 0, 1, [64, 0, 128, 192], 2);
    assert!(img.premultiplied_rgba.chunks_exact(4).all(|p| p[0] <= p[3] && p[2] <= p[3]));
    let clear = scene.render_with(&RenderOptions::for_stage(scene.list.size).with_background(anim_render::Background::Solid(Rgba8::new(255, 0, 0, 0))));
    assert_eq!(pixel(&clear, 3, 1), [0, 0, 0, 0], "a fully transparent backdrop colour is no backdrop");
}

#[test]
fn solid_rect_has_exact_pixels() {
    let mut scene = Scene::new(20, 20);
    scene.add(solid_rect(4.0, 4.0, 12.0, 10.0, RED));
    let img = scene.render();
    assert_eq!(count(&img, is_red), 8 * 6);
    for (x, y, red) in [(4, 4, true), (11, 9, true), (12, 9, false), (3, 4, false), (4, 10, false), (4, 3, false)] {
        let expected = if red { [255, 0, 0, 255] } else { [255, 255, 255, 255] };
        assert_px(&img, x, y, expected, 0);
    }
    assert_eq!((img.width, img.height), (20, 20));
    save("shapes_solid_rect", &img);
}

#[test]
fn fractional_edges_blend_with_the_background() {
    let mut scene = Scene::new(16, 16);
    scene.add(solid_rect(4.5, 4.0, 8.5, 8.0, RED));
    let img = scene.render();
    assert_px(&img, 4, 5, [255, 128, 128, 255], 4);
    assert_px(&img, 8, 5, [255, 128, 128, 255], 4);
    assert_px(&img, 5, 5, [255, 0, 0, 255], 0);
    assert_px(&img, 3, 5, [255, 255, 255, 255], 0);
}

#[test]
fn translucent_fill_blends_over_the_background() {
    let mut scene = Scene::new(8, 8);
    scene.add(solid_rect(0.0, 0.0, 8.0, 8.0, Rgba8::new(0, 0, 255, 128)));
    assert_px(&scene.render(), 3, 3, [127, 127, 255, 255], 2);
    let transparent = scene.render_transparent();
    let px = pixel(&transparent, 3, 3);
    assert_eq!(px[3], 128, "premultiplied output keeps the alpha");
    assert!(px[2] <= px[3] && px[2] >= 126, "blue is premultiplied: {px:?}");
}

#[test]
fn background_can_be_overridden() {
    let mut scene = Scene::with_background(8, 8, Some(Rgba8::opaque(10, 20, 30)));
    scene.add(solid_rect(0.0, 0.0, 4.0, 8.0, RED));
    assert_px(&scene.render(), 6, 3, [10, 20, 30, 255], 0);
    assert_eq!(pixel(&scene.render_transparent(), 6, 3), [0, 0, 0, 0]);
    let options = RenderOptions::for_stage(scene.list.size).with_background(anim_render::Background::Solid(Rgba8::opaque(0, 255, 0)));
    assert_px(&scene.render_with(&options), 6, 3, [0, 255, 0, 255], 0);
    assert_px(&scene.render_with(&options), 1, 3, [255, 0, 0, 255], 0);
}

#[test]
fn view_transform_maps_stage_to_pixels() {
    let mut scene = Scene::new(10, 10);
    scene.add(solid_rect(1.0, 1.0, 3.0, 3.0, RED));
    let options = RenderOptions::new(30, 20).with_view(Affine::translate((10.0, 0.0)) * Affine::scale(2.0));
    let img = scene.render_with(&options);
    assert_eq!((img.width, img.height), (30, 20));
    assert_eq!(count(&img, is_red), 4 * 4);
    assert_px(&img, 12, 2, [255, 0, 0, 255], 0);
    assert_px(&img, 15, 5, [255, 0, 0, 255], 0);
    assert_px(&img, 16, 5, [255, 255, 255, 255], 0);
    assert_px(&img, 11, 2, [255, 255, 255, 255], 0);
}

#[test]
fn fit_stage_letterboxes() {
    let mut scene = Scene::with_background(100, 50, Some(WHITE));
    scene.add(solid_rect(0.0, 0.0, 100.0, 50.0, RED));
    let img = scene.render_with(&RenderOptions::fit_stage(scene.list.size, 40, 40).with_background(anim_render::Background::Solid(BLACK)));
    assert_px(&img, 20, 20, [255, 0, 0, 255], 0);
    assert_px(&img, 20, 2, [0, 0, 0, 255], 0);
    assert_px(&img, 20, 37, [0, 0, 0, 255], 0);
}

#[test]
fn fill_rules_differ_on_nested_same_direction_contours() {
    let mut path = rect_path(2.0, 2.0, 18.0, 18.0);
    path.extend(rect_path(6.0, 6.0, 14.0, 14.0));
    let build = |rule| {
        let mut scene = Scene::new(20, 20);
        scene.add(Node::Fill(Fill { transform: Affine::IDENTITY, path: path.clone(), rule, paint: Paint::Solid(RED) }));
        scene.render()
    };
    let non_zero = build(FillRule::NonZero);
    let even_odd = build(FillRule::EvenOdd);
    assert_px(&non_zero, 10, 10, [255, 0, 0, 255], 0);
    assert_px(&even_odd, 10, 10, [255, 255, 255, 255], 0);
    assert_px(&even_odd, 3, 10, [255, 0, 0, 255], 0);
    assert_eq!(count(&non_zero, is_red), 16 * 16);
    assert_eq!(count(&even_odd, is_red), 16 * 16 - 8 * 8);
    save("shapes_even_odd", &even_odd);
}

#[test]
fn degenerate_fills_draw_nothing() {
    let mut scene = Scene::new(16, 16);
    scene.add(fill(BezPath::new(), Paint::Solid(RED)));
    scene.add(solid_rect(3.0, 3.0, 3.0, 9.0, RED));
    scene.add(solid_rect(3.0, 3.0, 9.0, 3.0, RED));
    let mut line = line_path(0.0, 0.0, 15.0, 15.0);
    line.close_path();
    scene.add(fill(line, Paint::Solid(RED)));
    assert_eq!(count(&scene.render(), not_white), 0);
}

#[test]
fn aliased_rendering_has_only_full_or_no_coverage() {
    let mut scene = Scene::new(32, 32);
    scene.add(solid_circle(16.0, 16.0, 10.3, RED));
    let hard = scene.render_aliased();
    assert_eq!(count(&hard, |px| px != [255, 255, 255, 255] && px != [255, 0, 0, 255]), 0);
    let soft = scene.render();
    assert!(count(&soft, |px| px != [255, 255, 255, 255] && px != [255, 0, 0, 255]) > 20, "anti-aliased edge pixels exist");
    save("shapes_circle_aliased", &hard);
    save("shapes_circle_smooth", &soft);
}

#[test]
fn circle_area_is_right() {
    let mut scene = Scene::new(40, 40);
    scene.add(solid_circle(20.0, 20.0, 12.0, BLACK));
    let img = scene.render();
    let covered: f64 = img.premultiplied_rgba.chunks_exact(4).map(|px| f64::from(255 - px[0]) / 255.0).sum();
    let expected = std::f64::consts::PI * 144.0;
    assert!((covered - expected).abs() < expected * 0.01, "area {covered} vs {expected}");
}

#[test]
fn butt_caps_end_exactly_at_the_endpoints() {
    let mut scene = Scene::new(24, 20);
    scene.add(stroke(line_path(4.0, 10.0, 16.0, 10.0), style(4.0, Cap::None, Join::Miter), RED));
    let img = scene.render();
    assert_eq!(count(&img, is_red), 12 * 4);
    assert_px(&img, 4, 8, [255, 0, 0, 255], 0);
    assert_px(&img, 15, 11, [255, 0, 0, 255], 0);
    assert_px(&img, 3, 10, [255, 255, 255, 255], 0);
    assert_px(&img, 16, 10, [255, 255, 255, 255], 0);
    assert_px(&img, 10, 7, [255, 255, 255, 255], 0);
    assert_px(&img, 10, 12, [255, 255, 255, 255], 0);
}

#[test]
fn square_and_round_caps_extend_by_half_the_width() {
    let line = line_path(6.0, 10.0, 18.0, 10.0);
    let mut square = Scene::new(24, 20);
    square.add(stroke(line.clone(), style(4.0, Cap::Square, Join::Miter), RED));
    let square = square.render();
    assert_eq!(count(&square, is_red), 16 * 4);
    assert_px(&square, 4, 8, [255, 0, 0, 255], 0);
    assert_px(&square, 3, 8, [255, 255, 255, 255], 0);

    let mut round = Scene::new(24, 20);
    round.add(stroke(line, style(4.0, Cap::Round, Join::Miter), RED));
    let round = round.render();
    assert_px(&round, 4, 10, [255, 0, 0, 255], 40);
    assert_px(&round, 3, 10, [255, 255, 255, 255], 0);
    let corner = pixel(&round, 4, 8);
    assert!(corner[1] > 100, "the round cap leaves the corner pixel mostly uncovered: {corner:?}");
    save("shapes_caps", &round);
}

#[test]
fn joins_and_miter_limit_change_the_corner() {
    let mut corner = BezPath::new();
    corner.move_to((4.0, 26.0));
    corner.line_to((16.0, 8.0));
    corner.line_to((28.0, 26.0));
    let render = |join, limit: f64| {
        let mut scene = Scene::new(32, 32);
        scene.add(stroke(corner.clone(), StrokeStyle { width: 5.0, cap: Cap::None, join, miter_limit: limit, ..StrokeStyle::default() }, RED));
        scene.render()
    };
    let ink = |img: &anim_display::Image| -> f64 { img.premultiplied_rgba.chunks_exact(4).map(|px| f64::from(255 - px[1]) / 255.0).sum() };
    let (miter, round, bevel, clipped) = (render(Join::Miter, 10.0), render(Join::Round, 10.0), render(Join::Bevel, 10.0), render(Join::Miter, 1.0));
    assert!(ink(&miter) > ink(&round) && ink(&round) > ink(&bevel), "miter {} round {} bevel {}", ink(&miter), ink(&round), ink(&bevel));
    assert!((ink(&clipped) - ink(&bevel)).abs() < 1.0, "a miter limit of 1 degrades to a bevel");
    save("shapes_joins", &miter);
}

#[test]
fn dashes_alternate_on_and_off() {
    let mut scene = Scene::new(40, 8);
    let dashed = StrokeStyle { width: 2.0, cap: Cap::None, dashes: vec![4.0, 4.0], ..StrokeStyle::default() };
    scene.add(stroke(line_path(0.0, 4.0, 40.0, 4.0), dashed, RED));
    let img = scene.render();
    for (x, on) in [(1, true), (3, true), (5, false), (7, false), (9, true), (13, false), (17, true)] {
        let expected = if on { [255, 0, 0, 255] } else { [255, 255, 255, 255] };
        assert_px(&img, x, 3, expected, 0);
    }
    assert_eq!(count(&img, is_red), 20 * 2);
}

#[test]
fn odd_dash_arrays_repeat() {
    let mut scene = Scene::new(48, 8);
    let dashed = StrokeStyle { width: 2.0, cap: Cap::None, dashes: vec![4.0], ..StrokeStyle::default() };
    scene.add(stroke(line_path(0.0, 4.0, 48.0, 4.0), dashed, RED));
    let img = scene.render();
    assert_px(&img, 2, 3, [255, 0, 0, 255], 0);
    assert_px(&img, 6, 3, [255, 255, 255, 255], 0);
    assert_px(&img, 10, 3, [255, 0, 0, 255], 0);
}

/// Number of rows in column `x` that are not white.
fn thickness_down(img: &anim_display::Image, x: u32) -> usize {
    (0..img.height).filter(|&y| not_white(pixel(img, x, y))).count()
}

fn thickness_across(img: &anim_display::Image, y: u32) -> usize {
    (0..img.width).filter(|&x| not_white(pixel(img, x, y))).count()
}

#[test]
fn hairlines_are_one_pixel_at_any_scale() {
    let render = |width: f64, scale: f64, y: f64| {
        let mut scene = Scene::new(10, 10);
        scene.add(stroke(line_path(1.0, y, 9.0, y), style(width, Cap::None, Join::Miter), BLACK));
        scene.render_with(&RenderOptions::new(100, 100).with_view(Affine::scale(scale)))
    };
    let hairline = render(0.0, 10.0, 4.55);
    assert_eq!(thickness_down(&hairline, 50), 1, "width 0 stays one pixel at 10x zoom");
    assert!(pixel(&hairline, 50, 45)[0] < 40, "and is dark, not faint: {:?}", pixel(&hairline, 50, 45));
    let normal = render(1.0, 10.0, 4.5);
    assert_eq!(thickness_down(&normal, 50), 10, "width 1 scales with the zoom");
    save("shapes_hairline", &hairline);
}

#[test]
fn stroke_scale_none_ignores_zoom_and_normal_follows_it() {
    let render = |scale_mode| {
        let mut scene = Scene::new(10, 10);
        let style = StrokeStyle { width: 2.0, cap: Cap::None, scale: scale_mode, ..StrokeStyle::default() };
        scene.add(stroke(line_path(1.0, 5.0, 9.0, 5.0), style, BLACK));
        scene.render_with(&RenderOptions::new(50, 50).with_view(Affine::scale(5.0)))
    };
    assert_eq!(thickness_down(&render(StrokeScale::None), 25), 2);
    assert_eq!(thickness_down(&render(StrokeScale::Normal), 25), 10);
}

#[test]
fn horizontal_and_vertical_scale_modes_ignore_one_axis() {
    // Stretch x by 4: a vertical line's thickness is measured along x.
    let render = |mode, stretch: (f64, f64), vertical: bool| {
        let mut scene = Scene::new(40, 40);
        let path = if vertical { line_path(5.0, 5.0, 5.0, 30.0) } else { line_path(5.0, 5.0, 30.0, 5.0) };
        let style = StrokeStyle { width: 2.0, cap: Cap::None, scale: mode, ..StrokeStyle::default() };
        let group = Group { transform: Affine::scale_non_uniform(stretch.0, stretch.1), children: vec![stroke(path, style, BLACK)], ..Group::default() };
        scene.add(Node::Group(group));
        scene.render_with(&RenderOptions::new(160, 160))
    };
    // Vertical line at x = 20 after a 4x horizontal stretch.
    let normal = render(StrokeScale::Normal, (4.0, 1.0), true);
    assert_eq!(thickness_across(&normal, 20), 8, "normal follows the horizontal stretch");
    let horizontal = render(StrokeScale::Horizontal, (4.0, 1.0), true);
    assert_eq!(thickness_across(&horizontal, 20), 2, "horizontal mode ignores horizontal scaling");
    let vertical = render(StrokeScale::Vertical, (4.0, 1.0), true);
    assert_eq!(thickness_across(&vertical, 20), 8, "vertical mode still follows horizontal scaling");
    // Horizontal line stretched vertically by 4: thickness is measured along y.
    let vertical_mode = render(StrokeScale::Vertical, (1.0, 4.0), false);
    assert_eq!(thickness_down(&vertical_mode, 20), 2, "vertical mode ignores vertical scaling");
    let horizontal_mode = render(StrokeScale::Horizontal, (1.0, 4.0), false);
    assert_eq!(thickness_down(&horizontal_mode, 20), 8, "horizontal mode still follows vertical scaling");
    save("shapes_scale_modes", &horizontal);
}

#[test]
fn nodes_with_their_own_transform_compose_with_the_view() {
    let mut scene = Scene::new(20, 20);
    scene.add(Node::Fill(Fill {
        transform: Affine::translate((10.0, 10.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2),
        path: rect_path(0.0, 0.0, 6.0, 2.0),
        rule: FillRule::NonZero,
        paint: Paint::Solid(RED),
    }));
    let img = scene.render();
    // The 6x2 rect rotated by 90 degrees about (10, 10) occupies x 8..10, y 10..16.
    assert_eq!(count(&img, is_red), 12);
    assert_px(&img, 8, 10, [255, 0, 0, 255], 2);
    assert_px(&img, 9, 15, [255, 0, 0, 255], 2);
    assert_px(&img, 10, 12, [255, 255, 255, 255], 2);
}
