//! Hostile and degenerate input, determinism, and renderer reuse.

use anim_core::{Affine, BezPath, BlendMode, ColorTransform, Point, Rgba8, Size};
use anim_display::{
    Bitmap, Cap, DisplayList, Fill, FillRule, Filter, Group, Image, ImageId, ImageStore, Join, Mesh, Node, Paint, Stroke, StrokeScale, StrokeStyle,
};
use anim_render::{MAX_DEPTH, MAX_DIMENSION, RenderError, RenderOptions, Renderer};
use proptest::prelude::*;

use crate::common::*;

#[test]
fn render_nodes_matches_rendering_the_same_list() {
    let mut scene = Scene::new(24, 24);
    scene.add(solid_rect(2.0, 2.0, 12.0, 12.0, RED));
    scene.add(with_color(vec![solid_circle(14.0, 14.0, 7.0, BLUE)], ColorTransform::alpha(0.5)));
    let options = RenderOptions::new(24, 24);
    let mut renderer = Renderer::new();
    let whole = renderer.render(&scene.list, &scene.images, &options).unwrap();
    let parts = renderer.render_nodes(&scene.list.nodes, scene.list.background, &scene.images, &options).unwrap();
    assert_eq!(whole, parts);
    let bare = renderer.render_nodes(&scene.list.nodes, None, &scene.images, &options).unwrap();
    assert_eq!(pixel(&bare, 20, 2), [0, 0, 0, 0], "no backdrop unless one is given");
    let solid = renderer.render_nodes(&scene.list.nodes, None, &scene.images, &options.clone().with_background(anim_render::Background::Solid(BLACK))).unwrap();
    assert_eq!(pixel(&solid, 20, 2), [0, 0, 0, 255]);
}

#[test]
fn options_and_stats_round_trip_through_json() {
    let options = RenderOptions::fit_stage(Size::new(320.0, 180.0), 640, 480)
        .with_background(anim_render::Background::Solid(Rgba8::new(1, 2, 3, 4)))
        .with_antialias(false);
    let json = serde_json::to_string(&options).unwrap();
    assert_eq!(serde_json::from_str::<RenderOptions>(&json).unwrap(), options);
    let (_, stats) = Scene::new(4, 4).render_stats();
    let json = serde_json::to_string(&stats).unwrap();
    assert_eq!(serde_json::from_str::<anim_render::RenderStats>(&json).unwrap(), stats);
}

#[test]
fn bad_output_sizes_are_errors() {
    let scene = Scene::new(8, 8);
    let try_size = |w, h| anim_render::render(&scene.list, &scene.images, &RenderOptions::new(w, h));
    assert!(matches!(try_size(0, 8), Err(RenderError::InvalidSize { .. })));
    assert!(matches!(try_size(8, 0), Err(RenderError::InvalidSize { .. })));
    assert!(matches!(try_size(MAX_DIMENSION + 1, 8), Err(RenderError::InvalidSize { .. })));
    assert!(matches!(try_size(MAX_DIMENSION, MAX_DIMENSION), Err(RenderError::TooManyPixels { .. })));
    assert!(try_size(1, 1).is_ok());
}

#[test]
fn non_finite_views_are_errors() {
    let scene = Scene::new(8, 8);
    for coeff in [f64::NAN, f64::INFINITY] {
        let view = Affine::new([coeff, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let result = anim_render::render(&scene.list, &scene.images, &RenderOptions::new(8, 8).with_view(view));
        assert_eq!(result.unwrap_err(), RenderError::InvalidView);
    }
}

#[test]
fn non_finite_geometry_is_skipped_not_fatal() {
    let mut scene = Scene::new(16, 16);
    let mut bad = BezPath::new();
    bad.move_to((0.0, 0.0));
    bad.line_to((f64::NAN, 5.0));
    bad.line_to((10.0, f64::INFINITY));
    scene.add(fill(bad.clone(), Paint::Solid(RED)));
    scene.add(stroke(bad, StrokeStyle::default(), RED));
    scene.add(Node::Fill(Fill {
        transform: Affine::new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]),
        path: rect_path(0.0, 0.0, 5.0, 5.0),
        rule: FillRule::NonZero,
        paint: Paint::Solid(RED),
    }));
    scene.add(solid_rect(2.0, 2.0, 6.0, 6.0, BLUE));
    let (img, stats) = scene.render_stats();
    assert_px(&img, 3, 3, [0, 0, 255, 255], 0);
    assert_eq!(count(&img, |px| px == [255, 0, 0, 255]), 0);
    assert!(stats.invalid >= 3, "{stats:?}");
}

