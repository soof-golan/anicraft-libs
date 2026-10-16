//! Armatures: bone trees on a layer, and the poses that move them.
//!
//! An armature layer keeps its artwork (the *rest* pose) in its first keyframe, as ordinary elements. The
//! [`Armature`] on the [`Layer`](crate::Layer) says which bones exist and which element each bone drives; every
//! keyframe may carry a [`Pose`] ([`Frame::pose`](crate::Frame)), the rotation of each bone away from its rest angle.
//! Between two pose keyframes whose first has a tween, poses are interpolated (see `anim-timeline`).
//!
//! Units: pixels, y down; angles are radians measured the way `atan2(dy, dx)` measures them on the stage, so a
//! positive rotation is clockwise on screen.

use anim_core::{Affine, Point, Vec2};
use serde::{Deserialize, Serialize};

use crate::extra::Extra;

/// Most bones one armature may hold; readers drop the rest.
pub const MAX_BONES: usize = 1024;

/// What an armature deforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ArmatureKind {
    /// Bones drive symbol instances (each bone moves the element it names).
    #[default]
    Symbol,
    /// Bones deform the vector shapes of the rest keyframe (every point follows the nearby bones).
    Shape,
}

impl ArmatureKind {
    /// The XFL `kind` attribute value.
    #[must_use]
    pub const fn xfl_name(self) -> &'static str {
        match self {
            Self::Symbol => "symbol",
            Self::Shape => "shape",
        }
    }

    /// Parses an XFL `kind` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "symbol" => Some(Self::Symbol),
            "shape" => Some(Self::Shape),
            _ => None,
        }
    }
}

/// One bone of an armature, described at rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmatureBone {
    pub name: String,
    /// Index of the parent bone; always smaller than this bone's own index. `None` for a root.
    pub parent: Option<usize>,
    /// Joint position at rest, stage pixels.
    pub head: Point,
    /// Distance from the head to the tip.
    pub length: f64,
    /// Direction from head to tip at rest, radians.
    pub angle: f64,
    /// Allowed pose rotation `(min, max)` in radians away from the rest angle; `None` is unconstrained.
    pub limits: Option<(f64, f64)>,
    /// Index of the element in the rest keyframe this bone moves (symbol armatures).
    pub element: Option<usize>,
    pub extra: Extra,
}

impl ArmatureBone {
    /// A bone from `head` to `tip` with no limits and no element.
    #[must_use]
    pub fn between(name: impl Into<String>, parent: Option<usize>, head: Point, tip: Point) -> Self {
        let d = tip - head;
        Self { name: name.into(), parent, head, length: d.hypot(), angle: d.y.atan2(d.x), limits: None, element: None, extra: Extra::new() }
    }

    /// Tip position at rest.
    #[must_use]
    pub fn tip(&self) -> Point {
        self.head + Vec2::from_angle(self.angle) * self.length
    }
}

/// The rotation of every bone away from its rest angle, plus a translation of the root.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Pose {
    /// Translation applied to the whole armature (the root bones' heads).
    pub offset: Vec2,
    /// Rotation of each bone relative to its parent's pose, radians; missing entries are zero.
    pub angles: Vec<f64>,
}

impl Pose {
    /// The rest pose of `bones` bones.
    #[must_use]
    pub fn rest(bones: usize) -> Self {
        Self { offset: Vec2::ZERO, angles: vec![0.0; bones] }
    }

    /// Rotation of bone `i` (zero when the pose has no entry).
    #[must_use]
    pub fn angle(&self, i: usize) -> f64 {
        self.angles.get(i).copied().unwrap_or(0.0)
    }

    /// Sets the rotation of bone `i`, growing the list with zeros.
    pub fn set_angle(&mut self, i: usize, angle: f64) {
        if i >= MAX_BONES {
            return;
        }
        if self.angles.len() <= i {
            self.angles.resize(i.saturating_add(1), 0.0);
        }
        if let Some(slot) = self.angles.get_mut(i) {
            *slot = angle;
        }
    }

    /// Straight interpolation from `self` to `other` at `t` (0 ..= 1, already eased).
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f64) -> Self {
        let n = self.angles.len().max(other.angles.len());
        let angles = (0..n).map(|i| (other.angle(i) - self.angle(i)).mul_add(t, self.angle(i))).collect();
        Self { offset: self.offset + (other.offset - self.offset) * t, angles }
    }

    /// True when every number is finite.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.offset.x.is_finite() && self.offset.y.is_finite() && self.angles.iter().all(|a| a.is_finite())
    }
}

/// A tree of bones on a layer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Armature {
    pub name: String,
    pub kind: ArmatureKind,
    /// Parents come before their children.
    pub bones: Vec<ArmatureBone>,
    pub extra: Extra,
}

impl Armature {
    /// An armature without bones.
    #[must_use]
    pub fn new(name: impl Into<String>, kind: ArmatureKind) -> Self {
        Self { name: name.into(), kind, bones: Vec::new(), extra: Extra::new() }
    }

