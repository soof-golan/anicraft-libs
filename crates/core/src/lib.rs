//! Shared primitives for the `AniCraft` family of crates.
//!
//! - Time is [`Tick`] (re-exported from `filmcraft-time`, 254 016 000 000 ticks per second) plus [`FrameRate`].
//! - Geometry is `kurbo`, re-exported so every crate uses the same version: stage space is y-down pixels.
//! - Colour is [`Rgba8`] (straight alpha, what files store) and [`PremulRgba`] (premultiplied `f32`, what renderers
//!   compute with), [`ColorTransform`] (Flash-style multiply/offset) and [`BlendMode`].
//! - [`num`] holds the only `as` conversions in the code base, saturating and documented, and the pixel rounding rule.
//! - [`hash::Fnv1a`] is the stable 64-bit content hash for ids and fingerprints.

pub mod blend;
pub mod color;
pub mod color_transform;
pub mod hash;
pub mod num;
pub mod projective;

pub use blend::BlendMode;
pub use color::{PremulRgba, Rgba8};
pub use color_transform::ColorTransform;
pub use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};
pub use kurbo::{self, Affine, BezPath, Point, Rect, Size, Vec2};
pub use projective::{Homography, Matrix3D, Perspective, Transform3D};
