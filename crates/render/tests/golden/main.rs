//! Golden tests for `anim-render`: small scenes whose pixels are checked exactly or with a stated tolerance.
//! PNGs of the output are written to `$ANICRAFT_VISUAL_OUT/anim-render/` when that variable is set, so a person can
//! look at them; the assertions run either way.

// Integration tests may panic and use plain arithmetic; the no-panic rules are for shipped code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
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
    clippy::chunks_exact_to_as_chunks
)]

mod common;
mod filters;
mod groups;
mod mesh;
mod paints;
mod perf;
mod perspective;
mod robustness;
mod shapes;
mod showcase;
