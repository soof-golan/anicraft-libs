//! Animate's edge-based vector shapes.
//!
//! An Animate shape is a planar set of *edges*: runs of lines and curves, each saying which fill lies on its left
//! (`fill_style0`) and right (`fill_style1`) and whether it is stroked. This crate owns that model.
//!
//! Edge coordinates are twips (1/20 px) everywhere in the model and in the encoded text; everything geometric that
//! this crate returns (paths, bounds, hit tests) is in pixels.

pub mod assemble;
pub mod codec;
pub mod display;
pub mod edges;
pub mod error;
pub mod geometry;
pub mod number;
pub mod segment;
pub mod styles;

pub use anim_display::{Cap, GradientStop, Interpolation, Join, Spread, StrokeScale};
pub use assemble::{Assembled, FillRegion, StrokeRun};
pub use codec::{CubicRun, encode_cubic, encode_edges, parse_cubics, parse_edges};
pub use display::{ImageResolver, NoImages};
pub use edges::{Edge, EdgeShape, XflEdge};
pub use error::{ParseError, ParseErrorKind, ShapeError};
pub use geometry::Hit;
pub use segment::{Segment, TWIPS_PER_PX};
pub use styles::{BITMAP_MATRIX_SCALE, BitmapFill, FillStyle, GradientFill, OtherStroke, SolidStroke, StrokeKind, StrokeStyle, XflName};
