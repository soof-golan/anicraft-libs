//! Fixtures `fill_rules`, `fill_sides` and `degenerate_edges`: fill rules, edge sides, holes and degenerate geometry.

use std::fmt::Write as _;

use anim_core::{Affine, BezPath, Rgba8};
use anim_display::{Fill, FillRule, Node, Paint};
use anim_shape::{Edge, EdgeShape, FillStyle, NoImages, StrokeStyle, parse_edges};

use crate::common::{RED, Scene, count, pixel};
use crate::kit::{assert_color, assert_white, finish, ink, ink_sum, save_crop};

fn filled(path: BezPath, rule: FillRule) -> Node {
    Node::Fill(Fill { transform: Affine::IDENTITY, path, rule, paint: Paint::Solid(RED) })
}

fn polygon(points: &[(f64, f64)]) -> BezPath {
    let mut path = BezPath::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(*p);
        } else {
            path.line_to(*p);
        }
    }
    path.close_path();
    path
}

fn star(cx: f64, cy: f64, radius: f64) -> BezPath {
    let points: Vec<(f64, f64)> = (0..5)
        .map(|k| {
            let angle = (-90.0 + 144.0 * f64::from(k)).to_radians();
            (radius.mul_add(angle.cos(), cx), radius.mul_add(angle.sin(), cy))
        })
        .collect();
    polygon(&points)
}

fn two(a: &[(f64, f64)], b: &[(f64, f64)]) -> BezPath {
    let mut path = polygon(a);
    for el in polygon(b).elements() {
        path.push(*el);
    }
    path
}

fn sq(x0: f64, y0: f64, x1: f64, y1: f64, clockwise: bool) -> Vec<(f64, f64)> {
    if clockwise { vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)] } else { vec![(x0, y0), (x0, y1), (x1, y1), (x1, y0)] }
}

#[test]
fn fill_rules() {
    let mut scene = Scene::new(240, 160);
    scene.add(filled(star(40.0, 40.0, 30.0), FillRule::NonZero));
    scene.add(filled(star(120.0, 40.0, 30.0), FillRule::EvenOdd));
    scene.add(filled(two(&sq(170.0, 10.0, 210.0, 50.0, true), &sq(190.0, 30.0, 230.0, 70.0, false)), FillRule::NonZero));
    scene.add(filled(two(&sq(10.0, 90.0, 50.0, 130.0, true), &sq(30.0, 110.0, 70.0, 150.0, true)), FillRule::NonZero));
    scene.add(filled(two(&sq(90.0, 90.0, 130.0, 130.0, true), &sq(110.0, 110.0, 150.0, 150.0, true)), FillRule::EvenOdd));
    scene.add(filled(two(&sq(170.0, 90.0, 230.0, 150.0, true), &sq(185.0, 105.0, 215.0, 135.0, false)), FillRule::NonZero));
    let image = scene.render();
    finish("fill_rules", &image);
    save_crop("fill_rules-star-nonzero-8x", &image, 0, 0, 80, 80, 8);
    save_crop("fill_rules-star-evenodd-8x", &image, 80, 0, 80, 80, 8);

    // P1: non-zero star.
    assert_color("A centre", &image, 40, 40, [255, 0, 0]);
    assert_white("A notch", &image, 40, 58);
    // P2: even-odd star.
    assert_white("B centre", &image, 120, 40);
    assert_white("B notch", &image, 120, 58);
    for k in 0..5 {
        let angle = (-90.0 + 72.0 * f64::from(k)).to_radians();
        for (cx, cy, name) in [(40.0, 40.0, "A"), (120.0, 40.0, "B")] {
            let (x, y) = (16.0_f64.mul_add(angle.cos(), cx), 16.0_f64.mul_add(angle.sin(), cy));
            assert_color(&format!("{name} arm {k}"), &image, x.floor() as u32, y.floor() as u32, [255, 0, 0]);
        }
    }
    // P3: opposite directions.
    assert_color("C square 1 only", &image, 180, 20, [255, 0, 0]);
    assert_color("C square 2 only", &image, 220, 60, [255, 0, 0]);
    assert_white("C overlap", &image, 200, 40);
    assert_white("C outside 1", &image, 220, 20);
    assert_white("C outside 2", &image, 180, 60);
    // P4: same direction, non-zero.
    for (x, y) in [(40, 120), (20, 100), (60, 140)] {
        assert_color("D", &image, x, y, [255, 0, 0]);
    }
    assert_white("D outside 1", &image, 60, 100);
    assert_white("D outside 2", &image, 20, 140);
    // P5: same direction, even-odd.
    assert_white("E overlap", &image, 120, 120);
    assert_color("E sq1", &image, 100, 100, [255, 0, 0]);
    assert_color("E sq2", &image, 140, 140, [255, 0, 0]);
    assert_white("E outside 1", &image, 140, 100);
    assert_white("E outside 2", &image, 100, 140);
    // P6: hole by direction.
    assert_white("F hole", &image, 200, 120);
    assert_color("F ring", &image, 175, 95, [255, 0, 0]);
    // P7: anti-aliased outline.
    assert!(count(&image, |p| p[3] == 255 && p[1] > 20 && p[1] < 235) > 20, "star outline should have partial coverage pixels");
}