#[test]
fn huge_coordinates_do_not_hang_or_panic() {
    let mut scene = Scene::new(16, 16);
    scene.add(solid_rect(-1e12, -1e12, 1e12, 1e12, GREEN));
    scene.add(stroke(line_path(-1e15, 8.0, 1e15, 8.0), StrokeStyle { width: 2.0, ..StrokeStyle::default() }, RED));
    scene.add(solid_circle(8.0, 8.0, 1e30, BLUE));
    let img = scene.render();
    assert_eq!((img.width, img.height), (16, 16));
}

#[test]
fn degenerate_matrices_are_skipped() {
    let mut scene = Scene::new(16, 16);
    scene.add(Node::Group(Group { transform: Affine::scale(0.0), children: vec![solid_rect(0.0, 0.0, 10.0, 10.0, RED)], ..Group::default() }));
    scene.add(Node::Group(Group {
        transform: Affine::scale(0.0),
        color: ColorTransform::alpha(0.5),
        children: vec![solid_rect(0.0, 0.0, 10.0, 10.0, RED)],
        ..Group::default()
    }));
    let stroked = Stroke {
        transform: Affine::scale_non_uniform(0.0, 1.0),
        path: line_path(0.0, 1.0, 10.0, 1.0),
        style: StrokeStyle { width: 2.0, scale: StrokeScale::Horizontal, ..StrokeStyle::default() },
        paint: Paint::Solid(RED),
    };
    scene.add(Node::Stroke(stroked));
    assert_eq!(count(&scene.render(), |px| px != [255, 255, 255, 255]), 0);
}

#[test]
fn bad_stroke_styles_are_skipped() {
    let mut scene = Scene::new(16, 16);
    for style in [
        StrokeStyle { width: f64::NAN, ..StrokeStyle::default() },
        StrokeStyle { width: -3.0, ..StrokeStyle::default() },
        StrokeStyle { width: f64::INFINITY, ..StrokeStyle::default() },
    ] {
        scene.add(stroke(line_path(0.0, 8.0, 16.0, 8.0), style, RED));
    }
    let (img, stats) = scene.render_stats();
    assert_eq!(count(&img, |px| px != [255, 255, 255, 255]), 0);
    assert_eq!(stats.invalid, 3);
}

#[test]
fn awkward_dash_arrays_are_harmless() {
    let mut scene = Scene::new(32, 32);
    for dashes in [vec![0.0, 0.0], vec![1e-9, 1e-9], vec![f64::NAN, 1.0], vec![-1.0, 1.0], vec![1e300, 1e300], vec![0.001; 5000], vec![1.0]] {
        let style = StrokeStyle { width: 2.0, cap: Cap::Round, join: Join::Round, dashes, ..StrokeStyle::default() };
        scene.add(stroke(line_path(0.0, 8.0, 32.0, 24.0), style, RED));
    }
    let img = scene.render();
    assert_eq!((img.width, img.height), (32, 32));
}

#[test]
fn enormous_stroke_widths_are_handled() {
    let mut scene = Scene::new(16, 16);
    scene.add(stroke(line_path(0.0, 8.0, 16.0, 8.0), StrokeStyle { width: 1e12, miter_limit: 1e300, join: Join::Miter, ..StrokeStyle::default() }, RED));
    assert_eq!(scene.render().width, 16);
}

