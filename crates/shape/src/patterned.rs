//! Patterned strokes (`DashedStroke`, `DottedStroke`, `RaggedStroke`, `StippleStroke`, `HatchedStroke`) as display
//! nodes.
//!
//! * **Dashed** and **dotted** become an ordinary stroke with a dash array: `dash1 gap1 dash2 gap2` for dashed, a
//!   zero-length dash with round caps every `weight + dotSpace` for dotted.
//! * **Ragged** walks the path and displaces a vertex every wave length alternately to either side by the wave
//!   height; `random` patterns jitter both. The `dotted` patterns add a dash array on top.
//! * **Stipple** scatters filled dots in the band of the stroke's width, at a density and size from the attributes.
//! * **Hatched** crosses the path with short lines at the spacing, thickness, jiggle and length variation given.
//!
//! The randomness is a fixed-seed generator seeded from the path, so the same shape always produces the same pattern
//! and exports are reproducible. The attribute vocabularies are Animate's (`"very dense"`, `"wavy"`, ...); unknown
//! values fall back to the middle setting. The look is a faithful imitation of Animate's, not a pixel copy: Animate's
//! generator is not documented.

#![allow(clippy::suboptimal_flops, reason = "no fused multiply-add: patterns must be identical on every CPU")]
#![allow(clippy::while_float, reason = "the walk advances a distance along the path until it runs out")]

use anim_core::kurbo::{BezPath, PathEl, flatten};
use anim_core::{Affine, Point, Vec2};
use anim_display::{Cap, Fill, FillRule, Node, Paint, Stroke, StrokeScale, StrokeStyle as DisplayStroke};

use crate::styles::{OtherStroke, StrokeKind};

/// Most dots or hatch lines generated for one stroke run.
const MAX_ITEMS: usize = 100_000;
/// Flattening tolerance in pixels.
const FLATNESS: f64 = 0.1;
/// Length of the "zero length" dash that makes a round-capped dot.
const DOT: f64 = 0.01;

/// The display nodes of `path` stroked with the patterned style `other`; `base` is the solid geometry of the stroke
/// and `paint` its fill.
#[must_use]
pub fn nodes(path: &BezPath, other: &OtherStroke, base: &DisplayStroke, paint: Paint) -> Vec<Node> {
    let attrs = Attrs(&other.attrs);
    match other.kind {
        StrokeKind::Dashed => vec![stroke(path.clone(), DisplayStroke { dashes: dashed(&attrs), ..base.clone() }, paint)],
        StrokeKind::Dotted => {
            let period = base.width.max(0.5) + attrs.number("dotSpace", 3.0).max(0.0);
            vec![stroke(path.clone(), DisplayStroke { cap: Cap::Round, dashes: vec![DOT, period], ..base.clone() }, paint)]
        }
        StrokeKind::Ragged => ragged(path, &attrs, base, paint),
        StrokeKind::Stipple => stipple(path, &attrs, base.width, paint),
        StrokeKind::Hatched => hatched(path, &attrs, base.width, paint),
    }
}

const fn stroke(path: BezPath, style: DisplayStroke, paint: Paint) -> Node {
    Node::Stroke(Stroke { transform: Affine::IDENTITY, path, style, paint })
}

fn dashed(attrs: &Attrs<'_>) -> Vec<f64> {
    let dash1 = attrs.number("dash1", 4.0).max(0.0);
    let gap1 = attrs.number("gap1", 4.0).max(0.0);
    let dash2 = attrs.number("dash2", 0.0).max(0.0);
    let gap2 = attrs.number("gap2", 0.0).max(0.0);
    let mut dashes = vec![dash1, gap1];
    if dash2 > 0.0 || gap2 > 0.0 {
        dashes.extend([dash2, gap2]);
    }
    if dashes.iter().sum::<f64>() <= 0.0 { Vec::new() } else { dashes }
}

