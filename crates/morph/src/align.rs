//! Bringing two matched contours to the same number of cubics with corresponding start points and direction.
//!
//! Without hints the start point and direction that minimise the total vertex distance win. A hint pins an arc
//! position of `a` to one of `b`: the contours are split there and each piece pair is resampled on its own.

use anim_core::kurbo::{CubicBez, ParamCurve};
use anim_core::num::{round_f64_to_u32, u32_to_usize, usize_to_f64};

use crate::MorphMode;
use crate::contour::{Contour, resample_chain, split_to};

/// Fewest cubics a distributive resample produces.
pub const MIN_SEGMENTS: usize = 16;
/// Most cubics a contour is resampled to.
pub const MAX_SEGMENTS: usize = 512;
const EPS: f64 = 1.0e-9;

/// A correspondence: arc position `a` of the first contour is arc position `b` of the second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub a: f64,
    pub b: f64,
}

/// Two chains of equal length, cubic `i` of one corresponding to cubic `i` of the other.
pub type Pair = (Vec<CubicBez>, Vec<CubicBez>);

fn cost(a: &[CubicBez], b: &[CubicBez]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x.p0.distance_squared(y.p0) + x.eval(0.5).distance_squared(y.eval(0.5))).sum()
}

fn piece_pair(a: Vec<CubicBez>, b: Vec<CubicBez>, k: usize, mode: MorphMode) -> Pair {
    match mode {
        MorphMode::Distributive => (resample_chain(&a, k), resample_chain(&b, k)),
        MorphMode::Angular => {
            let n = a.len().max(b.len()).clamp(1, MAX_SEGMENTS);
            (split_to(a, n), split_to(b, n))
        }
    }
}

/// Sorts anchors by their position on `a` and drops those that repeat a position on either side.
fn clean(anchors: &[Anchor], la: f64, lb: f64, closed: bool) -> Vec<Anchor> {
    let mut sorted: Vec<Anchor> = anchors.iter().copied().filter(|x| x.a.is_finite() && x.b.is_finite()).collect();
    for x in &mut sorted {
        if closed {
            x.a = if la > EPS { x.a.rem_euclid(la) } else { 0.0 };
            x.b = if lb > EPS { x.b.rem_euclid(lb) } else { 0.0 };
        } else {
            x.a = x.a.clamp(0.0, la);
            x.b = x.b.clamp(0.0, lb);
        }
    }
    sorted.sort_by(|p, q| p.a.total_cmp(&q.a));
    let mut out: Vec<Anchor> = Vec::new();
    for x in sorted {
        let dup = out.iter().any(|y| (y.a - x.a).abs() < 1.0e-6 || (y.b - x.b).abs() < 1.0e-6);
        if !dup {
            out.push(x);
        }
    }
    out
}

/// True when walking the anchors in `a` order also walks `b` in order (cyclically for closed contours).
fn monotone(anchors: &[Anchor], closed: bool) -> bool {
    let descents = anchors.iter().zip(anchors.iter().skip(1)).filter(|(p, q)| q.b < p.b).count();
    if closed {
        let wrap = match (anchors.first(), anchors.last()) {
            (Some(f), Some(l)) if anchors.len() > 1 => usize::from(f.b < l.b),
            _ => 0,
        };
        descents.saturating_add(wrap) <= 1
    } else {
        descents == 0
    }
}

fn segments_for(total_count: usize) -> usize {
    total_count.clamp(MIN_SEGMENTS, MAX_SEGMENTS)
}

fn share_count(n: usize, share: f64) -> usize {
    u32_to_usize(round_f64_to_u32((usize_to_f64(n) * share).ceil())).max(1)
}