#[test]
fn missing_images_are_counted_and_skipped() {
    let mut scene = Scene::new(16, 16);
    scene.add(Node::Bitmap(Bitmap { transform: Affine::IDENTITY, image: ImageId(42), smoothing: true }));
    scene.add(fill(rect_path(0.0, 0.0, 16.0, 16.0), Paint::Bitmap { image: ImageId(7), transform: Affine::IDENTITY, repeat: true, smoothing: false }));
    // An image whose buffer is shorter than its size says.
    let id = scene.images.insert(Image { width: 100, height: 100, premultiplied_rgba: vec![0; 8] });
    scene.add(Node::Bitmap(Bitmap { transform: Affine::IDENTITY, image: id, smoothing: false }));
    let zero = scene.images.insert(Image { width: 0, height: 0, premultiplied_rgba: Vec::new() });
    scene.add(Node::Bitmap(Bitmap { transform: Affine::IDENTITY, image: zero, smoothing: false }));
    let (img, stats) = scene.render_stats();
    assert_eq!(stats.missing_images, 4);
    assert_eq!(count(&img, |px| px != [255, 255, 255, 255]), 0);
}

fn nest(depth: usize) -> Node {
    let mut node = solid_rect(0.0, 0.0, 8.0, 8.0, RED);
    for _ in 0..depth {
        node = Node::Group(Group { children: vec![node], ..Group::default() });
    }
    node
}

#[test]
fn deep_nesting_is_cut_off_instead_of_overflowing_the_stack() {
    let mut scene = Scene::new(8, 8);
    scene.add(nest(MAX_DEPTH + 40));
    let (img, stats) = scene.render_stats();
    assert_eq!(count(&img, |px| px == [255, 0, 0, 255]), 0, "content below the depth limit is not drawn");
    assert!(stats.invalid >= 1);

    let mut ok = Scene::new(8, 8);
    ok.add(nest(MAX_DEPTH - 2));
    assert_eq!(count(&ok.render(), |px| px == [255, 0, 0, 255]), 64, "content within the limit is drawn");
}

#[test]
fn deep_nesting_of_isolated_groups_is_bounded_too() {
    let mut node = solid_rect(0.0, 0.0, 8.0, 8.0, RED);
    for _ in 0..(MAX_DEPTH + 10) {
        node = Node::Group(Group { color: ColorTransform::alpha(0.9), children: vec![node], ..Group::default() });
    }
    let mut scene = Scene::new(8, 8);
    scene.add(node);
    let (_, stats) = scene.render_stats();
    assert!(stats.invalid >= 1);
    assert!(stats.peak_layer_bytes <= 8 * 8 * 4 * MAX_DEPTH);
}

#[test]
fn many_sibling_groups_reuse_buffers() {
    let mut scene = Scene::new(64, 64);
    for i in 0..300 {
        let x = f64::from(i % 20) * 3.0;
        scene.add(with_color(vec![solid_rect(x, x, x + 4.0, x + 4.0, RED)], ColorTransform::alpha(0.5)));
    }
    let (_, stats) = scene.render_stats();
    assert_eq!(stats.isolated_groups, 300);
    assert!(stats.peak_layer_bytes <= 8 * 8 * 4 * 2, "layers are recycled between siblings: {}", stats.peak_layer_bytes);
}

fn busy_scene(seed: u64) -> Scene {
    let mut rng = Lcg(seed);
    let mut scene = Scene::new(48, 48);
    let colors = [RED, GREEN, BLUE, BLACK, Rgba8::new(255, 128, 0, 160)];
    for _ in 0..12 {
        let color = colors[usize::try_from(rng.next_u32()).unwrap() % colors.len()];
        let (x, y) = (rng.range(-8.0, 40.0), rng.range(-8.0, 40.0));
        let node = solid_circle(x, y, rng.range(2.0, 14.0), color);
        let wrapped = match rng.next_u32() % 4 {
            0 => with_color(vec![node], ColorTransform::alpha(rng.range(0.1, 1.0) as f32)),
            1 => Node::Group(Group { blend: BlendMode::Multiply, children: vec![node], ..Group::default() }),
            2 => Node::Group(Group {
                filters: vec![Filter::Blur { x: rng.range(0.0, 9.0), y: rng.range(0.0, 9.0), quality: 2 }],
                children: vec![node],
                ..Group::default()
            }),
            _ => node,
        };
        scene.add(wrapped);
    }
    scene
}

#[test]
fn rendering_is_deterministic() {
    let scene = busy_scene(7);
    let first = scene.render();
    for _ in 0..3 {
        assert_eq!(scene.render(), first);
    }
}

