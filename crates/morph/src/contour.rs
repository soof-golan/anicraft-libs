//! Contours as chains of cubic Béziers, with arc-length addressing.
//!
//! Everything the morph does to geometry goes through this representation: lines and quads are raised to cubics,
//! so two contours can be split and resampled to the same number of segments and interpolated control point by
//! control point. Positions on a contour are arc lengths in pixels.

use anim_core::kurbo::{CubicBez, ParamCurve, ParamCurveArclen, ParamCurveArea, ParamCurveExtrema, ParamCurveNearest, PathEl};
use anim_core::num::usize_to_f64;
use anim_core::{BezPath, Point, Rect, Vec2};

/// Arc-length accuracy in pixels.
pub const ACC: f64 = 0.01;
/// Lengths below this are treated as zero.
const EPS: f64 = 1.0e-9;
/// An open path whose end points are this close (pixels) is a closed contour.
const CLOSE_TOL: f64 = 0.05;

/// A chain of connected cubics, closed (the last ends where the first starts) or open.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    pub segs: Vec<CubicBez>,
    pub closed: bool,
    /// Arc length of each cubic, measured once.
    lens: Vec<f64>,
}

/// A straight cubic from `a` to `b`.
#[must_use]
pub fn line(a: Point, b: Point) -> CubicBez {
    CubicBez::new(a, a.lerp(b, 1.0 / 3.0), a.lerp(b, 2.0 / 3.0), b)
}

/// A cubic that stays at `p`.
#[must_use]
pub fn point(p: Point) -> CubicBez {
    CubicBez::new(p, p, p, p)
}

/// The cubic with the same shape traversed backwards.
#[must_use]
pub fn rev_cubic(c: CubicBez) -> CubicBez {
    CubicBez::new(c.p3, c.p2, c.p1, c.p0)
}

const fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn is_point(c: &CubicBez) -> bool {
    c.p0.distance(c.p1) < EPS && c.p0.distance(c.p2) < EPS && c.p0.distance(c.p3) < EPS
}

fn t_at(c: &CubicBez, len: f64, l: f64) -> f64 {
    if l <= 0.0 {
        0.0
    } else if l >= len {
        1.0
    } else {
        c.inv_arclen(l, ACC).clamp(0.0, 1.0)
    }
}

/// Lengths of each cubic of a chain.
#[must_use]
pub fn chain_lens(segs: &[CubicBez]) -> Vec<f64> {
    segs.iter().map(|c| c.arclen(ACC)).collect()
}

/// Cuts pieces out of a connected chain. Pieces asked for in increasing order cost only the cubics they cover, and
/// the parameter of a cut shared by two neighbouring pieces is solved once.
struct Cutter<'a> {
    segs: &'a [CubicBez],
    lens: &'a [f64],
    /// Arc position at which each cubic starts.
    starts: Vec<f64>,
    /// First cubic that can still overlap a piece.
    first: usize,
    /// The last solved cut: cubic, arc position within it and parameter.
    last_cut: Option<(usize, f64, f64)>,
}

impl<'a> Cutter<'a> {
    fn new(segs: &'a [CubicBez], lens: &'a [f64]) -> Self {
        let starts = lens
            .iter()
            .scan(0.0, |at, &len| {
                let start = *at;
                *at += len;
                Some(start)
            })
            .collect();
        Self { segs, lens, starts, first: 0, last_cut: None }
    }

    /// The part of the chain between the arc positions `s0 <= s1` (clamped to the chain). May be empty. Successive
    /// calls must not move `s0` backwards.
    fn cut(&mut self, s0: f64, s1: f64) -> Vec<CubicBez> {
        let Self { segs, lens, starts, first, last_cut } = self;
        while starts.get(*first).zip(lens.get(*first)).is_some_and(|(a, len)| a + len <= s0) {
            *first = first.saturating_add(1);
        }
        let mut cut_t = |index: usize, seg: &CubicBez, len: f64, l: f64| {
            if l <= 0.0 {
                return 0.0;
            }
            if l >= len {
                return 1.0;
            }
            if let Some((i, at, t)) = *last_cut
                && i == index
                && at.to_bits() == l.to_bits()
            {
                return t;
            }
            let t = t_at(seg, len, l);
            *last_cut = Some((index, l, t));
            t
        };
        let mut out = Vec::new();
        for (i, ((seg, &len), &a)) in segs.iter().zip(*lens).zip(starts.iter()).enumerate().skip(*first) {
            if a >= s1 {
                break;
            }
            let lo = s0.max(a);
            let hi = s1.min(a + len);
            if hi - lo > EPS {
                let (t0, t1) = (cut_t(i, seg, len, lo - a), cut_t(i, seg, len, hi - a));
                out.push(seg.subsegment(t0..t1));
            }
        }
        out
    }
}

