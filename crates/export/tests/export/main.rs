//! The integration tests of `anim-export`, built as one test binary (one link instead of four). Each module is a
//! theme; shared helpers live in `common`.

mod common;

mod bitmap;
mod svg;
mod visual;
mod visual_bitmap;