fn ragged(path: &BezPath, attrs: &Attrs<'_>, base: &DisplayStroke, paint: Paint) -> Vec<Node> {
    let width = base.width.max(1.0);
    let pattern = attrs.text("pattern", "solid");
    let amplitude = width * attrs.choice("waveHeight", "wavy", &[("flat", 0.15), ("wavy", 0.5), ("very wavy", 1.0), ("wild", 1.8)]);
    let wavelength = width * attrs.choice("waveLength", "medium", &[("very short", 0.8), ("short", 1.6), ("medium", 3.0), ("long", 6.0)]);
    let random = pattern.contains("random") || pattern == "wild";
    let mut rng = Rng::seeded(path);
    let mut out = BezPath::new();
    let mut count = 0_usize;
    for (points, _closed) in polylines(path) {
        let Some(first) = points.first().copied() else { continue };
        out.move_to(first);
        let mut side = 1.0_f64;
        walk(
            &points,
            &mut rng,
            &mut count,
            |rng| wavelength * if random { 0.5 + rng.next() } else { 1.0 },
            |p, tangent, rng| {
                let normal = Vec2::new(-tangent.y, tangent.x);
                let height = if random { amplitude * (0.4 + 0.6 * rng.next()) } else { amplitude };
                out.line_to(p + normal * (height * side));
                side = -side;
            },
        );
        if let Some(last) = points.last().copied() {
            out.line_to(last);
        }
    }
    let dashes = match pattern {
        "dotted" | "random dotted" => vec![width, width],
        "triple dotted" | "random triple dotted" => vec![width, width, width, width, width, width * 3.0],
        _ => Vec::new(),
    };
    vec![stroke(out, DisplayStroke { join: anim_display::Join::Round, dashes, ..base.clone() }, paint)]
}

fn stipple(path: &BezPath, attrs: &Attrs<'_>, weight: f64, paint: Paint) -> Vec<Node> {
    let band = weight.max(1.0);
    let diameter = (band * attrs.choice("dotSize", "small", &[("tiny", 0.12), ("small", 0.22), ("medium", 0.35), ("large", 0.5)])).max(0.5);
    let spacing = diameter * attrs.choice("density", "dense", &[("very dense", 0.9), ("dense", 1.5), ("sparse", 2.6), ("very sparse", 4.0)]);
    let variation = attrs.choice("variation", "one size", &[("one size", 0.0), ("small variation", 0.25), ("varied sizes", 0.5), ("random sizes", 0.8)]);
    let mut rng = Rng::seeded(path);
    let mut dots = BezPath::new();
    let mut count = 0_usize;
    for (points, _closed) in polylines(path) {
        walk(
            &points,
            &mut rng,
            &mut count,
            |rng| spacing * (0.5 + rng.next()),
            |p, tangent, rng| {
                let normal = Vec2::new(-tangent.y, tangent.x);
                let across = (rng.next() - 0.5) * band;
                let radius = diameter / 2.0 * (1.0 - variation * rng.next());
                push_circle(&mut dots, p + normal * across, radius.max(0.2));
            },
        );
    }
    vec![Node::Fill(Fill { transform: Affine::IDENTITY, path: dots, rule: FillRule::NonZero, paint })]
}

fn hatched(path: &BezPath, attrs: &Attrs<'_>, weight: f64, paint: Paint) -> Vec<Node> {
    let band = weight.max(1.0);
    let thickness = attrs.choice("thickness", "thin", &[("hairline", 0.0), ("thin", 1.0), ("medium", 2.0), ("thick", 3.0)]);
    let spacing = attrs.choice("spacing", "close", &[("very close", 1.5), ("close", 3.0), ("distant", 6.0), ("very distant", 10.0)]);
    let jiggle = attrs.choice("jiggle", "none", &[("none", 0.0), ("bounce", 0.15), ("loose", 0.35), ("wild", 0.7)]);
    let rotate = attrs.choice("rotate", "none", &[("none", 0.0), ("slight", 0.3), ("free", 0.8)]);
    let curve = attrs.choice("curve", "straight", &[("straight", 0.0), ("slight", 0.1), ("medium", 0.25), ("very curved", 0.5)]);
    let length = attrs.choice("length", "equal", &[("equal", 0.0), ("slight variation", 0.2), ("medium variation", 0.45), ("random", 0.8)]);
    let mut rng = Rng::seeded(path);
    let mut lines = BezPath::new();
    let mut count = 0_usize;
    for (points, _closed) in polylines(path) {
        walk(
            &points,
            &mut rng,
            &mut count,
            |_| spacing,
            |p, tangent, rng| {
                let tilt = (rng.next() - 0.5) * 2.0 * (rotate + jiggle);
                let (sin, cos) = tilt.sin_cos();
                let normal = Vec2::new(-tangent.y, tangent.x);
                let direction = Vec2::new(normal.x * cos - normal.y * sin, normal.x * sin + normal.y * cos);
                let half = band / 2.0 * (1.0 - length * rng.next());
                lines.move_to(p - direction * half);
                let end = p + direction * half;
                if curve > 0.0 {
                    lines.quad_to(p + tangent * (band * curve * (rng.next() - 0.5)), end);
                } else {
                    lines.line_to(end);
                }
            },
        );
    }
    let style = DisplayStroke {
        width: thickness.max(0.5),
        cap: Cap::None,
        scale: if thickness <= 0.0 { StrokeScale::None } else { StrokeScale::Normal },
        ..DisplayStroke::default()
    };
    vec![stroke(lines, style, paint)]
}

