//! The integration tests of `anim-text`, built as one test binary (one link instead of nine). Each module is a theme;
//! shared helpers live in `common`.

mod common;

mod display;
mod editing;
mod fallback;
mod font_names;
mod layout_rules;
mod robust;
mod shaping;
mod tools;
mod visual;