/// The part of a connected chain between the arc positions `s0 <= s1` (clamped to the chain). May be empty.
#[must_use]
pub fn extract_chain(segs: &[CubicBez], lens: &[f64], s0: f64, s1: f64) -> Vec<CubicBez> {
    Cutter::new(segs, lens).cut(s0, s1)
}

fn unit_or(v: Vec2, fallback: Vec2) -> Vec2 {
    let l = v.hypot();
    if l > EPS {
        v / l
    } else {
        let f = fallback.hypot();
        if f > EPS { fallback / f } else { Vec2::ZERO }
    }
}

/// One cubic that follows the connected chain `sub` from its start to its end: exact for a single cubic, a
/// Hermite fit (end tangents, arc length) for several.
#[must_use]
pub fn merge_chain(sub: &[CubicBez]) -> Option<CubicBez> {
    let first = sub.first()?;
    let last = sub.last()?;
    if sub.len() == 1 {
        return Some(*first);
    }
    let arc: f64 = sub.iter().map(|c| c.arclen(ACC)).sum();
    let chord = last.p3 - first.p0;
    let d0 = unit_or(first.p1 - first.p0, unit_or(first.p2 - first.p0, chord));
    let d1 = unit_or(last.p3 - last.p2, unit_or(last.p3 - last.p1, chord));
    Some(CubicBez::new(first.p0, first.p0 + d0 * (arc / 3.0), last.p3 - d1 * (arc / 3.0), last.p3))
}

/// Resamples a connected chain into `k` cubics of equal arc length. Ends are snapped so the result is connected.
#[must_use]
pub fn resample_chain(chain: &[CubicBez], lens: &[f64], k: usize) -> Vec<CubicBez> {
    let total: f64 = lens.iter().sum();
    let mut cutter = Cutter::new(chain, lens);
    let start = chain.first().map_or(Point::ZERO, |c| c.p0);
    let end = chain.last().map_or(start, |c| c.p3);
    let kf = usize_to_f64(k.max(1));
    let mut out: Vec<CubicBez> = Vec::with_capacity(k);
    let mut pen = start;
    for j in 0..k {
        let s0 = total * usize_to_f64(j) / kf;
        let s1 = total * usize_to_f64(j.saturating_add(1)) / kf;
        let mut piece = merge_chain(&cutter.cut(s0, s1)).unwrap_or_else(|| point(pen));
        piece.p0 = pen;
        if j.saturating_add(1) == k {
            piece.p3 = end;
        }
        pen = piece.p3;
        out.push(piece);
    }
    out
}

/// Splits the longest cubics in half until the chain has `n` cubics (corners are kept as vertices).
#[must_use]
pub fn split_to(chain: &[CubicBez], lens: &[f64], n: usize) -> Vec<CubicBez> {
    let mut items: Vec<(CubicBez, f64)> = chain.iter().copied().zip(lens.iter().copied()).collect();
    while items.len() < n {
        let Some((idx, &(seg, len))) = items.iter().enumerate().max_by(|x, y| x.1.1.total_cmp(&y.1.1)) else { break };
        let t = if len > EPS { t_at(&seg, len, len / 2.0) } else { 0.5 };
        let (a, b) = (seg.subsegment(0.0..t), seg.subsegment(t..1.0));
        let (la, lb) = (a.arclen(ACC), b.arclen(ACC));
        items.splice(idx..=idx, [(a, la), (b, lb)]);
    }
    items.into_iter().map(|(c, _)| c).collect()
}

impl Contour {
    /// The first subpath of `path` as a contour; `None` when it has no extent or a coordinate is not finite.
    #[must_use]
    pub fn from_path(path: &BezPath) -> Option<Self> {
        let mut segs: Vec<CubicBez> = Vec::new();
        let mut start: Option<Point> = None;
        let mut pen = Point::ZERO;
        let mut closed = false;
        for element in path.elements() {
            match *element {
                PathEl::MoveTo(p) => {
                    if start.is_some() {
                        break;
                    }
                    start = Some(p);
                    pen = p;
                }
                PathEl::LineTo(p) => {
                    start.get_or_insert(pen);
                    segs.push(line(pen, p));
                    pen = p;
                }
                PathEl::QuadTo(c, p) => {
                    start.get_or_insert(pen);
                    segs.push(CubicBez::new(pen, pen + (c - pen) * (2.0 / 3.0), p + (c - p) * (2.0 / 3.0), p));
                    pen = p;
                }
                PathEl::CurveTo(c1, c2, p) => {
                    start.get_or_insert(pen);
                    segs.push(CubicBez::new(pen, c1, c2, p));
                    pen = p;
                }
                PathEl::ClosePath => {
                    closed = true;
                    if let Some(s) = start
                        && pen.distance(s) > EPS
                    {
                        segs.push(line(pen, s));
                    }
                    break;
                }
            }
        }
        if segs.iter().any(|c| !(finite(c.p0) && finite(c.p1) && finite(c.p2) && finite(c.p3))) {
            return None;
        }
        segs.retain(|c| !is_point(c));
        let first = segs.first()?;
        let begin = first.p0;
        if !closed
            && let Some(last) = segs.last_mut()
            && last.p3.distance(begin) < CLOSE_TOL
        {
            last.p3 = begin;
            closed = true;
        }
        Some(Self::new(segs, closed))
    }

