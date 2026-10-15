//! Bone chains: forward kinematics, analytic two-bone IK, FABRIK for longer chains, angle limits.
//!
//! Angles are radians, counter-clockwise in a y-up frame (so clockwise on a y-down stage); the maths is symmetric,
//! callers just keep one convention. A bone's `angle` is relative to its parent (the chain's `root_angle` for the
//! first bone) and always normalised to `[-pi, pi]`.

use std::f64::consts::{PI, TAU};

use anim_core::{Point, Vec2, num};
use serde::{Deserialize, Serialize};

/// Inclusive range for a bone's relative angle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AngleLimits {
    pub min: f64,
    pub max: f64,
}

impl AngleLimits {
    /// Clamps a (normalised) angle into the range. A reversed range is treated as `[max, min]`.
    #[must_use]
    pub fn clamp(self, angle: f64) -> f64 {
        let (lo, hi) = if self.min <= self.max { (self.min, self.max) } else { (self.max, self.min) };
        normalize_angle(angle).clamp(lo, hi)
    }
}

/// One bone: a length and an angle relative to its parent.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    pub length: f64,
    pub angle: f64,
    pub limits: Option<AngleLimits>,
}

impl Bone {
    #[must_use]
    pub const fn new(length: f64) -> Self {
        Self { length, angle: 0.0, limits: None }
    }

    #[must_use]
    pub const fn with_limits(mut self, min: f64, max: f64) -> Self {
        self.limits = Some(AngleLimits { min, max });
        self
    }
}

/// Which way a two-bone chain bends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bend {
    /// The middle joint ends up on the left of the root-to-target line (counter-clockwise first rotation).
    #[default]
    Positive,
    /// The middle joint ends up on the right.
    Negative,
}

/// Result of [`two_bone_ik`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwoBoneSolution {
    /// Absolute angle of the first bone.
    pub angle1: f64,
    /// Angle of the second bone relative to the first.
    pub angle2: f64,
    /// Whether the target is within reach (otherwise the chain is stretched or folded as close as possible).
    pub reachable: bool,
}

/// Normalises an angle to `[-pi, pi]`; non-finite becomes 0.
#[must_use]
pub fn normalize_angle(a: f64) -> f64 {
    if !a.is_finite() {
        return 0.0;
    }
    let r = a - TAU * (a / TAU).round();
    r.clamp(-PI, PI)
}

fn dir(angle: f64) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c, s)
}

/// Analytic IK for a root, two bones of lengths `l1`, `l2`, and a target given relative to the root.
#[must_use]
pub fn two_bone_ik(l1: f64, l2: f64, target: Vec2, bend: Bend) -> TwoBoneSolution {
    let l1 = if l1.is_finite() { l1.max(0.0) } else { 0.0 };
    let l2 = if l2.is_finite() { l2.max(0.0) } else { 0.0 };
    if !target.x.is_finite() || !target.y.is_finite() {
        return TwoBoneSolution { angle1: 0.0, angle2: 0.0, reachable: false };
    }
    let d = target.hypot();
    let theta = target.y.atan2(target.x);
    let reachable = d <= l1 + l2 + 1.0e-12 && d >= (l1 - l2).abs() - 1.0e-12;
    if d < 1.0e-12 || l1 < 1.0e-12 {
        // Root on target (or a zero-length first bone): fold the second bone toward the target as best it can.
        let angle1 = theta;
        let elbow = dir(angle1) * l1;
        let rest = target - elbow;
        let abs2 = rest.y.atan2(rest.x);
        return TwoBoneSolution { angle1, angle2: normalize_angle(abs2 - angle1), reachable };
    }
    let cos_a = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0);
    let alpha = cos_a.acos();
    let angle1 = match bend {
        Bend::Positive => theta + alpha,
        Bend::Negative => theta - alpha,
    };
    let elbow = dir(angle1) * l1;
    let rest = target - elbow;
    let abs2 = if rest.hypot2() < 1.0e-24 { angle1 } else { rest.y.atan2(rest.x) };
    TwoBoneSolution { angle1: normalize_angle(angle1), angle2: normalize_angle(abs2 - angle1), reachable }
}

/// IK tuning.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IkOptions {
    /// FABRIK iteration cap.
    pub max_iterations: u32,
    /// Stop when the tip is this close to the target.
    pub tolerance: f64,
}

impl Default for IkOptions {
    fn default() -> Self {
        Self { max_iterations: 64, tolerance: 1.0e-4 }
    }
}

