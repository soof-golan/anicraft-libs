//! Layered artwork import into a neutral tree, for puppets (Character Animator) and Animate import.
//!
//! Three sources feed one [`ArtworkTree`]:
//!
//! - **PSD / PSB** ([`import_psd`]) through `photocraft-psd`: groups, exact layer names, visibility, opacity,
//!   blend modes, clipping and layer masks, with pixels decoded to premultiplied RGBA.
//! - **Illustrator / PDF** ([`import_ai`]) through `VectorCraft`'s importers: the editor's own layer, sublayer and
//!   group hierarchy (read from the private data a PDF-compatible `.ai` carries), PDF optional content groups
//!   for plain PDFs, vector paths with fills, strokes and gradients.
//! - **SVG / SVGZ** ([`import_svg`]) through `usvg`: groups with their ids as names.
//!
//! [`import`] sniffs the format. [`to_display`] turns a tree into a [`anim_display::DisplayList`] for previews and
//! rendering; [`node_to_display`] does the same for one subtree (one puppet layer).
//!
//! Every reader treats its input as hostile: [`Limits`] bound input size, nesting depth, node count and decoded
//! pixels, and nothing panics.

// Tests build pixels and expected values with plain arithmetic; the no-overflow rule is for shipped code.
#![cfg_attr(test, allow(clippy::arithmetic_side_effects))]

mod ai;
mod display;
mod psd;
mod raster;
mod svg;
mod tree;
mod vmask;

pub use ai::import_ai;
pub use display::{node_to_display, to_display};
pub use psd::import_psd;
pub use svg::import_svg;
pub use tree::{ArtKind, ArtMask, ArtNode, ArtworkTree};

/// Resource caps applied by every importer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest input accepted, in bytes.
    pub max_input_bytes: usize,
    /// Deepest group nesting kept; deeper content is hoisted or dropped with a warning.
    pub max_depth: usize,
    /// Most nodes in a tree.
    pub max_nodes: usize,
    /// Most pixels decoded for one raster layer.
    pub max_layer_pixels: u64,
    /// Most pixels decoded for all raster layers together.
    pub max_total_pixels: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_input_bytes: 1 << 30, max_depth: 64, max_nodes: 250_000, max_layer_pixels: 100_000_000, max_total_pixels: 400_000_000 }
    }
}

/// The formats [`import`] recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Photoshop document or large document (`8BPS`).
    Psd,
    /// Illustrator file: PDF-compatible, or legacy PostScript.
    Ai,
    /// SVG, plain or gzip-compressed.
    Svg,
}

/// Why an import failed.
#[derive(Debug, thiserror::Error)]
pub enum ArtworkError {
    #[error("not a PSD, AI/PDF or SVG file")]
    Unrecognized,
    #[error("PSD: {0}")]
    Psd(String),
    #[error("AI/PDF: {0}")]
    Pdf(String),
    #[error("SVG: {0}")]
    Svg(String),
    #[error("input exceeds the limit on {0}")]
    Limit(&'static str),
    #[error("reading the file: {0}")]
    Io(#[from] std::io::Error),
}

/// Identifies the format from the first bytes.
#[must_use]
pub fn detect(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"8BPS") {
        return Some(Format::Psd);
    }
    // PDFs may have junk before the header, within the first KiB.
    let head = bytes.get(..bytes.len().min(1024)).unwrap_or_default();
    if head.windows(5).any(|w| w == b"%PDF-") || head.starts_with(b"%!PS") || head.starts_with(b"\xC5\xD0\xD3\xC6") {
        return Some(Format::Ai);
    }
    if bytes.starts_with(&[0x1F, 0x8B]) {
        return Some(Format::Svg);
    }
    let text = bytes.get(..bytes.len().min(4096)).unwrap_or_default();
    let lossy = String::from_utf8_lossy(text);
    (lossy.contains("<svg") || lossy.contains("<!DOCTYPE svg")).then_some(Format::Svg)
}

/// Imports a PSD/PSB, AI/PDF or SVG, whichever `bytes` is.
///
/// # Errors
/// [`ArtworkError::Unrecognized`] for other data, otherwise the format importer's error.
pub fn import(bytes: &[u8], limits: &Limits) -> Result<ArtworkTree, ArtworkError> {
    match detect(bytes) {
        Some(Format::Psd) => import_psd(bytes, limits),
        Some(Format::Ai) => import_ai(bytes, limits),
        Some(Format::Svg) => import_svg(bytes, limits),
        None => Err(ArtworkError::Unrecognized),
    }
}

/// Reads and imports a file.
///
/// # Errors
/// [`ArtworkError::Io`] if it cannot be read, otherwise as [`import`].
pub fn import_path(path: &std::path::Path, limits: &Limits) -> Result<ArtworkTree, ArtworkError> {
    let len = std::fs::metadata(path)?.len();
    if u64::try_from(limits.max_input_bytes).is_ok_and(|max| len > max) {
        return Err(ArtworkError::Limit("input bytes"));
    }
    import(&std::fs::read(path)?, limits)
}

/// Checks the input size against the limits.
pub(crate) const fn check_input(bytes: &[u8], limits: &Limits) -> Result<(), ArtworkError> {
    if bytes.len() > limits.max_input_bytes {
        return Err(ArtworkError::Limit("input bytes"));
    }
    Ok(())
}
