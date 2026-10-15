//! Property tests: hostile text never panics, encoded text reads back, and assembly does not depend on the order
//! or direction the edges are stored in.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::suboptimal_flops,
    clippy::unwrap_used,
    reason = "test code"
)]

use anim_core::kurbo::{PathEl, Shape};
use anim_core::{Point, Rgba8};
use anim_shape::codec::{encode_cubic, parse_cubics};
use anim_shape::number::parse_number;
use anim_shape::{Edge, EdgeShape, FillStyle, Segment, encode_edges, parse_edges};
use proptest::prelude::*;

fn grid_coord() -> impl Strategy<Value = f64> {
    (-25_000_000_i32..25_000_000).prop_map(|v| f64::from(v) / 256.0)
}

fn grid_point() -> impl Strategy<Value = Point> {
    (grid_coord(), grid_coord()).prop_map(|(x, y)| Point::new(x, y))
}

fn line_or_quad() -> impl Strategy<Value = Segment> {
    // Not `prop_oneof!`: it shares its alternatives through an `Arc`.
    (any::<bool>(), grid_point(), grid_point(), grid_point())
        .prop_map(|(curved, from, ctrl, to)| if curved { Segment::Quad { from, ctrl, to } } else { Segment::Line { from, to } })
}

proptest! {
    #[test]
    fn parsers_never_panic_on_arbitrary_text(text in ".{0,400}") {
        let _ = parse_edges(&text);
        let _ = parse_cubics(&text);
        let _ = parse_number(&text);
    }

    #[test]
    fn parsers_never_panic_on_grammar_soup(text in "[!|/\\[S#.\\-+ 0-9A-Fa-f,;()qpQ\\r\\n]{0,300}") {
        let _ = parse_edges(&text);
        let _ = parse_cubics(&text);
    }

    #[test]
    fn whatever_parses_encodes_and_reads_back_the_same(text in "[!|/\\[S#. 0-9A-F-]{0,200}") {
        if let Ok(segments) = parse_edges(&text) {
            let reparsed = parse_edges(&encode_edges(&segments)).unwrap();
            prop_assert_eq!(reparsed.len(), segments.len());
        }
    }

    #[test]
    fn lines_and_quads_round_trip_exactly(segments in proptest::collection::vec(line_or_quad(), 0..40)) {
        let text = encode_edges(&segments);
        prop_assert_eq!(parse_edges(&text).unwrap(), segments);
    }

    #[test]
    fn cubics_round_trip_exactly(from in grid_point(), c1 in grid_point(), c2 in grid_point(), to in grid_point()) {
        let cubic = Segment::Cubic { from, ctrl1: c1, ctrl2: c2, to };
        let text = encode_cubic(&cubic).unwrap();
        let runs = parse_cubics(&text).unwrap();
        prop_assert_eq!(runs.len(), 1);
        prop_assert_eq!(runs[0].cubic, cubic);
    }

    #[test]
    fn assembly_ignores_edge_order_and_direction(
        radii in proptest::collection::vec(20.0_f64..100.0, 3..14),
        flips in proptest::collection::vec(any::<bool>(), 14),
        seed in any::<u64>(),
    ) {
        // A star-shaped polygon around (200, 200) px, stored one edge per side.
        let n = radii.len();
        let points: Vec<Point> = radii
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let angle = std::f64::consts::TAU * i as f64 / n as f64;
                Point::new(200.0 + r * angle.cos(), 200.0 + r * angle.sin())
            })
            .collect();
        let twips = |p: Point| Point::new(p.x * 20.0, p.y * 20.0);
        let area2: f64 = (0..n).map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        }).sum();
        let expected_area = area2.abs() / 2.0;
        let mut edges: Vec<Edge> = (0..n).map(|i| {
            let segment = Segment::Line { from: twips(points[i]), to: twips(points[(i + 1) % n]) };
            // Positive shoelace area is clockwise on screen, interior on the right.
            let (fill0, fill1) = if area2 > 0.0 { (0, 1) } else { (1, 0) };
            if flips[i] { Edge::new(fill1, fill0, 0, vec![segment.reversed()]) } else { Edge::new(fill0, fill1, 0, vec![segment]) }
        }).collect();
        // Deterministic shuffle from the seed.
        let mut state = seed | 1;
        for i in (1..edges.len()).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            edges.swap(i, (state % (i as u64 + 1)) as usize);
        }
        let shape = EdgeShape { fills: vec![FillStyle::Solid(Rgba8::BLACK)], strokes: vec![], edges };
        let assembled = shape.assemble();
        prop_assert_eq!(assembled.fills.len(), 1);
        let path = &assembled.fills[0].path;
        prop_assert_eq!(path.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count(), 1);
        prop_assert!((path.area().abs() - expected_area).abs() < 1e-6 * expected_area.max(1.0));
        prop_assert_eq!(shape.fill_at(Point::new(200.0, 200.0)), Some(1));
        prop_assert_eq!(shape.fill_at(Point::new(400.0, 400.0)), None);
    }
}