#[test]
fn large_parallel_blur_is_deterministic_and_symmetric() {
    let mut scene = Scene::new(900, 700);
    let blur = Filter::Blur { x: 14.0, y: 14.0, quality: 3 };
    scene.add(Node::Group(Group { filters: vec![blur], children: vec![solid_rect(300.0, 250.0, 600.0, 450.0, BLACK)], ..Group::default() }));
    let first = scene.render();
    assert_eq!(scene.render(), first, "work split across threads must not change the result");
    assert_eq!(pixel(&first, 450, 200), pixel(&first, 450, 499));
    assert_eq!(pixel(&first, 270, 350), pixel(&first, 629, 350));
    let mid = pixel(&first, 450, 350);
    assert_eq!(mid, [0, 0, 0, 255], "the middle of a big square stays solid");
}

#[test]
fn a_reused_renderer_gives_the_same_pixels_as_a_fresh_one() {
    let mut renderer = Renderer::new();
    let options = RenderOptions::new(48, 48);
    for seed in [1, 2, 3, 2, 1] {
        let scene = busy_scene(seed);
        let reused = renderer.render(&scene.list, &scene.images, &options).unwrap();
        let fresh = anim_render::render(&scene.list, &scene.images, &options).unwrap();
        assert_eq!(reused, fresh, "seed {seed}: stale pixels must never leak between renders");
        renderer.recycle(reused);
    }
    // And between different sizes.
    let small = Scene::new(8, 8);
    let img = renderer.render(&small.list, &small.images, &RenderOptions::new(8, 8)).unwrap();
    assert_eq!(count(&img, |px| px == [255, 255, 255, 255]), 64);
}

#[test]
fn content_bounds_cover_what_is_drawn() {
    let mut scene = Scene::new(64, 64);
    scene.add(solid_rect(10.0, 20.0, 30.0, 40.0, RED));
    scene.add(stroke(line_path(5.0, 50.0, 50.0, 50.0), StrokeStyle { width: 6.0, cap: Cap::None, ..StrokeStyle::default() }, BLACK));
    let bounds = anim_render::content_bounds(&scene.list, &scene.images, Affine::IDENTITY).unwrap();
    let img = scene.render();
    for y in 0..64 {
        for x in 0..64 {
            if pixel(&img, x, y) != [255, 255, 255, 255] {
                let (fx, fy) = (f64::from(x), f64::from(y));
                assert!(fx + 1.0 > bounds.x0 && fx < bounds.x1 && fy + 1.0 > bounds.y0 && fy < bounds.y1, "({x}, {y}) outside {bounds:?}");
            }
        }
    }
    assert!(anim_render::content_bounds(&DisplayList::new(Size::new(1.0, 1.0), None), &ImageStore::new(), Affine::IDENTITY).is_none());
}

#[test]
fn hand_rolled_fuzz_never_panics() {
    // 120 scenes by default; `ANIM_RENDER_FUZZ=20000 cargo test ...` runs a longer soak.
    let scenes = std::env::var("ANIM_RENDER_FUZZ").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(120);
    for seed in 0..scenes {
        let mut rng = Lcg(seed.wrapping_mul(7919).wrapping_add(13));
        let mut scene = Scene::new(24, 24);
        let id = scene.images.insert(Image::new(2, 2, vec![255; 16]).unwrap());
        let nodes: Vec<Node> = (0..6).map(|_| random_node(&mut rng, 0, id)).collect();
        scene.list.nodes = nodes;
        let options = RenderOptions::new(24, 24).with_view(random_affine(&mut rng));
        let result = anim_render::render(&scene.list, &scene.images, &options);
        match result {
            Ok(img) => assert_eq!((img.width, img.height), (24, 24)),
            Err(err) => assert_eq!(err, RenderError::InvalidView),
        }
    }
}

fn wild(rng: &mut Lcg) -> f64 {
    match rng.next_u32() % 16 {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => -1e30,
        3 => 1e12,
        4 => 0.0,
        // Just inside and just outside the largest coordinate the renderer accepts (1e7 device pixels).
        5 => 9.9e6,
        6 => -9.9e6,
        7 => 1.1e7,
        8 => 1e-9,
        _ => rng.range(-30.0, 60.0),
    }
}