// ---- fill_sides ----

fn tw(v: f64) -> String {
    format!("{}", v * 20.0)
}

/// Edge text for a polyline given in pixels.
fn path_text(points: &[(f64, f64)]) -> String {
    let mut text = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        text.push_str(if i == 0 { "!" } else { "|" });
        let _ = write!(text, "{} {}", tw(*x), tw(*y));
    }
    text
}

fn edge(f0: u32, f1: u32, stroke: u32, points: &[(f64, f64)]) -> Edge {
    Edge::new(f0, f1, stroke, parse_edges(&path_text(points)).unwrap())
}

fn solid_fills() -> Vec<FillStyle> {
    vec![FillStyle::Solid(RED), FillStyle::Solid(Rgba8::opaque(0, 160, 0)), FillStyle::Solid(Rgba8::opaque(0, 0, 255))]
}

fn closed(x0: f64, y0: f64, x1: f64, y1: f64, clockwise: bool) -> Vec<(f64, f64)> {
    let mut p = sq(x0, y0, x1, y1, clockwise);
    p.push(p[0]);
    p
}

fn fill_sides_image() -> anim_display::Image {
    let mut edges = vec![
        // Cell 1.
        edge(0, 1, 0, &closed(5.0, 5.0, 95.0, 95.0, true)),
        edge(0, 1, 0, &closed(30.0, 30.0, 70.0, 70.0, false)),
        // Cell 2.
        edge(1, 0, 0, &closed(105.0, 5.0, 195.0, 95.0, false)),
        edge(1, 0, 0, &closed(130.0, 30.0, 170.0, 70.0, true)),
        // Cell 3.
        edge(0, 3, 0, &closed(205.0, 5.0, 295.0, 95.0, true)),
        edge(3, 1, 0, &closed(225.0, 25.0, 275.0, 75.0, true)),
        edge(1, 2, 0, &closed(240.0, 40.0, 260.0, 60.0, true)),
        edge(1, 1, 0, &[(230.0, 30.0), (270.0, 30.0)]),
    ];
    // Cell 4: scrambled pieces, east side travelled northwards with the fill on its left, 0.4 twip gaps.
    let (x0, y0, x1, y1) = (5.0, 105.0, 95.0, 195.0);
    let gap = 0.4 / 20.0;
    edges.push(edge(1, 0, 0, &[(x1, y1), (x1, y0 + gap)]));
    edges.push(edge(0, 1, 0, &[(x0, y1 + gap), (x0, y0)]));
    edges.push(edge(0, 1, 0, &[(x1 - gap, y1), (x0, y1)]));
    edges.push(edge(0, 1, 0, &[(x0, y0), (x1, y0)]));
    // Cell 5: only three sides.
    edges.push(edge(0, 2, 0, &[(105.0, 105.0), (195.0, 105.0), (195.0, 195.0), (105.0, 195.0)]));
    // Cell 6: out-of-range style, then two squares sharing x = 250.5.
    edges.push(edge(0, 9, 0, &closed(205.0, 105.0, 295.0, 155.0, true)));
    edges.push(edge(2, 0, 0, &[(250.5, 160.0), (205.0, 160.0), (205.0, 195.0), (250.5, 195.0)]));
    edges.push(edge(3, 0, 0, &[(250.5, 195.0), (295.0, 195.0), (295.0, 160.0), (250.5, 160.0)]));
    edges.push(edge(3, 2, 0, &[(250.5, 160.0), (250.5, 195.0)]));
    let shape = EdgeShape { fills: solid_fills(), strokes: Vec::new(), edges };

    let mut scene = Scene::new(300, 200);
    for node in shape.to_display(&NoImages) {
        scene.add(node);
    }
    scene.render()
}