/// What an IK solve achieved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IkResult {
    /// The tip is within tolerance of the target.
    pub reached: bool,
    /// Remaining distance from tip to target.
    pub error: f64,
    /// FABRIK iterations used (0 for the analytic solver).
    pub iterations: u32,
}

/// A chain of bones from a root point.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    pub root: Point,
    /// Orientation of the parent frame the first bone is relative to.
    pub root_angle: f64,
    pub bones: Vec<Bone>,
}

impl Chain {
    #[must_use]
    pub const fn new(root: Point, bones: Vec<Bone>) -> Self {
        Self { root, root_angle: 0.0, bones }
    }

    /// Sum of bone lengths.
    #[must_use]
    pub fn reach(&self) -> f64 {
        self.bones.iter().map(|b| b.length.max(0.0)).sum()
    }

    /// Absolute angle of every bone.
    #[must_use]
    pub fn world_angles(&self) -> Vec<f64> {
        let mut acc = self.root_angle;
        self.bones
            .iter()
            .map(|b| {
                acc += b.angle;
                acc
            })
            .collect()
    }

    /// Forward kinematics: `bones.len() + 1` joint positions, root first.
    #[must_use]
    pub fn joints(&self) -> Vec<Point> {
        let mut p = self.root;
        let mut out = vec![p];
        for (b, a) in self.bones.iter().zip(self.world_angles()) {
            p += dir(a) * b.length;
            out.push(p);
        }
        out
    }

    /// End effector position.
    #[must_use]
    pub fn tip(&self) -> Point {
        self.joints().last().copied().unwrap_or(self.root)
    }

    /// Clamps every bone angle into its limits (normalising the others).
    pub fn apply_limits(&mut self) {
        for b in &mut self.bones {
            b.angle = b.limits.map_or_else(|| normalize_angle(b.angle), |l| l.clamp(b.angle));
        }
    }

    /// Sets bone angles so the chain passes through `joints` (as FK would produce them). Zero-length segments keep
    /// their current absolute direction.
    fn set_from_joints(&mut self, joints: &[Point]) {
        let mut parent = self.root_angle;
        let mut world = self.root_angle;
        for (i, b) in self.bones.iter_mut().enumerate() {
            world += b.angle; // the current absolute angle, used as the fallback
            if let (Some(&a), Some(&c)) = (joints.get(i), joints.get(i.saturating_add(1))) {
                let v = c - a;
                if v.hypot2() > 1.0e-24 {
                    world = v.y.atan2(v.x);
                }
            }
            b.angle = normalize_angle(world - parent);
            parent = world;
        }
    }

    /// Moves the tip toward `target`, respecting bone lengths and angle limits. Two-bone chains use the analytic
    /// solution (keeping the current bend side, or the other side when only it satisfies the limits better); longer
    /// chains use FABRIK with the limits applied after every pass. Unreachable targets stretch the chain toward them.
    pub fn solve_ik(&mut self, target: Point, options: &IkOptions) -> IkResult {
        if !target.x.is_finite() || !target.y.is_finite() || self.bones.is_empty() {
            return IkResult { reached: false, error: f64::INFINITY, iterations: 0 };
        }
        self.apply_limits();
        match self.bones.len() {
            2 => self.solve_two_bone(target, options),
            _ => self.solve_fabrik(target, options),
        }
    }

    fn result(&self, target: Point, options: &IkOptions, iterations: u32) -> IkResult {
        let error = self.tip().distance(target);
        IkResult { reached: error <= options.tolerance.max(1.0e-9), error, iterations }
    }

    fn solve_two_bone(&mut self, target: Point, options: &IkOptions) -> IkResult {
        let (Some(&b1), Some(&b2)) = (self.bones.first(), self.bones.get(1)) else {
            return self.result(target, options, 0);
        };
        let preferred = if b2.angle >= 0.0 { Bend::Positive } else { Bend::Negative };
        let other = if b2.angle >= 0.0 { Bend::Negative } else { Bend::Positive };
        let rel = target - self.root;
        let mut best: Option<(f64, [f64; 2])> = None;
        for bend in [preferred, other] {
            let s = two_bone_ik(b1.length, b2.length, rel, bend);
            let a1 = normalize_angle(s.angle1 - self.root_angle);
            let a1 = b1.limits.map_or(a1, |l| l.clamp(a1));
            let a2 = b2.limits.map_or(s.angle2, |l| l.clamp(s.angle2));
            let probe = Self { root: self.root, root_angle: self.root_angle, bones: vec![Bone { angle: a1, ..b1 }, Bone { angle: a2, ..b2 }] };
            let err = probe.tip().distance(target);
            // Prefer the current bend unless the other is clearly better.
            if best.is_none_or(|(e, _)| err + 1.0e-9 < e) {
                best = Some((err, [a1, a2]));
            }
        }
        if let Some((_, [a1, a2])) = best {
            if let Some(b) = self.bones.get_mut(0) {
                b.angle = a1;
            }
            if let Some(b) = self.bones.get_mut(1) {
                b.angle = a2;
            }
        }
        self.result(target, options, 0)
    }

