//! Building the correspondence between two shapes once, then sampling it at any `t`.

use std::collections::BTreeMap;

use anim_core::kurbo::CubicBez;
use anim_core::{BezPath, Point};
use anim_shape::{EdgeShape, FillStyle, PathInput, StrokeStyle};

use crate::align::{Anchor, align};
use crate::contour::{Contour, point, to_bez_path};
use crate::style::{lerp_fill, lerp_stroke};
use crate::{MorphMode, MorphOptions, ShapeHint};

/// A hint further than this many pixels from every contour is ignored.
const HINT_SNAP: f64 = 24.0;
/// Above this many candidate pairs, contours are matched by order only.
const MAX_CANDIDATES: usize = 250_000;

/// One contour of a shape with the facts the matcher compares.
struct Part {
    style: u32,
    contour: Contour,
    area: f64,
    length: f64,
    centroid: Point,
}

impl Part {
    fn new(style: u32, contour: Contour) -> Self {
        Self { style, area: contour.area(), length: contour.length(), centroid: contour.centroid(), contour }
    }
}

/// The fill contours and the stroke polylines of a shape.
fn parts(shape: &EdgeShape) -> (Vec<Part>, Vec<Part>) {
    let assembled = shape.assemble();
    let fills = assembled
        .fills
        .iter()
        .flat_map(|region| region.contours().into_iter().filter_map(|p| Contour::from_path(&p)).map(|c| Part::new(region.style, c)))
        .collect();
    let strokes = assembled
        .strokes
        .iter()
        .flat_map(|run| run.polylines().into_iter().filter_map(|p| Contour::from_path(&p)).map(|c| Part::new(run.style, c)))
        .collect();
    (fills, strokes)
}

fn nearest_part(parts: &[Part], p: Point) -> Option<(usize, f64)> {
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let (s, d) = part.contour.nearest(p);
            (i, s, d)
        })
        .filter(|&(_, _, d)| d <= HINT_SNAP)
        .min_by(|x, y| x.2.total_cmp(&y.2))
        .map(|(i, s, _)| (i, s))
}

type Forced = BTreeMap<usize, usize>;
type Anchors = BTreeMap<(usize, usize), Vec<Anchor>>;

/// Pins pairs of parts and arc positions from the hints.
fn resolve_hints(hints: &[ShapeHint], a: &[Part], b: &[Part]) -> (Forced, Anchors) {
    let mut forced = Forced::new();
    let mut anchors = Anchors::new();
    for hint in hints {
        let (Some((ia, sa)), Some((ib, sb))) = (nearest_part(a, hint.start), nearest_part(b, hint.end)) else { continue };
        let taken = match forced.get(&ia) {
            Some(&other) => other != ib,
            None => forced.values().any(|&other| other == ib),
        };
        if taken {
            continue;
        }
        forced.insert(ia, ib);
        anchors.entry((ia, ib)).or_default().push(Anchor { a: sa, b: sb });
    }
    (forced, anchors)
}

fn diagonal(a: &[Part], b: &[Part]) -> f64 {
    let rect = a.iter().chain(b).map(|p| p.contour.bounds()).reduce(|x, y| x.union(y));
    rect.map_or(1.0_f64, |r| r.width().hypot(r.height())).max(1.0e-9)
}

