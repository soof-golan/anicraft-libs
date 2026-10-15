//! Damped-spring "dangle" chains (hair, tails, antennae) integrated at a fixed timestep.
//!
//! Each particle is pulled toward `parent + rest_offset` by a damped spring (the parent is the anchor for the first
//! particle and the previous particle after that), plus gravity and wind accelerations. Time arrives as
//! [`Tick`] deltas and is consumed in whole [`DANGLE_STEP`] steps with the remainder carried over, so the result
//! depends only on the total elapsed time and the anchor path, never on how the caller chunks frames.

use anim_core::{Point, TICKS_PER_SECOND, Tick, Vec2, num};
use serde::{Deserialize, Serialize};

/// The fixed integration step: 1/240 s.
pub const DANGLE_STEP: Tick = Tick(TICKS_PER_SECOND / 240);
/// Largest time a single `step` call simulates (longer gaps are dropped, which avoids spirals of death).
const MAX_STEP_TIME: Tick = Tick(TICKS_PER_SECOND / 4);
/// Spring and damping rates are clamped to keep semi-implicit Euler stable at [`DANGLE_STEP`].
const MAX_STIFFNESS: f64 = 100_000.0;
const MAX_DAMPING: f64 = 200.0;
const MAX_SPEED: f64 = 1.0e7;

/// Physical parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DangleParams {
    /// Spring rate in 1/s^2 (acceleration per unit of displacement from the rest target).
    pub stiffness: f64,
    /// Velocity damping in 1/s.
    pub damping: f64,
    /// Constant acceleration in stage units per s^2 (y is down on the stage, so gravity is positive y).
    pub gravity: Vec2,
    /// Constant wind acceleration in stage units per s^2.
    pub wind: Vec2,
    /// Project every particle back to its rest distance from its parent after each step (rigid segments).
    pub keep_lengths: bool,
}

impl Default for DangleParams {
    fn default() -> Self {
        Self { stiffness: 120.0, damping: 6.0, gravity: Vec2::ZERO, wind: Vec2::ZERO, keep_lengths: false }
    }
}

/// A chain of spring-driven particles hanging from an anchor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dangle {
    pub params: DangleParams,
    offsets: Vec<Vec2>,
    pos: Vec<Point>,
    vel: Vec<Vec2>,
    anchor: Point,
    accumulator: Tick,
}

impl Dangle {
    /// Creates a chain at rest: particle `i` sits at `anchor + offsets[0] + ... + offsets[i]`.
    #[must_use]
    pub fn new(anchor: Point, rest_offsets: Vec<Vec2>, params: DangleParams) -> Self {
        let offsets: Vec<Vec2> = rest_offsets.into_iter().map(|o| if o.x.is_finite() && o.y.is_finite() { o } else { Vec2::ZERO }).collect();
        let mut p = anchor;
        let pos = offsets
            .iter()
            .map(|&o| {
                p += o;
                p
            })
            .collect();
        let vel = vec![Vec2::ZERO; offsets.len()];
        Self { params, offsets, pos, vel, anchor, accumulator: Tick(0) }
    }

    /// Current particle positions.
    #[must_use]
    pub fn positions(&self) -> &[Point] {
        &self.pos
    }

    /// Current particle velocities.
    #[must_use]
    pub fn velocities(&self) -> &[Vec2] {
        &self.vel
    }

    /// The anchor used by the last step.
    #[must_use]
    pub const fn anchor(&self) -> Point {
        self.anchor
    }

    /// Largest particle speed.
    #[must_use]
    pub fn max_speed(&self) -> f64 {
        self.vel.iter().map(|v| v.hypot()).fold(0.0, f64::max)
    }

    /// Returns to the rest pose at `anchor` with zero velocity.
    pub fn reset(&mut self, anchor: Point) {
        *self = Self::new(anchor, std::mem::take(&mut self.offsets), self.params);
    }

    /// Advances by `dt` with the anchor at `anchor` (a non-finite anchor keeps the previous one). Returns the
    /// number of fixed steps taken.
    pub fn step(&mut self, anchor: Point, dt: Tick) -> u32 {
        if anchor.x.is_finite() && anchor.y.is_finite() {
            self.anchor = anchor;
        }
        let dt = dt.max(Tick(0)).min(MAX_STEP_TIME);
        self.accumulator = Tick(self.accumulator.0.saturating_add(dt.0)).min(MAX_STEP_TIME);
        let mut steps = 0;
        while self.accumulator.0 >= DANGLE_STEP.0 {
            self.accumulator = Tick(self.accumulator.0.saturating_sub(DANGLE_STEP.0));
            self.substep();
            steps = u32::saturating_add(steps, 1);
        }
        steps
    }

    fn substep(&mut self) {
        let dt = DANGLE_STEP.seconds();
        let k = self.params.stiffness.clamp(0.0, MAX_STIFFNESS);
        let c = self.params.damping.clamp(0.0, MAX_DAMPING);
        let g = finite_or_zero(self.params.gravity) + finite_or_zero(self.params.wind);
        let before = self.pos.clone();
        let mut parent_old = self.anchor;
        let mut parent_new = self.anchor;
        for (i, off) in self.offsets.iter().enumerate() {
            let (Some(p), Some(v), Some(&old)) = (self.pos.get_mut(i), self.vel.get_mut(i), before.get(i)) else { continue };
            let target = parent_old + *off;
            let a = (target - old) * k - *v * c + g;
            *v += a * dt;
            let speed = v.hypot();
            if speed > MAX_SPEED {
                *v *= MAX_SPEED / speed;
            }
            *p = old + *v * dt;
            if self.params.keep_lengths {
                let len = off.hypot();
                let d = *p - parent_new;
                let dl = d.hypot();
                if dl > 1.0e-12 {
                    *p = parent_new + d * (len / dl);
                } else {
                    *p = parent_new + *off;
                }
                *v = (*p - old) / dt;
            }
            parent_old = old;
            parent_new = *p;
        }
    }