    /// The rest pose.
    #[must_use]
    pub fn rest_pose(&self) -> Pose {
        Pose::rest(self.bones.len())
    }

    /// Appends a bone and returns its index, or `None` when the parent does not come before it or the armature is
    /// full.
    pub fn push(&mut self, bone: ArmatureBone) -> Option<usize> {
        let index = self.bones.len();
        if index >= MAX_BONES || bone.parent.is_some_and(|p| p >= index) {
            return None;
        }
        self.bones.push(bone);
        Some(index)
    }

    /// The bones from the root down to `bone`, root first. Empty when `bone` does not exist.
    #[must_use]
    pub fn chain_to(&self, bone: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut at = Some(bone);
        while let Some(i) = at {
            let Some(b) = self.bones.get(i) else { break };
            out.push(i);
            at = b.parent.filter(|&p| p < i);
            if out.len() > self.bones.len() {
                break;
            }
        }
        out.reverse();
        out
    }

    /// Indices of the bones whose parent is `bone`.
    #[must_use]
    pub fn children_of(&self, bone: usize) -> Vec<usize> {
        self.bones.iter().enumerate().filter(|(_, b)| b.parent == Some(bone)).map(|(i, _)| i).collect()
    }

    /// Removes `bone` and everything below it; later indices shift down and element bindings stay with their bones.
    pub fn remove_subtree(&mut self, bone: usize) -> Vec<usize> {
        let mut gone = vec![false; self.bones.len()];
        for (i, b) in self.bones.iter().enumerate() {
            let hit = i == bone || b.parent.and_then(|p| gone.get(p).copied()).unwrap_or(false);
            if let Some(slot) = gone.get_mut(i) {
                *slot = hit;
            }
        }
        // old index -> new index
        let mut remap: Vec<Option<usize>> = Vec::with_capacity(self.bones.len());
        let mut next = 0usize;
        for g in &gone {
            if *g {
                remap.push(None);
            } else {
                remap.push(Some(next));
                next = next.saturating_add(1);
            }
        }
        let removed: Vec<usize> = gone.iter().enumerate().filter(|(_, g)| **g).map(|(i, _)| i).collect();
        let old = std::mem::take(&mut self.bones);
        for (i, mut b) in old.into_iter().enumerate() {
            if gone.get(i).copied().unwrap_or(false) {
                continue;
            }
            b.parent = b.parent.and_then(|p| remap.get(p).copied().flatten());
            self.bones.push(b);
        }
        removed
    }

    /// True when parents precede children and all numbers are finite.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.bones.iter().enumerate().all(|(i, b)| {
            b.parent.is_none_or(|p| p < i)
                && b.head.x.is_finite()
                && b.head.y.is_finite()
                && b.length.is_finite()
                && b.length >= 0.0
                && b.angle.is_finite()
                && b.limits.is_none_or(|(lo, hi)| lo.is_finite() && hi.is_finite())
        })
    }

    /// `pose` with every angle clamped into its bone's limits.
    #[must_use]
    pub fn clamped(&self, pose: &Pose) -> Pose {
        let mut out = pose.clone();
        for (i, b) in self.bones.iter().enumerate() {
            if let Some((lo, hi)) = b.limits {
                let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
                out.set_angle(i, pose.angle(i).clamp(lo, hi));
            }
        }
        out
    }

    /// For each bone, the rigid motion that takes its rest geometry to where `pose` puts it.
    #[must_use]
    pub fn motions(&self, pose: &Pose) -> Vec<Affine> {
        let mut out: Vec<Affine> = Vec::with_capacity(self.bones.len());
        let root = Affine::translate(pose.offset);
        for (i, bone) in self.bones.iter().enumerate() {
            let parent = bone.parent.filter(|&p| p < i).and_then(|p| out.get(p).copied()).unwrap_or(root);
            out.push(parent * Affine::rotate_about(pose.angle(i), bone.head));
        }
        out
    }

    /// Head and tip of every bone in the pose.
    #[must_use]
    pub fn posed_segments(&self, pose: &Pose) -> Vec<(Point, Point)> {
        self.motions(pose).iter().zip(&self.bones).map(|(m, b)| (*m * b.head, *m * b.tip())).collect()
    }

    /// The bone nearest to `p` in the pose, and the distance to its segment. `None` for an armature without bones.
    #[must_use]
    pub fn nearest_bone(&self, pose: &Pose, p: Point) -> Option<(usize, f64)> {
        self.posed_segments(pose).iter().enumerate().map(|(i, (a, b))| (i, segment_distance(*a, *b, p))).min_by(|x, y| x.1.total_cmp(&y.1))
    }

    /// Where a point of the rest artwork goes in the pose: a blend of the bones' motions weighted by closeness to
    /// each bone's rest segment (linear blend skinning with inverse-distance weights).
    #[must_use]
    pub fn skin_point(&self, p: Point, motions: &[Affine]) -> Point {
        let mut total = 0.0;
        let mut acc = Vec2::ZERO;
        for (bone, m) in self.bones.iter().zip(motions) {
            let d = segment_distance(bone.head, bone.tip(), p);
            let w = 1.0 / d.mul_add(d, 1.0).powi(2);
            total += w;
            acc += (*m * p).to_vec2() * w;
        }
        if total > 0.0 { (acc / total).to_point() } else { p }
    }
}

