//! Deterministic CPU rasteriser for [`anim_display::DisplayList`]s.
//!
//! [`render`] turns a display list and its [`ImageStore`] into a premultiplied RGBA8 [`Image`]; a [`Renderer`]
//! does the same while keeping its scratch buffers between frames (animation playback, export, previews).
//!
//! What is drawn, and how:
//!
//! - **Fills and strokes** through tiny-skia: non-zero and even-odd rules, caps, joins, miter limit, dashes, and
//!   Animate's stroke-scale modes including hairlines (see [`anim_display::StrokeScale`]).
//! - **Paints**: solid colours, Flash gradients (linear over -819.2..=819.2 on x, radial with a focal point, pad /
//!   reflect / repeat, optional linear-RGB interpolation) mapped through the gradient matrix, and bitmap fills
//!   (repeat or clamp, smoothed or nearest).
//! - **Groups** with a transform draw straight onto their parent. A group with a colour transform, a blend mode
//!   other than normal/pass-through, a mask or filters is rendered into a bounds-limited offscreen layer first:
//!   children, then mask, then filters, then colour transform, then composite with the blend mode. Flash's
//!   `Subtract`, `Invert`, `Alpha` and `Erase` are implemented here; the rest map onto tiny-skia's blend modes.
//! - **Filters**: blur (box passes, as Flash), drop shadow, glow (outer, inner, knockout, hide-object) and colour
//!   matrix.
//! - **Bitmaps** with smoothing on or off, and **meshes**: content rendered once into a texture and drawn through
//!   per-triangle affine maps.
//!
//! The output depends only on the inputs: parallel work (blur rows, colour transforms) is split into independent
//! units, and nothing reads a clock or a random source. tiny-skia's SIMD paths can differ in the last bit between
//! CPU architectures, so cross-platform comparisons should use a small tolerance.
//!
//! Hostile input never panics or allocates without bound: output size and offscreen memory are capped, nesting is
//! limited to [`MAX_DEPTH`], non-finite numbers and degenerate matrices skip the node, and every skip is counted
//! in [`RenderStats`].

// Tests build pixels and expected values with plain arithmetic; the no-overflow rule is for shipped code.
#![cfg_attr(test, allow(clippy::arithmetic_side_effects))]

mod bounds;
mod composite;
mod convert;
mod draw;
mod error;
mod filters;
mod group;
mod mesh;
mod options;
mod paint;
mod par;
mod pixel;
mod pool;
mod px;
mod renderer;
mod stats;
mod stroke;

pub use bounds::MAX_DEPTH;
pub use error::RenderError;
pub use options::{Background, RenderOptions};
pub use renderer::{MAX_DIMENSION, MAX_PIXELS, Renderer, content_bounds, render};
pub use stats::RenderStats;