/// Pieces between consecutive anchors, resampled and concatenated.
fn build_pieces(a: &Contour, b: &Contour, anchors: &[Anchor], closed: bool, mode: MorphMode) -> Pair {
    let (la, lb) = (a.length(), b.length());
    let n = segments_for(a.segs.len().max(b.segs.len()));
    // Piece boundaries as (a, b) positions plus the final one.
    let mut bounds: Vec<Anchor> = Vec::new();
    if closed {
        bounds.extend_from_slice(anchors);
        if let Some(first) = anchors.first() {
            bounds.push(Anchor { a: first.a + la, b: first.b + lb });
        }
    } else {
        bounds.push(Anchor { a: 0.0, b: 0.0 });
        bounds.extend_from_slice(anchors);
        bounds.push(Anchor { a: la, b: lb });
    }
    let mut out: Pair = (Vec::new(), Vec::new());
    for (cur, next) in bounds.iter().zip(bounds.iter().skip(1)) {
        let (span_a, span_b) = (next.a - cur.a, next.b - cur.b);
        if span_a <= EPS && span_b <= EPS {
            continue;
        }
        let share = f64::midpoint(if la > EPS { span_a / la } else { 0.0 }, if lb > EPS { span_b / lb } else { 0.0 });
        let (pa, pb) = piece_pair(a.arc(cur.a, span_a), b.arc(cur.b, span_b), share_count(n, share), mode);
        out.0.extend(pa);
        out.1.extend(pb);
    }
    out
}

/// Whole-contour pair with the best cyclic shift of `b`.
fn build_free(a: &Contour, b: &Contour, closed: bool, mode: MorphMode) -> Pair {
    let n = segments_for(a.segs.len().max(b.segs.len()));
    let (pa, mut pb) = piece_pair(a.segs.clone(), b.segs.clone(), n, mode);
    if closed {
        let mut best = (f64::INFINITY, 0usize);
        for shift in 0..pb.len() {
            let mut rotated = pb.clone();
            rotated.rotate_left(shift);
            let c = cost(&pa, &rotated);
            if c < best.0 {
                best = (c, shift);
            }
        }
        pb.rotate_left(best.1);
        // Rotating breaks the snapped chain only at the old seam, which is closed anyway.
    }
    (pa, pb)
}

fn flip_anchors(anchors: &[Anchor], lb: f64) -> Vec<Anchor> {
    let mut v: Vec<Anchor> = anchors.iter().map(|x| Anchor { a: x.a, b: lb - x.b }).collect();
    v.sort_by(|p, q| p.a.total_cmp(&q.a));
    v
}

/// Brings `a` and `b` to the same number of cubics with corresponding vertices. Returns the two chains and
/// whether the result is closed (both inputs closed).
#[must_use]
pub fn align(a: &Contour, b: &Contour, anchors: &[Anchor], mode: MorphMode) -> (Pair, bool) {
    let closed = a.closed && b.closed;
    let (la, lb) = (a.length(), b.length());
    let anchors = clean(anchors, la, lb, closed);
    let bflip = b.reversed();
    let mut best: Option<(f64, Pair)> = None;
    // Hints pin points, not the direction: keep the winding of the keyframes (reversing a square pinned at one corner
    // would collapse the in-betweens) and reverse only when the contours wind oppositely or the hints demand it.
    let preferred = (closed && !anchors.is_empty())
        .then(|| a.area() * b.area() < 0.0)
        .filter(|&flip| monotone(&if flip { flip_anchors(&anchors, lb) } else { anchors.clone() }, closed));
    for flip in [false, true] {
        if preferred.is_some_and(|p| p != flip) {
            continue;
        }
        let candidate = if flip { &bflip } else { b };
        let anchors_here = if flip { flip_anchors(&anchors, lb) } else { anchors.clone() };
        if !monotone(&anchors_here, closed) {
            continue;
        }
        let pair = if anchors_here.is_empty() { build_free(a, candidate, closed, mode) } else { build_pieces(a, candidate, &anchors_here, closed, mode) };
        let c = cost(&pair.0, &pair.1);
        if best.as_ref().is_none_or(|(bc, _)| c < *bc) {
            best = Some((c, pair));
        }
    }
    if let Some((_, pair)) = best {
        (pair, closed)
    } else {
        // Contradictory hints: keep only the first.
        let first: Vec<Anchor> = anchors.iter().take(1).copied().collect();
        align(a, b, &first, mode)
    }
}
