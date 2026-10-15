//! A few rich scenes rendered to PNG for people to look at, with light assertions that they are not blank.

use anim_core::{Affine, BezPath, BlendMode, ColorTransform, Point, Rgba8, Vec2};
use anim_display::{
    Bitmap, Cap, Fill, FillRule, Filter, Gradient, GradientStop, Group, Image, ImageId, Interpolation, Join, Mesh, Node, Paint, Spread, StrokeStyle,
};

use crate::common::*;

fn panel(col: u32, row: u32, children: Vec<Node>) -> Node {
    let origin = Vec2::new(f64::from(col) * 200.0 + 10.0, f64::from(row) * 150.0 + 10.0);
    // A light card behind each panel so the transparent parts read as "empty".
    let card = solid_rect(0.0, 0.0, 180.0, 130.0, Rgba8::opaque(252, 252, 254));
    let mut all = vec![card];
    all.extend(children);
    Node::Group(Group { transform: Affine::translate(origin), children: all, ..Group::default() })
}

fn star(cx: f64, cy: f64, outer: f64, inner: f64) -> BezPath {
    let mut path = BezPath::new();
    for i in 0..10 {
        let angle = -std::f64::consts::FRAC_PI_2 + f64::from(i) * std::f64::consts::PI / 5.0;
        let radius = if i % 2 == 0 { outer } else { inner };
        let point = (cx + radius * angle.cos(), cy + radius * angle.sin());
        if i == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close_path();
    path
}

fn rounded_rect(x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> BezPath {
    use anim_core::kurbo::{RoundedRect, Shape};
    RoundedRect::new(x0, y0, x1, y1, r).to_path(0.05)
}

fn grad(stops: &[(f32, Rgba8)], transform: Affine, spread: Spread) -> Gradient {
    Gradient {
        transform,
        stops: stops.iter().map(|&(offset, color)| GradientStop { offset, color }).collect(),
        spread,
        interpolation: Interpolation::Rgb,
        focal: 0.0,
    }
}

/// Maps gradient space onto the segment `(x0, y0)` to `(x1, y1)`.
fn along(x0: f64, y0: f64, x1: f64, y1: f64) -> Affine {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let length = dx.hypot(dy);
    let angle = dy.atan2(dx);
    Affine::translate((f64::midpoint(x0, x1), f64::midpoint(y0, y1)))
        * Affine::rotate(angle)
        * Affine::scale_non_uniform(length / (anim_display::GRADIENT_EXTENT * 2.0), 1.0)
}

fn checker(store: &mut anim_display::ImageStore) -> ImageId {
    let mut pixels = Vec::new();
    for y in 0..8_u8 {
        for x in 0..8_u8 {
            let dark = (x / 2 + y / 2) % 2 == 0;
            pixels.extend_from_slice(&if dark { [40, 60, 140, 255] } else { [250, 200, 60, 255] });
        }
    }
    store.insert(Image::new(8, 8, pixels).unwrap())
}

fn stroke_style(width: f64, cap: Cap, join: Join) -> StrokeStyle {
    StrokeStyle { width, cap, join, miter_limit: 4.0, ..StrokeStyle::default() }
}

fn mesh_artwork() -> Vec<Node> {
    vec![
        solid_rect(0.0, 0.0, 40.0, 40.0, Rgba8::opaque(255, 232, 140)),
        solid_circle(15.0, 15.0, 10.0, Rgba8::opaque(220, 50, 50)),
        solid_rect(5.0, 30.0, 35.0, 36.0, Rgba8::opaque(40, 80, 200)),
        fill(star(28.0, 14.0, 9.0, 4.0), Paint::Solid(Rgba8::opaque(30, 140, 70))),
    ]
}

fn warped_mesh() -> Node {
    let cells = 8_u32;
    let (mut source, mut target, mut triangles) = (Vec::new(), Vec::new(), Vec::new());
    for row in 0..=cells {
        for col in 0..=cells {
            let p = Point::new(f64::from(col) * 5.0, f64::from(row) * 5.0);
            source.push(p);
            let bend = 8.0 * (p.x / 40.0 * std::f64::consts::PI).sin() * (1.0 - p.y / 60.0);
            target.push(Point::new(p.x * 2.4 + 10.0 + bend * 1.5, p.y * 2.4 + 8.0 + 6.0 * (p.x / 8.0).sin()));
        }
    }
    for row in 0..cells {
        for col in 0..cells {
            let a = row * (cells + 1) + col;
            triangles.push([a, a + 1, a + cells + 1]);
            triangles.push([a + 1, a + cells + 2, a + cells + 1]);
        }
    }
    Node::Mesh(Mesh { transform: Affine::IDENTITY, content: mesh_artwork(), source, target, triangles })
}

fn overview() -> Scene {
    let mut scene = Scene::with_background(800, 600, Some(Rgba8::opaque(228, 230, 238)));
    let tile = checker(&mut scene.images);
    let red = Rgba8::opaque(225, 50, 60);
    let blue = Rgba8::opaque(50, 90, 220);
    let yellow = Rgba8::opaque(250, 215, 60);
    let green = Rgba8::opaque(60, 190, 110);

    // Row 0: gradients and a bitmap fill.
    scene.add(panel(
        0,
        0,
        vec![fill(
            rounded_rect(10.0, 10.0, 170.0, 120.0, 16.0),
            Paint::Linear(grad(&[(0.0, red), (0.5, yellow), (1.0, blue)], along(10.0, 10.0, 170.0, 120.0), Spread::Pad)),
        )],
    ));
    let mut focal = grad(
        &[(0.0, Rgba8::WHITE), (0.4, yellow), (1.0, red)],
        Affine::translate((90.0, 65.0)) * Affine::scale(55.0 / anim_display::GRADIENT_EXTENT),
        Spread::Pad,
    );
    focal.focal = 0.55;
    scene.add(panel(1, 0, vec![fill(crate::common::circle_path(90.0, 65.0, 55.0), Paint::Radial(focal))]));
    let mut repeat = grad(&[(0.0, blue), (0.5, Rgba8::WHITE), (1.0, blue)], along(10.0, 0.0, 40.0, 0.0), Spread::Repeat);
    repeat.interpolation = Interpolation::LinearRgb;
    let reflect = grad(&[(0.0, red), (1.0, green)], along(10.0, 0.0, 50.0, 0.0), Spread::Reflect);
    scene.add(panel(
        2,
        0,
        vec![fill(rect_path(10.0, 10.0, 170.0, 58.0), Paint::Linear(repeat)), fill(rect_path(10.0, 72.0, 170.0, 120.0), Paint::Linear(reflect))],
    ));
    scene.add(panel(
        3,
        0,
        vec![fill(
            star(90.0, 68.0, 60.0, 26.0),
            Paint::Bitmap { image: tile, transform: Affine::translate((3.0, 3.0)) * Affine::scale(7.0), repeat: true, smoothing: false },
        )],
    ));

    // Row 1: strokes, even-odd, mask, mesh.
    let mut zigzag = BezPath::new();
    zigzag.move_to((14.0, 30.0));
    zigzag.line_to((50.0, 8.0));
    zigzag.line_to((86.0, 30.0));
    let line = |dy: f64| -> BezPath {
        let mut moved = zigzag.clone();
        moved.apply_affine(Affine::translate((0.0, dy)));
        moved
    };
    scene.add(panel(
        0,
        1,
        vec![
            stroke(line(0.0), stroke_style(10.0, Cap::None, Join::Miter), red),
            stroke(line(40.0), stroke_style(10.0, Cap::Round, Join::Round), blue),
            stroke(line(80.0), stroke_style(10.0, Cap::Square, Join::Bevel), green),
            stroke(
                line_path(100.0, 20.0, 170.0, 110.0),
                StrokeStyle { width: 8.0, cap: Cap::Round, dashes: vec![14.0, 10.0], ..StrokeStyle::default() },
                Rgba8::opaque(90, 60, 150),
            ),
        ],
    ));
    let mut ring = star(90.0, 65.0, 58.0, 30.0);
    ring.extend(crate::common::circle_path(90.0, 65.0, 20.0));
    let even_odd = Node::Fill(Fill { transform: Affine::IDENTITY, path: ring, rule: FillRule::EvenOdd, paint: Paint::Solid(Rgba8::opaque(60, 60, 80)) });
    let hair = |y: f64, width: f64| stroke(line_path(10.0, y, 60.0, y + 10.0), stroke_style(width, Cap::None, Join::Miter), red);
    scene.add(panel(1, 1, vec![even_odd, hair(20.0, 0.0), hair(40.0, 1.0), hair(60.0, 3.0)]));
    let masked = Group {
        mask: Some(vec![solid_circle(70.0, 65.0, 45.0, BLACK), fill(star(130.0, 65.0, 36.0, 16.0), Paint::Solid(Rgba8::new(0, 0, 0, 160)))]),
        children: vec![fill(
            rect_path(0.0, 0.0, 180.0, 130.0),
            Paint::Linear(grad(&[(0.0, red), (0.5, yellow), (1.0, blue)], along(0.0, 0.0, 180.0, 130.0), Spread::Pad)),
        )],
        ..Group::default()
    };
    scene.add(panel(2, 1, vec![Node::Group(masked)]));
    scene.add(panel(3, 1, vec![warped_mesh()]));

    // Row 2: blend modes, blur, shadows.
    let blend_swatch = |mode: BlendMode, x: f64, y: f64| -> Node {
        Node::Group(Group { blend: mode, children: vec![solid_circle(x, y, 22.0, Rgba8::opaque(250, 160, 40))], ..Group::default() })
    };
    let backdrop = fill(
        rect_path(0.0, 0.0, 180.0, 130.0),
        Paint::Linear(grad(&[(0.0, Rgba8::opaque(30, 60, 160)), (1.0, Rgba8::opaque(60, 200, 160))], along(0.0, 0.0, 180.0, 0.0), Spread::Pad)),
    );
    scene.add(panel(
        0,
        2,
        vec![
            backdrop,
            blend_swatch(BlendMode::Normal, 30.0, 35.0),
            blend_swatch(BlendMode::Multiply, 90.0, 35.0),
            blend_swatch(BlendMode::Screen, 150.0, 35.0),
            blend_swatch(BlendMode::Add, 30.0, 95.0),
            blend_swatch(BlendMode::Difference, 90.0, 95.0),
            blend_swatch(BlendMode::Overlay, 150.0, 95.0),
        ],
    ));
    let blurred = |x: f64, amount: f64, quality: u8| -> Node {
        Node::Group(Group {
            filters: vec![Filter::Blur { x: amount, y: amount, quality }],
            children: vec![solid_circle(x, 65.0, 22.0, red), solid_rect(x - 12.0, 40.0, x + 12.0, 52.0, Rgba8::BLACK)],
            ..Group::default()
        })
    };
    scene.add(panel(1, 2, vec![blurred(32.0, 0.0, 1), blurred(90.0, 8.0, 3), blurred(148.0, 16.0, 3)]));
    let shape = |filter: Filter, x: f64| -> Node {
        Node::Group(Group { filters: vec![filter], children: vec![solid_circle(x, 55.0, 20.0, blue)], ..Group::default() })
    };
    scene.add(panel(
        2,
        2,
        vec![
            shape(
                Filter::DropShadow {
                    color: Rgba8::new(0, 0, 0, 200),
                    angle_deg: 45.0,
                    distance: 10.0,
                    blur_x: 8.0,
                    blur_y: 8.0,
                    strength: 1.0,
                    inner: false,
                    knockout: false,
                    hide_object: false,
                },
                38.0,
            ),
            shape(Filter::Glow { color: Rgba8::opaque(255, 190, 0), blur_x: 14.0, blur_y: 14.0, strength: 2.5, inner: false, knockout: false }, 100.0),
            shape(
                Filter::DropShadow {
                    color: Rgba8::new(0, 0, 0, 220),
                    angle_deg: 225.0,
                    distance: 6.0,
                    blur_x: 6.0,
                    blur_y: 6.0,
                    strength: 1.5,
                    inner: true,
                    knockout: false,
                    hide_object: false,
                },
                150.0,
            ),
            Node::Group(Group {
                filters: vec![Filter::DropShadow {
                    color: Rgba8::new(80, 0, 120, 255),
                    angle_deg: 90.0,
                    distance: 8.0,
                    blur_x: 0.0,
                    blur_y: 0.0,
                    strength: 1.0,
                    inner: false,
                    knockout: true,
                    hide_object: false,
                }],
                children: vec![fill(star(90.0, 100.0, 22.0, 10.0), Paint::Solid(red))],
                ..Group::default()
            }),
        ],
    ));
    let tinted = |x: f64, ct: ColorTransform| -> Node {
        with_color(vec![solid_rect(x, 20.0, x + 36.0, 110.0, Rgba8::opaque(230, 120, 40)), solid_circle(x + 18.0, 40.0, 12.0, Rgba8::opaque(40, 90, 200))], ct)
    };
    scene.add(panel(
        3,
        2,
        vec![
            tinted(6.0, ColorTransform::tint(Rgba8::opaque(0, 160, 80), 0.6)),
            tinted(48.0, ColorTransform::brightness(0.5)),
            tinted(90.0, ColorTransform::alpha(0.4)),
            tinted(132.0, ColorTransform { mul: [0.2, 1.0, 1.4, 1.0], add: [0.3, 0.0, 0.0, 0.0] }),
        ],
    ));

    // Row 3: alpha/erase, smoothing, colour matrix, nested rotations.
    let layer = Group {
        blend: BlendMode::Layer,
        children: vec![
            fill(rounded_rect(15.0, 15.0, 165.0, 115.0, 10.0), Paint::Linear(grad(&[(0.0, red), (1.0, yellow)], along(15.0, 15.0, 165.0, 115.0), Spread::Pad))),
            Node::Group(Group {
                blend: BlendMode::Erase,
                children: vec![solid_circle(60.0, 65.0, 28.0, BLACK), fill(star(120.0, 65.0, 30.0, 13.0), Paint::Solid(BLACK))],
                ..Group::default()
            }),
        ],
        ..Group::default()
    };
    scene.add(panel(0, 3, vec![Node::Group(layer)]));
    let bitmap = |x: f64, smoothing: bool| Node::Bitmap(Bitmap { transform: Affine::translate((x, 30.0)) * Affine::scale(9.0), image: tile, smoothing });
    scene.add(panel(1, 3, vec![bitmap(6.0, false), bitmap(98.0, true)]));
    #[rustfmt::skip]
    let sepia = [
        0.393, 0.769, 0.189, 0.0, 0.0,
        0.349, 0.686, 0.168, 0.0, 0.0,
        0.272, 0.534, 0.131, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    let colourful = vec![
        fill(
            rect_path(10.0, 10.0, 170.0, 120.0),
            Paint::Linear(grad(&[(0.0, red), (0.33, yellow), (0.66, green), (1.0, blue)], along(10.0, 0.0, 170.0, 0.0), Spread::Pad)),
        ),
        solid_circle(90.0, 65.0, 30.0, Rgba8::opaque(240, 240, 250)),
    ];
    scene.add(panel(2, 3, vec![Node::Group(Group { filters: vec![Filter::ColorMatrix(sepia)], children: colourful, ..Group::default() })]));
    let fan: Vec<Node> = (0..9)
        .map(|i| {
            let angle = f64::from(i) * 0.22 - 0.9;
            let shade = u8::try_from(i * 24).unwrap();
            Node::Group(Group {
                transform: Affine::translate((90.0, 115.0)) * Affine::rotate(angle),
                color: ColorTransform::alpha(0.75),
                children: vec![solid_rect(-8.0, -100.0, 8.0, 0.0, Rgba8::opaque(250 - shade, 90 + shade / 2, 60 + shade / 2))],
                ..Group::default()
            })
        })
        .collect();
    scene.add(panel(3, 3, fan));
    scene
}

#[test]
fn overview_scene_renders_and_is_saved() {
    let scene = overview();
    let (img, stats) = scene.render_stats();
    save("showcase_overview", &img);
    assert!(stats.is_clean(), "{stats:?}");
    assert!(stats.isolated_groups >= 20, "{stats:?}");
    assert!(count(&img, |px| px == [228, 230, 238, 255]) > 1000, "the page background shows between panels");
    assert!(count(&img, |px| px[3] == 255 && px != [252, 252, 254, 255]) > 50_000, "panels are full of content");
    assert_eq!(scene.render(), img, "deterministic");
}

#[test]
fn overview_at_other_zooms() {
    let scene = overview();
    for (name, zoom) in [("showcase_overview_half", 0.5), ("showcase_overview_double_crop", 2.0)] {
        let options = if zoom > 1.0 {
            // Look at the top-left quarter in detail.
            anim_render::RenderOptions::new(800, 600).with_view(Affine::scale(zoom))
        } else {
            anim_render::RenderOptions::scaled_stage(scene.list.size, zoom)
        };
        let img = scene.render_with(&options);
        save(name, &img);
        assert_eq!(count(&img, |px| px[3] == 255), usize::try_from(img.width * img.height).unwrap());
    }
}

#[test]
fn transparent_render_keeps_alpha_for_compositing() {
    let scene = overview();
    let img = scene.render_transparent();
    save("showcase_overview_transparent", &img);
    assert!(count(&img, |px| px[3] == 0) > 1000);
    assert!(img.premultiplied_rgba.chunks_exact(4).all(|px| px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]));
}
