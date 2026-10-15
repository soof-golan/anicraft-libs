//! Textured meshes: identity warps equal plain renders, affine warps equal transformed renders, deformation
//! moves the texture, and neighbouring triangles leave no seams.

use anim_core::{Affine, Point, Rgba8};
use anim_display::{Group, Mesh, Node, Paint};

use crate::common::*;

/// The artwork every test deforms: a yellow card with a red disc, a blue bar and a gradient strip.
fn artwork() -> Vec<Node> {
    vec![
        solid_rect(0.0, 0.0, 32.0, 32.0, Rgba8::opaque(255, 230, 120)),
        solid_circle(12.0, 12.0, 7.5, RED),
        solid_rect(4.0, 24.0, 28.0, 28.0, BLUE),
        fill(rect_path(20.0, 4.0, 30.0, 18.0), Paint::Linear(gradient(20.0, 30.0, GREEN, BLACK))),
    ]
}

/// A `cells` x `cells` grid of triangles over `(x0, y0)..(x1, y1)`, with every vertex moved by `warp`.
fn grid_mesh(content: Vec<Node>, area: (f64, f64, f64, f64), cells: u32, transform: Affine, warp: impl Fn(Point) -> Point) -> Node {
    let (x0, y0, x1, y1) = area;
    let (mut source, mut target, mut triangles) = (Vec::new(), Vec::new(), Vec::new());
    for row in 0..=cells {
        for col in 0..=cells {
            let point = Point::new(x0 + (x1 - x0) * f64::from(col) / f64::from(cells), y0 + (y1 - y0) * f64::from(row) / f64::from(cells));
            source.push(point);
            target.push(warp(point));
        }
    }
    let stride = cells + 1;
    for row in 0..cells {
        for col in 0..cells {
            let a = row * stride + col;
            let (b, c, d) = (a + 1, a + stride, a + stride + 1);
            triangles.push([a, b, c]);
            triangles.push([b, d, c]);
        }
    }
    Node::Mesh(Mesh { transform, content, source, target, triangles })
}

fn plain(width: u32, height: u32, nodes: Vec<Node>) -> anim_display::Image {
    let mut scene = Scene::new(width, height);
    for node in nodes {
        scene.add(node);
    }
    scene.render()
}

fn on_stage(width: u32, height: u32, mesh: Node) -> (anim_display::Image, anim_render::RenderStats) {
    let mut scene = Scene::new(width, height);
    scene.add(mesh);
    scene.render_stats()
}

fn mean_diff(a: &anim_display::Image, b: &anim_display::Image) -> f64 {
    let total: u64 = a.premultiplied_rgba.iter().zip(&b.premultiplied_rgba).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    total as f64 / a.premultiplied_rgba.len() as f64
}

#[test]
fn identity_warp_equals_the_plain_render() {
    let reference = plain(32, 32, artwork());
    for cells in [1, 2, 4, 7] {
        let mesh = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), cells, Affine::IDENTITY, |p| p);
        let (img, stats) = on_stage(32, 32, mesh);
        assert_eq!(max_diff(&img, &reference), 0, "{cells}x{cells} grid must reproduce the plain render exactly");
        assert!(stats.is_clean());
    }
    save("mesh_identity", &reference);
}

#[test]
fn identity_warp_with_a_node_transform_equals_the_plain_render_under_it() {
    let transform = Affine::translate((6.0, 4.0));
    let mut scene = Scene::new(48, 40);
    scene.add(Node::Group(Group { transform, children: artwork(), ..Group::default() }));
    let reference = scene.render();
    let mesh = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 3, transform, |p| p);
    let (img, _) = on_stage(48, 40, mesh);
    assert_eq!(max_diff(&img, &reference), 0);
}

#[test]
fn translating_the_targets_translates_the_texture() {
    let mut reference = Scene::new(48, 40);
    reference.add(Node::Group(Group { transform: Affine::translate((9.0, 5.0)), children: artwork(), ..Group::default() }));
    let mesh = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 4, Affine::IDENTITY, |p| Point::new(p.x + 9.0, p.y + 5.0));
    let (img, _) = on_stage(48, 40, mesh);
    assert_eq!(max_diff(&img, &reference.render()), 0);
}

#[test]
fn uniform_scaling_of_the_targets_equals_a_scaled_render() {
    let mut reference = Scene::new(64, 64);
    reference.add(Node::Group(Group { transform: Affine::scale(2.0), children: artwork(), ..Group::default() }));
    let mesh = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 4, Affine::IDENTITY, |p| Point::new(p.x * 2.0, p.y * 2.0));
    let (img, _) = on_stage(64, 64, mesh);
    assert_eq!(max_diff(&img, &reference.render()), 0, "the texture is rendered at the target resolution, not magnified");
    save("mesh_scaled", &img);
}