    /// Number of particles as `u32` (saturating).
    #[must_use]
    pub fn len_u32(&self) -> u32 {
        num::usize_to_u32_sat(self.pos.len())
    }
}

const fn finite_or_zero(v: Vec2) -> Vec2 {
    if v.x.is_finite() && v.y.is_finite() { v } else { Vec2::ZERO }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(params: DangleParams) -> Dangle {
        Dangle::new(Point::new(0.0, 0.0), vec![Vec2::new(0.0, 10.0); 4], params)
    }

    fn run(d: &mut Dangle, seconds: f64, frame: f64) {
        let dt = Tick::from_seconds_f64(frame);
        let n = num::round_f64_to_u32(seconds / frame);
        for _ in 0..n {
            d.step(Point::ZERO, dt);
        }
    }

    #[test]
    fn rest_pose_is_stable_without_forces() {
        let mut d = chain(DangleParams::default());
        let before = d.positions().to_vec();
        run(&mut d, 2.0, 1.0 / 60.0);
        assert_eq!(d.positions(), before.as_slice());
    }

    #[test]
    fn displaced_particles_settle_back() {
        let mut d = chain(DangleParams { stiffness: 200.0, damping: 12.0, ..DangleParams::default() });
        let rest = d.positions().to_vec();
        d.pos[3] += Vec2::new(30.0, -20.0);
        run(&mut d, 8.0, 1.0 / 60.0);
        assert!(d.max_speed() < 1e-3, "{}", d.max_speed());
        for (p, r) in d.positions().iter().zip(&rest) {
            assert!(p.distance(*r) < 1e-2, "{p:?} vs {r:?}");
        }
    }

    #[test]
    fn gravity_and_wind_reach_equilibrium() {
        let params = DangleParams { gravity: Vec2::new(0.0, 500.0), wind: Vec2::new(150.0, 0.0), ..DangleParams::default() };
        let mut d = chain(params);
        run(&mut d, 15.0, 1.0 / 60.0);
        assert!(d.max_speed() < 1e-3, "{}", d.max_speed());
        let tip = d.positions()[3];
        let rest_tip = Point::new(0.0, 40.0);
        assert!(tip.x > rest_tip.x + 1.0, "wind blows +x: {tip:?}");
        assert!(tip.y > rest_tip.y + 1.0, "gravity pulls +y: {tip:?}");
        // Settling is stable: another second changes nothing.
        let settled = d.positions().to_vec();
        run(&mut d, 1.0, 1.0 / 60.0);
        for (a, b) in settled.iter().zip(d.positions()) {
            assert!(a.distance(*b) < 1e-3);
        }
    }

    #[test]
    fn frame_chunking_does_not_change_the_result() {
        let params = DangleParams { gravity: Vec2::new(0.0, 300.0), ..DangleParams::default() };
        let mut a = chain(params);
        let mut b = chain(params);
        run(&mut a, 1.0, 1.0 / 60.0);
        run(&mut b, 1.0, 1.0 / 30.0);
        assert_eq!(a.positions(), b.positions());
        let mut c = chain(params);
        run(&mut c, 1.0, 1.0 / 60.0);
        assert_eq!(a, c, "deterministic");
    }

    #[test]
    fn keep_lengths_holds_segments_rigid() {
        let params = DangleParams { gravity: Vec2::new(0.0, 900.0), wind: Vec2::new(400.0, 0.0), keep_lengths: true, ..DangleParams::default() };
        let mut d = chain(params);
        for i in 0..600 {
            d.step(Point::new(f64::from(i % 50), 0.0), Tick::from_seconds_f64(1.0 / 60.0));
            let mut prev = d.anchor();
            for p in d.positions() {
                assert!((prev.distance(*p) - 10.0).abs() < 1e-6);
                prev = *p;
            }
        }
    }

    #[test]
    fn extreme_parameters_stay_finite() {
        let params =
            DangleParams { stiffness: 1.0e12, damping: 1.0e12, gravity: Vec2::new(1.0e9, f64::NAN), wind: Vec2::new(f64::INFINITY, 0.0), keep_lengths: false };
        let mut d = chain(params);
        run(&mut d, 3.0, 1.0 / 30.0);
        assert!(d.positions().iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        // Huge frame gaps and bad anchors are survivable.
        d.step(Point::new(f64::NAN, 0.0), Tick(i64::MAX));
        d.step(Point::ZERO, Tick(i64::MIN));
        assert!(d.positions().iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    }

    #[test]
    fn reset_restores_rest_pose() {
        let mut d = chain(DangleParams { gravity: Vec2::new(0.0, 100.0), ..DangleParams::default() });
        run(&mut d, 1.0, 1.0 / 60.0);
        d.reset(Point::new(5.0, 5.0));
        assert_eq!(d.positions()[0], Point::new(5.0, 15.0));
        assert_eq!(d.max_speed(), 0.0);
    }
}
