//! Visual verification of shapes and rendering: hard synthetic fixtures, each with a prediction written down in
//! `tests/fixtures/visual/<name>.expect.md` before the first render. The assertions here are the checked
//! predictions; the PNGs written to `/mnt/data/anim/scratch/visual/shape-render/` (and zoomed crops) are what a person
//! looks at, and the small goldens in `tests/fixtures/visual/` catch any later drift.

// Integration tests may panic and use plain arithmetic; the no-panic rules are for shipped code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::unreachable,
    clippy::manual_is_multiple_of,
    clippy::chunks_exact_to_as_chunks,
    clippy::unseparated_literal_suffix,
    clippy::cognitive_complexity,
    clippy::items_after_statements,
    clippy::needless_pass_by_value
)]

#[path = "../golden/common.rs"]
mod common;
mod kit;

mod bitmaps;
mod blends;
mod colors;
mod curves;
mod fills;
mod gradients;
mod masks;
mod meshes;
mod strokes;
