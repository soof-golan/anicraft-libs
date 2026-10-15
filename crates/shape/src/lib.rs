//! Animate's edge-based vector shapes.
//!
//! An Animate shape is a planar set of *edges*: runs of lines and curves, each saying which fill lies on its left
//! (`fill_style0`) and right (`fill_style1`) and whether it is stroked. This crate owns that model.
//!
//! Edge coordinates are twips (1/20 px) everywhere in the model and in the encoded text; everything geometric that
//! this crate returns (paths, bounds, hit tests) is in pixels.

pub mod codec;
pub mod error;
pub mod number;
pub mod segment;

pub use codec::{CubicRun, encode_cubic, encode_edges, parse_cubics, parse_edges};
pub use error::{ParseError, ParseErrorKind, ShapeError};
pub use segment::{Segment, TWIPS_PER_PX};