/// Distance from `p` to the segment `a`..`b`.
#[must_use]
pub fn segment_distance(a: Point, b: Point, p: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.hypot2();
    if len2 < 1.0e-18 {
        return a.distance(p);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    fn arm() -> Armature {
        let mut a = Armature::new("arm", ArmatureKind::Symbol);
        a.push(ArmatureBone::between("upper", None, Point::new(0.0, 0.0), Point::new(100.0, 0.0)));
        a.push(ArmatureBone::between("lower", Some(0), Point::new(100.0, 0.0), Point::new(200.0, 0.0)));
        a
    }

    #[test]
    fn rest_pose_leaves_bones_where_they_are() {
        let a = arm();
        let seg = a.posed_segments(&a.rest_pose());
        assert!(seg[1].1.distance(Point::new(200.0, 0.0)) < 1.0e-9);
    }

    #[test]
    fn rotating_the_parent_swings_the_child_tip() {
        let a = arm();
        let mut p = a.rest_pose();
        p.set_angle(0, FRAC_PI_2);
        let seg = a.posed_segments(&p);
        assert!(seg[0].1.distance(Point::new(0.0, 100.0)) < 1.0e-9, "{:?}", seg[0]);
        assert!(seg[1].0.distance(Point::new(0.0, 100.0)) < 1.0e-9);
        assert!(seg[1].1.distance(Point::new(0.0, 200.0)) < 1.0e-9);
        p.set_angle(1, -FRAC_PI_2);
        let seg = a.posed_segments(&p);
        assert!(seg[1].1.distance(Point::new(100.0, 100.0)) < 1.0e-9, "{:?}", seg[1]);
    }

    #[test]
    fn limits_clamp_the_pose() {
        let mut a = arm();
        a.bones[1].limits = Some((-0.5, 0.25));
        let mut p = a.rest_pose();
        p.set_angle(1, 2.0);
        assert!((a.clamped(&p).angle(1) - 0.25).abs() < 1.0e-12);
        p.set_angle(1, -2.0);
        assert!((a.clamped(&p).angle(1) + 0.5).abs() < 1.0e-12);
    }

    #[test]
    fn parents_must_come_first() {
        let mut a = Armature::new("x", ArmatureKind::Symbol);
        assert!(a.push(ArmatureBone::between("b", Some(0), Point::ORIGIN, Point::new(1.0, 0.0))).is_none());
        assert_eq!(a.push(ArmatureBone::between("a", None, Point::ORIGIN, Point::new(1.0, 0.0))), Some(0));
        assert!(a.is_well_formed());
    }

    #[test]
    fn removing_a_subtree_renumbers() {
        let mut a = arm();
        a.push(ArmatureBone::between("hand", Some(1), Point::new(200.0, 0.0), Point::new(250.0, 0.0)));
        a.push(ArmatureBone::between("other", Some(0), Point::new(100.0, 0.0), Point::new(100.0, 50.0)));
        let removed = a.remove_subtree(1);
        assert_eq!(removed, vec![1, 2]);
        assert_eq!(a.bones.len(), 2);
        assert_eq!(a.bones[1].name, "other");
        assert_eq!(a.bones[1].parent, Some(0));
    }

    #[test]
    fn skinning_follows_the_nearest_bone() {
        let a = arm();
        let mut p = a.rest_pose();
        p.set_angle(1, FRAC_PI_2);
        let m = a.motions(&p);
        let near_lower = a.skin_point(Point::new(190.0, 0.0), &m);
        assert!(near_lower.y > 5.0, "{near_lower:?}");
        let near_root = a.skin_point(Point::new(5.0, 0.0), &m);
        assert!(near_root.distance(Point::new(5.0, 0.0)) < 1.0, "{near_root:?}");
    }

    #[test]
    fn pose_lerp_blends_angles_and_offset() {
        let a = Pose { offset: Vec2::ZERO, angles: vec![0.0, 1.0] };
        let b = Pose { offset: Vec2::new(10.0, 0.0), angles: vec![2.0] };
        let m = a.lerp(&b, 0.5);
        assert!((m.angle(0) - 1.0).abs() < 1.0e-12);
        assert!((m.angle(1) - 0.5).abs() < 1.0e-12);
        assert!((m.offset.x - 5.0).abs() < 1.0e-12);
    }
}