fn push_circle(path: &mut BezPath, centre: Point, radius: f64) {
    // Four cubic arcs.
    const K: f64 = 0.552_284_749_830_793_4;
    let (cx, cy, r) = (centre.x, centre.y, radius);
    let k = r * K;
    path.move_to(Point::new(cx + r, cy));
    path.curve_to(Point::new(cx + r, cy + k), Point::new(cx + k, cy + r), Point::new(cx, cy + r));
    path.curve_to(Point::new(cx - k, cy + r), Point::new(cx - r, cy + k), Point::new(cx - r, cy));
    path.curve_to(Point::new(cx - r, cy - k), Point::new(cx - k, cy - r), Point::new(cx, cy - r));
    path.curve_to(Point::new(cx + k, cy - r), Point::new(cx + r, cy - k), Point::new(cx + r, cy));
    path.close_path();
}

/// The path as flattened polylines with a closed flag.
fn polylines(path: &BezPath) -> Vec<(Vec<Point>, bool)> {
    let mut out: Vec<(Vec<Point>, bool)> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let flush = |current: &mut Vec<Point>, closed: bool, out: &mut Vec<(Vec<Point>, bool)>| {
        if current.len() > 1 {
            out.push((std::mem::take(current), closed));
        } else {
            current.clear();
        }
    };
    let mut start = Point::ORIGIN;
    flatten(path.elements().iter().copied(), FLATNESS, |el| match el {
        PathEl::MoveTo(p) => {
            flush(&mut current, false, &mut out);
            start = p;
            current.push(p);
        }
        PathEl::LineTo(p) => current.push(p),
        PathEl::ClosePath => {
            current.push(start);
            flush(&mut current, true, &mut out);
        }
        PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
    });
    flush(&mut current, false, &mut out);
    out
}

/// Calls `emit(point, unit tangent, rng)` at distances `step(rng)` apart along the polyline, the first after a step
/// drawn the same way. `count` is the number of items made so far for the whole stroke; at [`MAX_ITEMS`] the walk stops.
fn walk(points: &[Point], rng: &mut Rng, count: &mut usize, step: impl Fn(&mut Rng) -> f64, mut emit: impl FnMut(Point, Vec2, &mut Rng)) {
    let mut until_next = step(rng) * 0.5;
    for pair in points.windows(2) {
        let (Some(a), Some(b)) = (pair.first().copied(), pair.get(1).copied()) else { continue };
        let delta = b - a;
        let length = delta.hypot();
        if length <= 1.0e-9 || !length.is_finite() {
            continue;
        }
        let tangent = delta / length;
        let mut at = until_next;
        while at <= length {
            if *count >= MAX_ITEMS {
                return;
            }
            emit(a + tangent * at, tangent, rng);
            *count = count.saturating_add(1);
            let next = step(rng);
            at += if next.is_finite() && next > 1.0e-6 { next } else { 1.0 };
        }
        until_next = at - length;
    }
}

/// A fixed-seed generator (xorshift64*), seeded from the path so the pattern is the same on every export.
struct Rng(u64);

impl Rng {
    fn seeded(path: &BezPath) -> Self {
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        for el in path.elements() {
            let p = match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => *p,
                PathEl::ClosePath => continue,
            };
            seed = seed.rotate_left(5) ^ p.x.to_bits().wrapping_mul(0x0100_0000_01B3) ^ p.y.to_bits();
        }
        Self(seed | 1)
    }

    /// A number in `[0, 1)`.
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32;
        f64::from(u32::try_from(value).unwrap_or(0)) / 4_294_967_296.0
    }
}

/// Typed access to a stroke's kind-specific attributes.
struct Attrs<'a>(&'a [(String, String)]);

impl Attrs<'_> {
    fn text<'b>(&'b self, name: &str, default: &'b str) -> &'b str {
        self.0.iter().find(|(key, _)| key == name).map_or(default, |(_, value)| value.as_str())
    }

    fn number(&self, name: &str, default: f64) -> f64 {
        self.0.iter().find(|(key, _)| key == name).and_then(|(_, value)| value.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(default)
    }

    /// The factor listed for the attribute's value; the default's factor when the value is unknown.
    fn choice(&self, name: &str, default: &str, table: &[(&str, f64)]) -> f64 {
        let value = self.text(name, default);
        let find = |wanted: &str| table.iter().find(|(key, _)| *key == wanted).map(|(_, factor)| *factor);
        find(value).or_else(|| find(default)).unwrap_or(1.0)
    }
}

#[cfg(test)]
mod tests {
    use anim_core::Rgba8;

    use super::*;
    use crate::styles::SolidStroke;