#[test]
fn fill_sides() {
    let image = fill_sides_image();
    finish("fill_sides", &image);
    save_crop("fill_sides-seam-16x", &image, 240, 170, 20, 12, 16);

    // P1, P2.
    assert_color("c1 ring left", &image, 15, 50, [255, 0, 0]);
    assert_color("c1 ring top", &image, 50, 15, [255, 0, 0]);
    assert_white("c1 hole", &image, 50, 50);
    assert_color("c2 ring left", &image, 115, 50, [255, 0, 0]);
    assert_color("c2 ring top", &image, 150, 15, [255, 0, 0]);
    assert_white("c2 hole", &image, 150, 50);
    for y in 0..100 {
        for x in 0..100 {
            let (a, b) = (pixel(&image, x, y), pixel(&image, x + 100, y));
            assert!(a.iter().zip(b).all(|(p, q)| p.abs_diff(q) <= 1), "cells 1 and 2 differ at ({x}, {y}): {a:?} vs {b:?}");
        }
    }
    // P3.
    assert_color("blue ring", &image, 210, 50, [0, 0, 255]);
    assert_color("red ring", &image, 230, 50, [255, 0, 0]);
    assert_color("green core", &image, 250, 50, [0, 160, 0]);
    assert_color("blue top", &image, 250, 15, [0, 0, 255]);
    for y in [29, 30, 31] {
        assert_color("interior edge leaves no seam", &image, 250, y, [255, 0, 0]);
    }
    // P4, P5.
    assert_color("c4 centre", &image, 50, 150, [255, 0, 0]);
    assert_color("c4 corner", &image, 6, 106, [255, 0, 0]);
    assert_color("c4 corner 2", &image, 93, 193, [255, 0, 0]);
    assert_color("c5 centre", &image, 150, 150, [0, 160, 0]);
    assert_color("c5 corner", &image, 106, 106, [0, 160, 0]);
    assert_color("c5 corner 2", &image, 193, 193, [0, 160, 0]);
    // P6.
    assert_white("fill 9 draws nothing", &image, 225, 130);
    assert_color("left of shared edge", &image, 225, 180, [0, 160, 0]);
    assert_color("right of shared edge", &image, 275, 180, [0, 0, 255]);
}

/// P7: Flash shows no background through the seam between two abutting fills. Each fill is anti-aliased on its own
/// here, so a pixel that is half green and half blue ends up with a quarter of the white background in it.
#[test]
#[ignore = "known limitation: fills are anti-aliased one after the other (conflation), see fill_sides.expect.md P7"]
fn fill_sides_seam() {
    let image = fill_sides_image();
    let seam = pixel(&image, 250, 180);
    assert!(seam[0] < 20, "seam pixel {seam:?} is lightened by the white background");
}