    fn solve_fabrik(&mut self, target: Point, options: &IkOptions) -> IkResult {
        let lengths: Vec<f64> = self.bones.iter().map(|b| if b.length.is_finite() { b.length.max(0.0) } else { 0.0 }).collect();
        let mut used = 0;
        let mut previous = f64::INFINITY;
        for it in 0..options.max_iterations {
            let error = self.tip().distance(target);
            if error <= options.tolerance {
                break;
            }
            if previous - error < 1.0e-12 {
                if self.root.distance(target) >= self.reach() {
                    break; // fully stretched toward an unreachable target: nothing more to gain
                }
                // Stagnation, typically a perfectly straight chain whose collinear folds cannot reach the target:
                // bend it in a zigzag so the next pass has a plane to work in.
                for (i, b) in self.bones.iter_mut().enumerate() {
                    let nudge = if i % 2 == 0 { 0.1 } else { -0.1 };
                    b.angle = b.limits.map_or_else(|| normalize_angle(b.angle + nudge), |l| l.clamp(b.angle + nudge));
                }
            }
            previous = error;
            used = it.saturating_add(1);
            let mut j = self.joints();
            let n = j.len();
            // Backward: tip onto the target, each joint pulled to its bone length from the next.
            if let Some(last) = j.last_mut() {
                *last = target;
            }
            for i in (0..n.saturating_sub(1)).rev() {
                let (Some(&next), Some(&cur), Some(&len)) = (j.get(i.saturating_add(1)), j.get(i), lengths.get(i)) else { continue };
                let v = cur - next;
                let d = if v.hypot2() > 1.0e-24 { v.normalize() } else { Vec2::new(1.0, 0.0) };
                if let Some(slot) = j.get_mut(i) {
                    *slot = next + d * len;
                }
            }
            // Forward: root pinned, each joint pushed out to its bone length.
            if let Some(first) = j.first_mut() {
                *first = self.root;
            }
            for i in 0..n.saturating_sub(1) {
                let (Some(&prev), Some(&cur), Some(&len)) = (j.get(i), j.get(i.saturating_add(1)), lengths.get(i)) else { continue };
                let v = cur - prev;
                let d = if v.hypot2() > 1.0e-24 { v.normalize() } else { Vec2::new(1.0, 0.0) };
                if let Some(slot) = j.get_mut(i.saturating_add(1)) {
                    *slot = prev + d * len;
                }
            }
            self.set_from_joints(&j);
            self.apply_limits();
        }
        self.result(target, options, used)
    }

