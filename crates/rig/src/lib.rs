//! Deformation for puppets and asset warp.
//!
//! - [`mesh`]: outline polygons (outer ring plus holes) to a triangle [`Mesh`] with interior points, built with a
//!   constrained Delaunay triangulation and a maximum edge length.
//! - [`handles`]: binding arbitrary points to a mesh (nearest vertex or barycentric) and turning handle targets into
//!   vertex pins.
//! - [`arap`]: as-rigid-as-possible deformation with pinned vertices ([`ArapSolver`]).
//! - [`bones`]: bone [`Chain`]s with forward kinematics, analytic two-bone IK, FABRIK and angle limits.
//! - [`dangle`]: damped-spring [`Dangle`] chains with gravity and wind, integrated at a fixed [`Tick`] timestep.
//!
//! Everything is deterministic: the same inputs give bit-identical outputs on every run. Non-finite inputs never
//! panic; they are ignored or replaced by the rest pose.

#![allow(clippy::many_single_char_names, reason = "numerical code follows the papers' notation (a, b, c, p, q, r, ...)")]
#![allow(clippy::suboptimal_flops, reason = "no fused multiply-add: results must be bit-identical on every CPU")]
#![cfg_attr(test, allow(clippy::as_conversions, reason = "tests index vectors with vertex ids"))]

pub mod arap;
pub mod bones;
pub mod dangle;
pub mod handles;
pub mod mesh;

pub use arap::{ArapError, ArapOptions, ArapSolver, WeightKind};
pub use bones::{AngleLimits, Bend, Bone, Chain, IkOptions, IkResult, TwoBoneSolution, two_bone_ik};
pub use dangle::{DANGLE_STEP, Dangle, DangleParams};
pub use handles::{Binding, BindingMode, HandleSet};
pub use mesh::{Mesh, MeshError, MeshOptions, Outline};

pub use anim_core::{Point, Tick, Vec2};