#[test]
fn degenerate_edges() {
    let caps = [anim_display::Cap::Round, anim_display::Cap::Square, anim_display::Cap::None];
    let mut strokes: Vec<StrokeStyle> = caps
        .iter()
        .map(|cap| StrokeStyle::Solid(anim_shape::SolidStroke { weight: 20.0, caps: *cap, fill: FillStyle::Solid(Rgba8::BLACK), ..Default::default() }))
        .collect();
    strokes.push(StrokeStyle::Solid(anim_shape::SolidStroke { weight: 4.0, fill: FillStyle::Solid(Rgba8::BLACK), ..Default::default() }));
    strokes.push(StrokeStyle::plain(2.0, Rgba8::BLACK));
    let mut edges = vec![
        Edge::new(0, 0, 1, parse_edges("!600 1000|600 1000").unwrap()),
        Edge::new(0, 0, 2, parse_edges("!1800 1000|1800 1000").unwrap()),
        Edge::new(0, 0, 3, parse_edges("!3000 1000|3000 1000").unwrap()),
    ];
    // G1: the same edge twice.
    let g1 = "!200 1600|1000 1600|1000 2400|200 2400|200 1600";
    edges.push(Edge::new(0, 1, 0, parse_edges(g1).unwrap()));
    edges.push(Edge::new(0, 1, 0, parse_edges(g1).unwrap()));
    // G2: fill-only edge and stroke-only edge with the same segments.
    let g2 = "!1400 1600|2200 1600|2200 2400|1400 2400|1400 1600";
    edges.push(Edge::new(0, 2, 0, parse_edges(g2).unwrap()));
    edges.push(Edge::new(0, 0, 4, parse_edges(g2).unwrap()));
    // G3: zero-length segments at every corner.
    edges.push(Edge::new(0, 3, 0, parse_edges("!2600 1600|3400 1600|3400 1600|3400 2400|3400 2400|2600 2400|2600 2400|2600 1600").unwrap()));
    // G4: zero area; lone moveto with a stroke.
    edges.push(Edge::new(0, 1, 0, parse_edges("!3600 1600|3800 1600|3700 1600").unwrap()));
    edges.push(Edge::new(0, 0, 5, vec![]));
    edges.push(Edge::new(0, 0, 5, parse_edges("!3700 2000").unwrap()));
    let shape = EdgeShape { fills: solid_fills(), strokes, edges };
    let mut scene = Scene::new(200, 140);
    for node in shape.to_display(&NoImages) {
        scene.add(node);
    }
    let image = scene.render();
    finish("degenerate_edges", &image);
    save_crop("degenerate_edges-dots-8x", &image, 0, 30, 180, 40, 6);
    save_crop("degenerate_edges-g2-12x", &image, 60, 70, 30, 30, 12);

    // P1: round dot.
    assert!(ink(&image, 30, 50) > 0.95 && ink(&image, 37, 50) > 0.95, "round dot centre");
    assert!(ink(&image, 22, 42) < 0.05 && ink(&image, 42, 50) < 0.05, "round dot extent");
    // P2: square dot.
    for (x, y) in [(90, 50), (82, 42), (97, 57)] {
        assert!(ink(&image, x, y) > 0.95, "square dot ({x}, {y})");
    }
    assert!(ink(&image, 78, 50) < 0.05, "square dot extent");
    // P3: butt cap draws nothing.
    assert!(ink_sum(&image, 140..160, 40..60) < 0.01, "butt dot");
    // P4: duplicates do not cancel.
    assert_color("G1 centre", &image, 30, 100, [255, 0, 0]);
    assert_color("G1 corner", &image, 12, 82, [255, 0, 0]);
    // P5.
    assert_color("G2 fill", &image, 90, 100, [0, 160, 0]);
    assert!(ink(&image, 70, 100) > 0.95 && ink(&image, 71, 100) > 0.95, "G2 stroke");
    assert_color("G2 fill near stroke", &image, 74, 100, [0, 160, 0]);
    assert!(ink(&image, 69, 79) > 0.4, "G2 stroke corner {:?}", pixel(&image, 69, 79));
    // P6.
    assert_color("G3 centre", &image, 150, 100, [0, 0, 255]);
    assert_color("G3 corner", &image, 130, 80, [0, 0, 255]);
    assert_color("G3 corner 2", &image, 169, 119, [0, 0, 255]);
    // P7.
    assert_white("G4 zero area", &image, 185, 80);
    assert_white("G4 lone point", &image, 185, 100);
}