#[test]
fn rotation_of_the_targets_matches_a_rotated_render() {
    let rotation = Affine::translate((32.0, 8.0)) * Affine::rotate(0.5);
    let mut reference = Scene::new(64, 64);
    reference.add(Node::Group(Group { transform: rotation, children: artwork(), ..Group::default() }));
    let mesh = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 4, Affine::IDENTITY, |p| rotation * p);
    let (img, _) = on_stage(64, 64, mesh);
    let reference = reference.render();
    assert!(mean_diff(&img, &reference) < 1.0, "mean difference {}", mean_diff(&img, &reference));
    save("mesh_rotated", &img);
}

#[test]
fn deformation_moves_the_texture_with_the_vertices() {
    // Left half red, right half blue; pull the right-hand vertices 12 px to the right.
    let content = vec![solid_rect(0.0, 0.0, 16.0, 16.0, RED), solid_rect(16.0, 0.0, 32.0, 16.0, BLUE)];
    let mesh = grid_mesh(content, (0.0, 0.0, 32.0, 16.0), 4, Affine::IDENTITY, |p| Point::new(if p.x > 31.0 { p.x + 12.0 } else { p.x }, p.y));
    let (img, _) = on_stage(48, 16, mesh);
    assert_px(&img, 4, 8, [255, 0, 0, 255], 0);
    assert_px(&img, 40, 8, [0, 0, 255, 255], 0);
    assert_px(&img, 43, 8, [0, 0, 255, 255], 0);
    assert_px(&img, 44, 8, [255, 255, 255, 255], 0);
    // The blue half now spans x 16..44 instead of 16..32.
    let blue = (0..48).filter(|&x| pixel(&img, x, 8) == [0, 0, 255, 255]).count();
    assert!(blue >= 26, "the stretched half is wider: {blue}");
    save("mesh_deformed", &img);
}

#[test]
fn smooth_non_affine_warp_leaves_no_seams() {
    let content = vec![solid_rect(0.0, 0.0, 32.0, 32.0, Rgba8::opaque(20, 160, 60))];
    let warp = |p: Point| Point::new(p.x + 2.5 * (p.y / 4.0).sin() + 4.0, p.y + 2.5 * (p.x / 5.0).cos() + 4.0);
    let mesh = grid_mesh(content, (0.0, 0.0, 32.0, 32.0), 8, Affine::IDENTITY, warp);
    let (img, _) = on_stage(48, 48, mesh);
    let mut holes = 0;
    for y in 10..30 {
        for x in 10..30 {
            if pixel(&img, x, y) != [20, 160, 60, 255] {
                holes += 1;
            }
        }
    }
    assert_eq!(holes, 0, "every interior pixel is covered by exactly the texture colour");
    save("mesh_wavy", &img);
}

#[test]
fn warped_artwork_stays_smooth_at_the_silhouette() {
    let warp = |p: Point| Point::new(p.x * 1.2 + 3.0 + 2.0 * (p.y / 6.0).sin(), p.y * 1.1 + 3.0);
    let mesh = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 6, Affine::IDENTITY, warp);
    let (img, stats) = on_stage(56, 48, mesh);
    assert!(stats.is_clean());
    let soft = count(&img, |px| px != [255, 255, 255, 255] && px[3] == 255 && px[2] > 150 && px[2] < 250 && px[0] > 200);
    assert!(soft > 0, "the mesh outline is anti-aliased");
    save("mesh_warped_artwork", &img);
}

#[test]
fn invalid_triangles_are_skipped() {
    let Node::Mesh(mut mesh) = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 2, Affine::IDENTITY, |p| p) else { unreachable!() };
    mesh.triangles = vec![[0, 1, 99], [0, 0, 0], [u32::MAX, 1, 2]];
    let (img, stats) = on_stage(32, 32, Node::Mesh(mesh.clone()));
    assert_eq!(count(&img, |px| px != [255, 255, 255, 255]), 0);
    assert!(stats.invalid >= 1);
    mesh.triangles = vec![[0, 1, 99], [0, 1, 3], [1, 4, 3]];
    mesh.target[0] = Point::new(f64::NAN, 0.0);
    let (img, _) = on_stage(32, 32, Node::Mesh(mesh));
    assert!(count(&img, |px| px != [255, 255, 255, 255]) > 0, "valid triangles still draw next to broken ones");
}

