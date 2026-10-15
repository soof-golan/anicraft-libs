//! Counters describing what the last render did.

use serde::{Deserialize, Serialize};

/// What a render did. Content problems never fail a render: the node is skipped and counted here, so tools (and
/// tests) can tell "drew nothing" from "could not draw".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderStats {
    /// Fills, strokes, bitmaps and meshes that were rasterised.
    pub drawn: usize,
    /// Nodes skipped because they lie entirely outside the visible area.
    pub culled: usize,
    /// Groups drawn straight onto their parent (no offscreen layer).
    pub direct_groups: usize,
    /// Groups rendered into an offscreen layer first (colour transform, blend mode, mask or filters).
    pub isolated_groups: usize,
    /// Bitmap references that are not in the image store.
    pub missing_images: usize,
    /// Nodes skipped because of non-finite numbers, degenerate matrices or styles, or nesting deeper than
    /// [`crate::MAX_DEPTH`].
    pub invalid: usize,
    /// Groups drawn without their effects because an offscreen layer could not be allocated.
    pub degraded_groups: usize,
    /// Most bytes held by offscreen layers (excluding the output) at any moment.
    pub peak_layer_bytes: usize,
    /// Mip levels built for bitmaps drawn small (0 on a repeat render when the image cache is kept).
    pub mips_built: usize,
    /// Bytes of mip levels held by the renderer after this render.
    pub mip_cache_bytes: usize,
}

impl RenderStats {
    /// Whether every node was either drawn or legitimately off screen.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.missing_images == 0 && self.invalid == 0 && self.degraded_groups == 0
    }
}