/// Pairs the parts of two shapes: forced pairs first, then the cheapest remaining pairs by centroid distance, size,
/// winding and style. Returns one entry per output contour, in the order of `a` and then the leftovers of `b`.
fn pair_up(a: &[Part], b: &[Part], forced: &Forced, style_penalty: &dyn Fn(&Part, &Part) -> f64) -> Vec<(Option<usize>, Option<usize>)> {
    let mut partner_of_a: Vec<Option<usize>> = vec![None; a.len()];
    let mut used_b = vec![false; b.len()];
    for (&ia, &ib) in forced {
        if let (Some(slot), Some(used)) = (partner_of_a.get_mut(ia), used_b.get_mut(ib)) {
            *slot = Some(ib);
            *used = true;
        }
    }
    let free_a: Vec<usize> = partner_of_a.iter().enumerate().filter(|(_, p)| p.is_none()).map(|(i, _)| i).collect();
    let free_b: Vec<usize> = used_b.iter().enumerate().filter(|(_, u)| !**u).map(|(i, _)| i).collect();
    if free_a.len().saturating_mul(free_b.len()) <= MAX_CANDIDATES {
        let scale = diagonal(a, b);
        let mut candidates: Vec<(f64, usize, usize)> = Vec::with_capacity(free_a.len().saturating_mul(free_b.len()));
        for &ia in &free_a {
            for &ib in &free_b {
                let (Some(pa), Some(pb)) = (a.get(ia), b.get(ib)) else { continue };
                let order = usize::abs_diff(ia, ib);
                let order_cost = 1.0e-3 * anim_core::num::usize_to_f64(order.min(1000));
                let size = (pa.area.abs().sqrt() - pb.area.abs().sqrt()).abs() / scale;
                let length = (pa.length - pb.length).abs() / pa.length.max(pb.length).max(1.0e-9);
                let winding = if pa.area * pb.area < 0.0 { 0.3 } else { 0.0 };
                let cost = 0.2f64.mul_add(length, pa.centroid.distance(pb.centroid) / scale + size) + winding + style_penalty(pa, pb) + order_cost;
                candidates.push((cost, ia, ib));
            }
        }
        candidates.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
        for (_, ia, ib) in candidates {
            if let (Some(slot), Some(used)) = (partner_of_a.get_mut(ia), used_b.get_mut(ib))
                && slot.is_none()
                && !*used
            {
                *slot = Some(ib);
                *used = true;
            }
        }
    } else {
        for (&ia, &ib) in free_a.iter().zip(&free_b) {
            if let (Some(slot), Some(used)) = (partner_of_a.get_mut(ia), used_b.get_mut(ib)) {
                *slot = Some(ib);
                *used = true;
            }
        }
    }
    let mut out: Vec<(Option<usize>, Option<usize>)> = partner_of_a.iter().enumerate().map(|(ia, p)| (Some(ia), *p)).collect();
    out.extend(used_b.iter().enumerate().filter(|(_, u)| !**u).map(|(ib, _)| (None, Some(ib))));
    out
}

/// One output contour: where it comes from and goes to, as equally long cubic chains.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    a_style: Option<u32>,
    b_style: Option<u32>,
    a: Vec<CubicBez>,
    b: Vec<CubicBez>,
    closed: bool,
}

fn collapse(chain: &[CubicBez], to: Point) -> Vec<CubicBez> {
    chain.iter().map(|_| point(to)).collect()
}

fn build_items(a: &[Part], b: &[Part], forced: &Forced, anchors: &Anchors, mode: MorphMode, style_penalty: &dyn Fn(&Part, &Part) -> f64) -> Vec<Item> {
    pair_up(a, b, forced, style_penalty)
        .into_iter()
        .filter_map(|pair| match pair {
            (Some(ia), Some(ib)) => {
                let (pa, pb) = (a.get(ia)?, b.get(ib)?);
                let hints = anchors.get(&(ia, ib)).map_or(&[][..], Vec::as_slice);
                let ((ca, cb), closed) = align(&pa.contour, &pb.contour, hints, mode);
                Some(Item { a_style: Some(pa.style), b_style: Some(pb.style), a: ca, b: cb, closed })
            }
            (Some(ia), None) => {
                let pa = a.get(ia)?;
                Some(Item {
                    a_style: Some(pa.style),
                    b_style: None,
                    b: collapse(&pa.contour.segs, pa.centroid),
                    a: pa.contour.segs.clone(),
                    closed: pa.contour.closed,
                })
            }
            (None, Some(ib)) => {
                let pb = b.get(ib)?;
                Some(Item {
                    a_style: None,
                    b_style: Some(pb.style),
                    a: collapse(&pb.contour.segs, pb.centroid),
                    b: pb.contour.segs.clone(),
                    closed: pb.contour.closed,
                })
            }
            (None, None) => None,
        })
        .collect()
}

fn lerp_chain(a: &[CubicBez], b: &[CubicBez], t: f64) -> Vec<CubicBez> {
    a.iter().zip(b).map(|(x, y)| CubicBez::new(x.p0.lerp(y.p0, t), x.p1.lerp(y.p1, t), x.p2.lerp(y.p2, t), x.p3.lerp(y.p3, t))).collect()
}

type StylePair = (Option<u32>, Option<u32>);