#[test]
fn mismatched_source_and_target_lengths_do_not_panic() {
    let Node::Mesh(mut mesh) = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 2, Affine::IDENTITY, |p| p) else { unreachable!() };
    mesh.target.truncate(4);
    let (img, _) = on_stage(32, 32, Node::Mesh(mesh));
    assert!(count(&img, |px| px != [255, 255, 255, 255]) > 0, "triangles whose vertices exist still draw");
}

#[test]
fn meshes_compose_with_group_effects() {
    let mesh = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 3, Affine::IDENTITY, |p| p);
    let mut scene = Scene::new(32, 32);
    scene.add(with_color(vec![mesh], anim_core::ColorTransform::alpha(0.5)));
    let img = scene.render();
    assert_px(&img, 12, 12, [255, 128, 128, 255], 2);
    let mut reference = Scene::new(32, 32);
    reference.add(with_color(artwork(), anim_core::ColorTransform::alpha(0.5)));
    assert_eq!(max_diff(&img, &reference.render()), 0);
}

#[test]
fn partially_visible_mesh_matches_the_visible_part_of_the_plain_render() {
    let shift = Affine::translate((-14.0, -9.0));
    let mut reference = Scene::new(24, 24);
    reference.add(Node::Group(Group { transform: shift, children: artwork(), ..Group::default() }));
    let mesh = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 4, shift, |p| p);
    let (img, _) = on_stage(24, 24, mesh);
    let reference = reference.render();
    // Identical except where tiny-skia flattens a curve that crosses the view edge differently from the same
    // curve drawn whole into the texture (one anti-aliased pixel of the disc, off by a few percent).
    let differing = img.premultiplied_rgba.chunks_exact(4).zip(reference.premultiplied_rgba.chunks_exact(4)).filter(|(a, b)| a != b).count();
    assert!(differing <= 2, "{differing} pixels differ");
    assert!(max_diff(&img, &reference) <= 24);
}

/// A big twisted mesh over a checkerboard, for people to look at: seams would show as bright lines.
#[test]
fn large_twist_has_no_seams_and_is_saved() {
    let mut scene = Scene::with_background(480, 480, Some(Rgba8::opaque(30, 34, 48)));
    let tile = {
        let mut pixels = Vec::new();
        for y in 0..16_u8 {
            for x in 0..16_u8 {
                pixels.extend_from_slice(&if (x / 2 + y / 2) % 2 == 0 { [250, 250, 250, 255] } else { [20, 90, 200, 255] });
            }
        }
        scene.images.insert(anim_display::Image::new(16, 16, pixels).unwrap())
    };
    let content = vec![
        fill(rect_path(0.0, 0.0, 160.0, 160.0), Paint::Bitmap { image: tile, transform: Affine::scale(10.0), repeat: true, smoothing: true }),
        solid_circle(80.0, 80.0, 30.0, Rgba8::new(255, 60, 40, 200)),
    ];
    let twist = |p: Point| {
        let (dx, dy) = (p.x - 80.0, p.y - 80.0);
        let radius = dx.hypot(dy);
        let angle = 0.9 * (1.0 - (radius / 113.0).min(1.0));
        let (sin, cos) = angle.sin_cos();
        Point::new(240.0 + 1.35 * (dx * cos - dy * sin), 240.0 + 1.35 * (dx * sin + dy * cos))
    };
    scene.add(grid_mesh(content, (0.0, 0.0, 160.0, 160.0), 16, Affine::IDENTITY, twist));
    let (img, stats) = scene.render_stats();
    assert!(stats.is_clean());
    // The centre region of the mesh is inside every triangle's coverage: no background (30,34,48) may show.
    let background = |px: [u8; 4]| px == [30, 34, 48, 255];
    let mut gaps = 0;
    for y in 150..330_u32 {
        for x in 150..330_u32 {
            gaps += usize::from(background(pixel(&img, x, y)));
        }
    }
    assert_eq!(gaps, 0);
    save("mesh_twist_large", &img);
}

#[test]
fn meshes_far_away_or_enormous_are_bounded() {
    let far = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 2, Affine::translate((5000.0, 5000.0)), |p| p);
    let (img, stats) = on_stage(32, 32, far);
    assert_eq!(count(&img, |px| px != [255, 255, 255, 255]), 0);
    assert_eq!(stats.culled, 1);
    let huge = grid_mesh(artwork(), (0.0, 0.0, 32.0, 32.0), 2, Affine::scale(100_000.0), |p| p);
    let (img, stats) = on_stage(32, 32, huge);
    assert_eq!((img.width, img.height), (32, 32));
    assert!(stats.peak_layer_bytes < 200_000_000, "{}", stats.peak_layer_bytes);
}