fn random_affine(rng: &mut Lcg) -> Affine {
    match rng.next_u32() % 6 {
        0 | 1 => Affine::new([wild(rng), wild(rng), wild(rng), wild(rng), wild(rng), wild(rng)]),
        // Extreme but legal zoom levels.
        2 => Affine::scale_non_uniform(10_f64.powf(rng.range(-6.0, 5.0)), 10_f64.powf(rng.range(-6.0, 5.0))),
        _ => {
            Affine::translate((rng.range(-20.0, 20.0), rng.range(-20.0, 20.0)))
                * Affine::rotate(rng.range(-3.0, 3.0))
                * Affine::scale_non_uniform(rng.range(0.2, 3.0), rng.range(0.2, 3.0))
        }
    }
}

fn random_path(rng: &mut Lcg) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((wild(rng), wild(rng)));
    for _ in 0..(rng.next_u32() % 5) {
        match rng.next_u32() % 3 {
            0 => path.line_to((wild(rng), wild(rng))),
            1 => path.quad_to((wild(rng), wild(rng)), (wild(rng), wild(rng))),
            _ => path.curve_to((wild(rng), wild(rng)), (wild(rng), wild(rng)), (wild(rng), wild(rng))),
        }
    }
    if rng.next_u32() % 2 == 0 {
        path.close_path();
    }
    path
}

fn random_paint(rng: &mut Lcg, image: ImageId) -> Paint {
    match rng.next_u32() % 4 {
        0 => Paint::Solid(Rgba8::new(rng.next_u32() as u8, 40, 200, rng.next_u32() as u8)),
        1 => Paint::Linear(random_gradient(rng)),
        2 => Paint::Radial(random_gradient(rng)),
        _ => Paint::Bitmap { image, transform: random_affine(rng), repeat: rng.next_u32() % 2 == 0, smoothing: rng.next_u32() % 2 == 0 },
    }
}

fn random_gradient(rng: &mut Lcg) -> anim_display::Gradient {
    let mut g = gradient(0.0, 30.0, RED, BLUE);
    g.transform = random_affine(rng);
    g.focal = wild(rng);
    g.spread = [anim_display::Spread::Pad, anim_display::Spread::Reflect, anim_display::Spread::Repeat][usize::try_from(rng.next_u32() % 3).unwrap()];
    g.interpolation = if rng.next_u32() % 2 == 0 { anim_display::Interpolation::LinearRgb } else { anim_display::Interpolation::Rgb };
    if rng.next_u32() % 5 == 0 {
        g.stops.clear();
    }
    g
}

fn random_filter(rng: &mut Lcg) -> Filter {
    match rng.next_u32() % 4 {
        0 => Filter::Blur { x: wild(rng).abs().min(40.0), y: wild(rng), quality: rng.next_u32() as u8 },
        1 => Filter::DropShadow {
            color: BLACK,
            angle_deg: wild(rng),
            distance: wild(rng),
            blur_x: wild(rng),
            blur_y: 3.0,
            strength: wild(rng),
            inner: rng.next_u32() % 2 == 0,
            knockout: rng.next_u32() % 2 == 0,
            hide_object: rng.next_u32() % 2 == 0,
        },
        2 => Filter::Glow { color: RED, blur_x: wild(rng), blur_y: wild(rng), strength: 2.0, inner: rng.next_u32() % 2 == 0, knockout: false },
        _ => Filter::ColorMatrix([wild(rng) as f32; 20]),
    }
}

