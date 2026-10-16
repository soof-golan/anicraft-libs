//! Motion guides: the path a guided classic tween follows.
//!
//! The registration point of every tweened element travels along the guide's first stroke at constant speed (equal
//! arc length per unit of eased position progress). With "orient to path" the element's baseline follows the path
//! tangent: its rotation is the tween's own rotation plus the tangent angle (clockwise positive on screen).

use anim_core::kurbo::{ParamCurve, ParamCurveArclen, PathEl, PathSeg};
use anim_core::{Affine, BezPath, Point, Vec2};
use anim_scene::{Element, Frame};

/// Arc length accuracy in pixels.
const ACCURACY: f64 = 1.0e-6;
/// Half-width in curve parameter of the window used to measure a tangent.
const WINDOW: f64 = 1.0e-4;

/// A guide path with its arc lengths.
#[derive(Debug, Clone)]
pub struct Guide {
    segments: Vec<(PathSeg, f64)>,
    total: f64,
}

impl Guide {
    /// The guide of a path: its first subpath. `None` when that has no length.
    #[must_use]
    pub fn new(path: &BezPath) -> Option<Self> {
        let mut elements = path.elements().iter().copied();
        let mut first = BezPath::new();
        first.extend(elements.next());
        first.extend(elements.take_while(|e| !matches!(e, PathEl::MoveTo(_))));
        let segments: Vec<(PathSeg, f64)> = first.segments().map(|seg| (seg, seg.arclen(ACCURACY))).filter(|(_, len)| len.is_finite() && *len > 0.0).collect();
        let total = segments.iter().map(|(_, l)| l).sum::<f64>();
        (total > 0.0).then_some(Self { segments, total })
    }

    /// The guide of a keyframe's content: the first stroke of its first shape, in the keyframe's space.
    #[must_use]
    pub fn from_keyframe(frame: &Frame) -> Option<Self> {
        frame.elements.iter().find_map(|element| {
            let Element::Shape(shape) = element else { return None };
            let assembled = shape.shape.assemble();
            let run = assembled.strokes.first()?;
            let path = run.polylines().into_iter().next()?;
            Self::new(&(shape.matrix * path))
        })
    }

    /// Point and tangent angle (radians, clockwise on screen) at `fraction` (0 to 1) of the length. `reversed`
    /// walks the path from its end.
    #[must_use]
    pub fn at(&self, fraction: f64, reversed: bool) -> (Point, f64) {
        let along = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
        let target = if reversed { 1.0 - along } else { along } * self.total;
        let mut walked = 0.0;
        let mut found = self.segments.first().map(|(s, _)| (s, 0.0));
        for (seg, len) in &self.segments {
            found = Some((seg, seg.inv_arclen((target - walked).clamp(0.0, *len), ACCURACY)));
            if target <= walked + len {
                break;
            }
            walked += len;
        }
        let Some((seg, t)) = found else { return (Point::ORIGIN, 0.0) };
        let (a, b) = ((t - WINDOW).max(0.0), (t + WINDOW).min(1.0));
        let d = seg.eval(b) - seg.eval(a);
        let angle = if d.hypot() > 0.0 { d.y.atan2(d.x) } else { 0.0 };
        (seg.eval(t), if reversed { angle + std::f64::consts::PI } else { angle })
    }

    /// True when the path runs from `to` towards `from` rather than `from` towards `to`: a guide drawn in either
    /// direction is walked from the start keyframe's end.
    #[must_use]
    pub fn runs_backwards(&self, from: Point, to: Point) -> bool {
        let (head, tail) = (self.at(0.0, false).0, self.at(1.0, false).0);
        head.distance(to) + tail.distance(from) < head.distance(from) + tail.distance(to)
    }
}

/// Moves a tweened matrix onto the guide: translation from the path (plus, unless `snap`, the keyframes' offsets
/// from the path ends, faded in linearly), rotation turned by the tangent when `orient`.
#[must_use]
pub fn follow(m: Affine, from: Affine, to: Affine, guide: &Guide, progress: f64, snap: bool, orient: bool) -> Affine {
    let (a, b) = (from.translation().to_point(), to.translation().to_point());
    let reversed = guide.runs_backwards(a, b);
    let (point, angle) = guide.at(progress, reversed);
    let (head, tail) = (guide.at(0.0, reversed).0, guide.at(1.0, reversed).0);
    let offset = if snap {
        Vec2::ZERO
    } else {
        let (start, end) = (a - head, b - tail);
        start + (end - start) * progress
    };
    let mut parts = crate::xform::decompose(m);
    parts.tx = point.x + offset.x;
    parts.ty = point.y + offset.y;
    if orient {
        parts.rotation += angle;
    }
    crate::xform::compose(&parts)
}