    fn other(kind: StrokeKind, weight: f64, attrs: &[(&str, &str)]) -> OtherStroke {
        OtherStroke {
            kind,
            base: SolidStroke { weight, ..SolidStroke::default() },
            attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
        }
    }

    fn line() -> BezPath {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 0.0));
        path
    }

    fn run(kind: StrokeKind, weight: f64, attrs: &[(&str, &str)]) -> Vec<Node> {
        let o = other(kind, weight, attrs);
        nodes(&line(), &o, &crate::display::display_stroke(&o.base), Paint::Solid(Rgba8::BLACK))
    }

    fn path_of(node: &Node) -> &BezPath {
        match node {
            Node::Stroke(s) => &s.path,
            Node::Fill(f) => &f.path,
            _ => panic!("not a path node"),
        }
    }

    fn moves(path: &BezPath) -> usize {
        path.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count()
    }

    #[test]
    fn dashed_strokes_become_a_dash_array() {
        let out = run(StrokeKind::Dashed, 2.0, &[("dash1", "4"), ("gap1", "2")]);
        let Some(Node::Stroke(s)) = out.first() else { panic!("stroke") };
        assert_eq!(s.style.dashes, vec![4.0, 2.0]);
        let out = run(StrokeKind::Dashed, 2.0, &[("dash1", "4"), ("gap1", "2"), ("dash2", "1"), ("gap2", "3")]);
        let Some(Node::Stroke(s)) = out.first() else { panic!("stroke") };
        assert_eq!(s.style.dashes, vec![4.0, 2.0, 1.0, 3.0]);
    }

    #[test]
    fn dotted_strokes_are_round_dots_one_weight_plus_space_apart() {
        let out = run(StrokeKind::Dotted, 2.0, &[("dotSpace", "3")]);
        let Some(Node::Stroke(s)) = out.first() else { panic!("stroke") };
        assert_eq!(s.style.cap, Cap::Round);
        assert!((s.style.dashes[1] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn hatching_crosses_the_path_at_the_spacing() {
        // Spacing "close" is 3 px; the first line is half a step in: 1.5, 4.5, ... 97.5 is 33 lines.
        let out = run(StrokeKind::Hatched, 4.0, &[("spacing", "close")]);
        let path = path_of(&out[0]);
        assert_eq!(moves(path), 33);
        for el in path.elements() {
            if let PathEl::MoveTo(p) | PathEl::LineTo(p) = el {
                assert!((p.y.abs() - 2.0).abs() < 1e-9, "hatch ends sit half a weight off the path: {p:?}");
            }
        }
    }

    #[test]
    fn stipple_scatters_dots_inside_the_band() {
        let out = run(StrokeKind::Stipple, 4.0, &[("dotSize", "small"), ("density", "dense")]);
        let Some(Node::Fill(fill)) = out.first() else { panic!("fill") };
        // Diameter 0.88 px, mean spacing 1.32 px: about 75 dots over 100 px.
        let dots = moves(&fill.path);
        assert!((50..110).contains(&dots), "{dots} dots");
        let bounds = anim_core::kurbo::Shape::bounding_box(&fill.path);
        assert!(bounds.y0 >= -2.5 && bounds.y1 <= 2.5, "{bounds:?}");
    }

    #[test]
    fn ragged_strokes_alternate_sides_within_the_wave_height_and_repeat_exactly() {
        let attrs = [("pattern", "simple"), ("waveHeight", "wavy"), ("waveLength", "short")];
        let a = run(StrokeKind::Ragged, 2.0, &attrs);
        let b = run(StrokeKind::Ragged, 2.0, &attrs);
        assert_eq!(path_of(&a[0]), path_of(&b[0]));
        let ys: Vec<f64> = path_of(&a[0]).elements().iter().filter_map(|e| if let PathEl::LineTo(p) = e { Some(p.y) } else { None }).collect();
        assert!(ys.len() > 10);
        // "wavy" is half a weight: 1 px either side of the path.
        for pair in ys.windows(2).take(ys.len() - 2) {
            assert!((pair[0].abs() - 1.0).abs() < 1e-9 && pair[0] * pair[1] < 0.0, "{pair:?}");
        }
    }

    #[test]
    fn random_patterns_differ_with_the_path_and_never_exceed_the_item_limit() {
        let mut long = BezPath::new();
        long.move_to((0.0, 0.0));
        long.line_to((50_000_000.0, 0.0));
        let o = other(StrokeKind::Hatched, 4.0, &[("spacing", "very close")]);
        let out = nodes(&long, &o, &crate::display::display_stroke(&o.base), Paint::Solid(Rgba8::BLACK));
        assert!(moves(path_of(&out[0])) <= MAX_ITEMS);
    }
}