    /// Number of bones as `u32` (saturating), for callers that index bones by id.
    #[must_use]
    pub fn len_u32(&self) -> u32 {
        num::usize_to_u32_sat(self.bones.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arm(l1: f64, l2: f64) -> Chain {
        Chain::new(Point::new(10.0, 20.0), vec![Bone::new(l1), Bone { angle: 0.3, ..Bone::new(l2) }])
    }

    #[test]
    fn forward_kinematics() {
        let c = Chain::new(Point::ZERO, vec![Bone::new(10.0), Bone { angle: PI / 2.0, ..Bone::new(5.0) }]);
        let j = c.joints();
        assert_eq!(j.len(), 3);
        assert!(j[1].distance(Point::new(10.0, 0.0)) < 1e-12);
        assert!(j[2].distance(Point::new(10.0, 5.0)) < 1e-12);
        assert!((c.reach() - 15.0).abs() < 1e-12);
    }

    #[test]
    fn two_bone_reaches_target_both_bends() {
        for bend in [Bend::Positive, Bend::Negative] {
            let s = two_bone_ik(10.0, 7.0, Vec2::new(9.0, 6.0), bend);
            assert!(s.reachable);
            let e = dir(s.angle1) * 10.0;
            let t = e + dir(s.angle1 + s.angle2) * 7.0;
            assert!((t - Vec2::new(9.0, 6.0)).hypot() < 1e-9);
        }
    }

    #[test]
    fn two_bone_unreachable_stretches() {
        let s = two_bone_ik(10.0, 7.0, Vec2::new(100.0, 0.0), Bend::Positive);
        assert!(!s.reachable);
        assert!(s.angle1.abs() < 1e-9 && s.angle2.abs() < 1e-9);
        let s = two_bone_ik(10.0, 7.0, Vec2::new(0.0, 0.0), Bend::Positive);
        assert!(s.angle1.is_finite() && s.angle2.is_finite());
    }

    #[test]
    fn chain_two_bone_reaches() {
        let mut c = arm(10.0, 8.0);
        let target = Point::new(22.0, 30.0);
        let r = c.solve_ik(target, &IkOptions::default());
        assert!(r.reached, "{r:?}");
        assert!(c.tip().distance(target) < 1e-6);
        let j = c.joints();
        assert!((j[0].distance(j[1]) - 10.0).abs() < 1e-9);
        assert!((j[1].distance(j[2]) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn fabrik_reaches_and_keeps_lengths() {
        let mut c = Chain::new(Point::new(0.0, 0.0), (0..6).map(|i| Bone { angle: 0.1 * f64::from(i), ..Bone::new(5.0 + f64::from(i)) }).collect());
        let target = Point::new(12.0, 14.0);
        let r = c.solve_ik(target, &IkOptions::default());
        assert!(r.reached, "{r:?}");
        let j = c.joints();
        for (i, b) in c.bones.iter().enumerate() {
            assert!((j[i].distance(j[i + 1]) - b.length).abs() < 1e-9);
        }
    }

    #[test]
    fn fabrik_unreachable_points_at_target() {
        let mut c = Chain::new(Point::ZERO, vec![Bone::new(5.0); 4]);
        let r = c.solve_ik(Point::new(0.0, 100.0), &IkOptions::default());
        assert!(!r.reached);
        assert!(c.tip().distance(Point::new(0.0, 20.0)) < 1e-6);
    }

    #[test]
    fn limits_are_respected() {
        let lim = 0.4;
        let mut c = Chain::new(
            Point::ZERO,
            vec![Bone::new(10.0).with_limits(-lim, lim), Bone::new(10.0).with_limits(-lim, lim), Bone::new(10.0).with_limits(-lim, lim)],
        );
        c.solve_ik(Point::new(0.0, 25.0), &IkOptions::default());
        assert!(c.bones.iter().all(|b| b.angle.abs() <= lim + 1e-12));
        let mut two = Chain::new(Point::ZERO, vec![Bone::new(10.0).with_limits(-0.2, 0.2), Bone::new(10.0).with_limits(0.0, 0.5)]);
        two.solve_ik(Point::new(-5.0, 12.0), &IkOptions::default());
        assert!(two.bones[0].angle.abs() <= 0.2 + 1e-12);
        assert!((0.0..=0.5 + 1e-12).contains(&two.bones[1].angle));
    }

    #[test]
    fn non_finite_target_is_a_noop() {
        let mut c = arm(5.0, 5.0);
        let before = c.clone();
        let r = c.solve_ik(Point::new(f64::NAN, 1.0), &IkOptions::default());
        assert!(!r.reached);
        assert_eq!(c, before);
    }

    proptest! {
        #[test]
        fn two_bone_reachable_always_hits(l1 in 1.0f64..50.0, l2 in 1.0f64..50.0, ang in -PI..PI, f in 0.0f64..1.0, neg in any::<bool>()) {
            let lo = (l1 - l2).abs();
            let d = lo + (l1 + l2 - lo) * f;
            let t = Vec2::new(d * ang.cos(), d * ang.sin());
            let s = two_bone_ik(l1, l2, t, if neg { Bend::Negative } else { Bend::Positive });
            let tip = dir(s.angle1) * l1 + dir(s.angle1 + s.angle2) * l2;
            prop_assert!((tip - t).hypot() < 1e-6 * (1.0 + d));
        }

        #[test]
        fn fabrik_reachable_hits(n in 3usize..8, ang in -PI..PI, f in 0.1f64..0.95) {
            let mut c = Chain::new(Point::ZERO, vec![Bone::new(4.0); n]);
            let d = c.reach() * f;
            let target = Point::new(d * ang.cos(), d * ang.sin());
            let r = c.solve_ik(target, &IkOptions { max_iterations: 400, tolerance: 1e-2 });
            prop_assert!(r.reached, "{:?}", r);
        }
    }
}