fn random_node(rng: &mut Lcg, depth: usize, image: ImageId) -> Node {
    let kind = if depth >= 3 { rng.next_u32() % 3 } else { rng.next_u32() % 5 };
    match kind {
        0 => Node::Fill(Fill {
            transform: random_affine(rng),
            path: random_path(rng),
            rule: if rng.next_u32() % 2 == 0 { FillRule::EvenOdd } else { FillRule::NonZero },
            paint: random_paint(rng, image),
        }),
        1 => Node::Stroke(Stroke {
            transform: random_affine(rng),
            path: random_path(rng),
            style: StrokeStyle {
                width: wild(rng).abs().min(30.0),
                scale: [StrokeScale::Normal, StrokeScale::Horizontal, StrokeScale::Vertical, StrokeScale::None][usize::try_from(rng.next_u32() % 4).unwrap()],
                dashes: if rng.next_u32() % 3 == 0 { vec![wild(rng), wild(rng)] } else { Vec::new() },
                miter_limit: wild(rng),
                ..StrokeStyle::default()
            },
            paint: random_paint(rng, image),
        }),
        2 => Node::Bitmap(Bitmap {
            transform: random_affine(rng),
            image: if rng.next_u32() % 4 == 0 { ImageId(99) } else { image },
            smoothing: rng.next_u32() % 2 == 0,
        }),
        3 => {
            let blends = [
                BlendMode::Normal,
                BlendMode::Layer,
                BlendMode::Multiply,
                BlendMode::Add,
                BlendMode::Subtract,
                BlendMode::Invert,
                BlendMode::Alpha,
                BlendMode::Erase,
                BlendMode::Hue,
            ];
            Node::Group(Group {
                transform: random_affine(rng),
                color: if rng.next_u32() % 2 == 0 { ColorTransform::IDENTITY } else { ColorTransform { mul: [wild(rng) as f32; 4], add: [0.1; 4] } },
                blend: blends[usize::try_from(rng.next_u32()).unwrap() % blends.len()],
                mask: if rng.next_u32() % 3 == 0 { Some((0..2).map(|_| random_node(rng, depth + 1, image)).collect()) } else { None },
                filters: if rng.next_u32() % 3 == 0 { vec![random_filter(rng)] } else { Vec::new() },
                children: (0..(rng.next_u32() % 3)).map(|_| random_node(rng, depth + 1, image)).collect(),
                key: None,
            })
        }
        _ => {
            let points: Vec<Point> = (0..4).map(|_| Point::new(wild(rng), wild(rng))).collect();
            Node::Mesh(Mesh {
                transform: random_affine(rng),
                content: (0..2).map(|_| random_node(rng, depth + 1, image)).collect(),
                source: points.clone(),
                target: points.iter().map(|p| Point::new(p.x + wild(rng), p.y)).collect(),
                triangles: vec![[0, 1, 2], [1, 2, 3], [0, 2, 9]],
            })
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    #[expect(clippy::disallowed_types, reason = "proptest's prop_oneof! keeps its alternatives behind an Arc")]
    fn random_rects_with_any_matrix_never_panic(
        coeffs in proptest::array::uniform6(prop_oneof![Just(f64::NAN), Just(f64::INFINITY), Just(0.0), -50.0..50.0_f64, -1e9..1e9_f64]),
        x0 in -100.0..100.0_f64, y0 in -100.0..100.0_f64, w in 0.0..200.0_f64, h in 0.0..200.0_f64,
        alpha in 0.0..2.0_f32,
        blur in 0.0..30.0_f64,
    ) {
        let mut scene = Scene::new(20, 20);
        let group = Group {
            transform: Affine::new(coeffs),
            color: ColorTransform::alpha(alpha),
            filters: vec![Filter::Blur { x: blur, y: blur, quality: 2 }],
            children: vec![solid_rect(x0, y0, x0 + w, y0 + h, RED)],
            ..Group::default()
        };
        scene.add(Node::Group(group));
        let img = anim_render::render(&scene.list, &scene.images, &RenderOptions::new(20, 20)).unwrap();
        prop_assert_eq!(img.premultiplied_rgba.len(), 20 * 20 * 4);
        // Every pixel stays a valid premultiplied colour.
        prop_assert!(img.premultiplied_rgba.chunks_exact(4).all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
    }

    #[test]
    fn opaque_background_stays_opaque(
        x in -30.0..50.0_f64, y in -30.0..50.0_f64, r in 0.5..40.0_f64, mode in 0usize..8,
    ) {
        let modes = [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen, BlendMode::Add, BlendMode::Subtract, BlendMode::Difference, BlendMode::Invert, BlendMode::Layer];
        let mut scene = Scene::with_background(20, 20, Some(Rgba8::opaque(90, 120, 150)));
        scene.add(Node::Group(Group { blend: modes[mode], children: vec![solid_circle(x, y, r, Rgba8::new(200, 50, 10, 180))], ..Group::default() }));
        let img = scene.render();
        prop_assert!(img.premultiplied_rgba.chunks_exact(4).all(|p| p[3] == 255), "blending onto an opaque backdrop keeps it opaque");
    }
}