/// Gathers the in-between contours into one path per style pair, so holes stay holes of their region. A contour that
/// exists at only one end joins the group of a matched pair with the same style on that side (a hole that grows in a
/// square tweening to a ring is part of the ring); only when there is none does it form its own group.
fn group_items(items: &[Item], t: f64) -> BTreeMap<StylePair, BezPath> {
    let mut groups: BTreeMap<StylePair, BezPath> = BTreeMap::new();
    let (matched, lone): (Vec<&Item>, Vec<&Item>) = items.iter().partition(|i| i.a_style.is_some() && i.b_style.is_some());
    for item in matched {
        groups.entry((item.a_style, item.b_style)).or_default().extend(to_bez_path(&lerp_chain(&item.a, &item.b, t), item.closed).elements().iter().copied());
    }
    for item in lone {
        let host = groups.keys().find(|(a, b)| (item.a_style.is_some() && *a == item.a_style) || (item.b_style.is_some() && *b == item.b_style)).copied();
        groups
            .entry(host.unwrap_or((item.a_style, item.b_style)))
            .or_default()
            .extend(to_bez_path(&lerp_chain(&item.a, &item.b, t), item.closed).elements().iter().copied());
    }
    groups
}

/// A prepared tween between two shapes: contours matched, aligned and resampled once; [`MorphPlan::at`] is cheap.
#[derive(Debug, Clone)]
pub struct MorphPlan {
    start: EdgeShape,
    end: EdgeShape,
    fills: Vec<Item>,
    strokes: Vec<Item>,
}

impl MorphPlan {
    /// Matches the contours of `start` and `end` and aligns each matched pair.
    #[must_use]
    pub fn new(start: &EdgeShape, end: &EdgeShape, options: &MorphOptions) -> Self {
        let ((fa, sa), (fb, sb)) = (parts(start), parts(end));
        let (fill_forced, fill_anchors) = resolve_hints(&options.hints, &fa, &fb);
        let (stroke_forced, stroke_anchors) = resolve_hints(&options.hints, &sa, &sb);
        let fill_penalty = |x: &Part, y: &Part| -> f64 {
            let (p, q) = (start.fill(x.style), end.fill(y.style));
            match (p, q) {
                (Some(p), Some(q)) if p == q => 0.0,
                _ if x.style == y.style => 0.15,
                _ => 0.5,
            }
        };
        let stroke_penalty = |x: &Part, y: &Part| -> f64 {
            let (p, q) = (start.stroke(x.style), end.stroke(y.style));
            match (p, q) {
                (Some(p), Some(q)) if p == q => 0.0,
                _ if x.style == y.style => 0.15,
                _ => 0.5,
            }
        };
        Self {
            fills: build_items(&fa, &fb, &fill_forced, &fill_anchors, options.mode, &fill_penalty),
            strokes: build_items(&sa, &sb, &stroke_forced, &stroke_anchors, options.mode, &stroke_penalty),
            start: start.clone(),
            end: end.clone(),
        }
    }

    /// The shape `t` of the way along the tween (`t` is not eased here; 0 is the start shape, 1 the end shape).
    #[must_use]
    pub fn at(&self, t: f64) -> EdgeShape {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        if t <= 0.0 {
            return self.start.clone();
        }
        if t >= 1.0 {
            return self.end.clone();
        }
        let fill_groups = group_items(&self.fills, t);
        let stroke_groups = group_items(&self.strokes, t);
        let mut inputs: Vec<PathInput> = Vec::new();
        for ((a, b), path) in fill_groups {
            let style = match (a.and_then(|i| self.start.fill(i)), b.and_then(|i| self.end.fill(i))) {
                (Some(x), Some(y)) => lerp_fill(x, y, t),
                (Some(x), None) => x.clone(),
                (None, Some(y)) => y.clone(),
                (None, None) => FillStyle::default(),
            };
            inputs.push(PathInput::new(path, Some(style), None));
        }
        for ((a, b), path) in stroke_groups {
            let style = match (a.and_then(|i| self.start.stroke(i)), b.and_then(|i| self.end.stroke(i))) {
                (Some(x), Some(y)) => lerp_stroke(x, y, t),
                (Some(x), None) => x.clone(),
                (None, Some(y)) => y.clone(),
                (None, None) => StrokeStyle::plain(1.0, anim_core::Rgba8::BLACK),
            };
            inputs.push(PathInput::new(path, None, Some(style)));
        }
        EdgeShape::from_paths(&inputs)
    }

    /// Number of fill contours the tween animates (matched pairs plus shrinking and growing ones).
    #[must_use]
    pub const fn fill_contours(&self) -> usize {
        self.fills.len()
    }

    /// Number of stroke polylines the tween animates.
    #[must_use]
    pub const fn stroke_contours(&self) -> usize {
        self.strokes.len()
    }

    /// Fill contours that exist only at the start (they shrink away) and only at the end (they grow).
    #[must_use]
    pub fn unmatched_fills(&self) -> (usize, usize) {
        (self.fills.iter().filter(|i| i.b_style.is_none()).count(), self.fills.iter().filter(|i| i.a_style.is_none()).count())
    }
}