    fn new(segs: Vec<CubicBez>, closed: bool) -> Self {
        let lens = chain_lens(&segs);
        Self { segs, closed, lens }
    }

    /// Arc length of each cubic.
    #[must_use]
    pub fn lens(&self) -> &[f64] {
        &self.lens
    }

    #[must_use]
    pub fn length(&self) -> f64 {
        self.lens.iter().sum()
    }

    /// The same contour traversed the other way.
    #[must_use]
    pub fn reversed(&self) -> Self {
        Self::new(self.segs.iter().rev().map(|c| rev_cubic(*c)).collect(), self.closed)
    }

    /// Signed area (positive clockwise on a y-down screen); 0 for open contours.
    #[must_use]
    pub fn area(&self) -> f64 {
        if self.closed { self.segs.iter().map(ParamCurveArea::signed_area).sum() } else { 0.0 }
    }

    /// A representative interior point: the length-weighted mean of the outline.
    #[must_use]
    pub fn centroid(&self) -> Point {
        let mut sum = Vec2::ZERO;
        let mut weight = 0.0;
        for (seg, &w) in self.segs.iter().zip(&self.lens) {
            let mid = (seg.eval(0.25).to_vec2() + seg.eval(0.75).to_vec2()) * 0.5;
            sum += mid * w;
            weight += w;
        }
        if weight > EPS { (sum / weight).to_point() } else { self.segs.first().map_or(Point::ZERO, |c| c.p0) }
    }

    #[must_use]
    pub fn bounds(&self) -> Rect {
        self.segs.iter().map(ParamCurveExtrema::bounding_box).reduce(|a, b| a.union(b)).unwrap_or_default()
    }

    /// The arc position of the point of the contour nearest to `p`, and its distance.
    #[must_use]
    pub fn nearest(&self, p: Point) -> (f64, f64) {
        let mut best = (0.0, f64::INFINITY);
        let mut before = 0.0;
        for (seg, &len) in self.segs.iter().zip(&self.lens) {
            let near = seg.nearest(p, ACC);
            let d = near.distance_sq.sqrt();
            if d < best.1 {
                let s = if near.t <= 0.0 { 0.0 } else { seg.subsegment(0.0..near.t).arclen(ACC) };
                best = (before + s, d);
            }
            before += len;
        }
        best
    }

    /// The part of the contour that starts at arc position `s0` and runs `span` pixels forwards. Closed contours
    /// wrap around; open ones are clamped. Never empty: a zero span gives a point.
    #[must_use]
    pub fn arc(&self, s0: f64, span: f64) -> Vec<CubicBez> {
        let total = self.length();
        let s0 = if self.closed && total > EPS { s0.rem_euclid(total) } else { s0.clamp(0.0, total) };
        let span = span.max(0.0);
        let out = if self.closed && s0 + span > total {
            let doubled: Vec<CubicBez> = self.segs.iter().chain(self.segs.iter()).copied().collect();
            let dlens: Vec<f64> = self.lens.iter().chain(self.lens.iter()).copied().collect();
            extract_chain(&doubled, &dlens, s0, s0 + span)
        } else {
            extract_chain(&self.segs, &self.lens, s0, s0 + span)
        };
        if out.is_empty() {
            let at = extract_chain(&self.segs, &self.lens, s0, total).first().map_or_else(|| self.segs.first().map_or(Point::ZERO, |c| c.p0), |c| c.p0);
            vec![point(at)]
        } else {
            out
        }
    }
}

/// Appends a closed or open subpath of interpolated cubics to `path`. Cubics that are straight become lines.
pub fn push_bez_path(path: &mut BezPath, segs: impl Iterator<Item = CubicBez>, closed: bool) {
    let mut segs = segs.peekable();
    let Some(first) = segs.peek() else { return };
    path.move_to(first.p0);
    for c in segs {
        if straight(&c) {
            path.line_to(c.p3);
        } else {
            path.curve_to(c.p1, c.p2, c.p3);
        }
    }
    if closed {
        path.close_path();
    }
}

fn straight(c: &CubicBez) -> bool {
    let chord = c.p3 - c.p0;
    let len = chord.hypot();
    if len < EPS {
        return c.p0.distance(c.p1) < EPS && c.p0.distance(c.p2) < EPS;
    }
    let off = |p: Point| (p - c.p0).cross(chord).abs() / len;
    off(c.p1) < 1.0e-6 && off(c.p2) < 1.0e-6
}
