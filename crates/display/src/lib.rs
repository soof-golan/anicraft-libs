//! Display lists: one fully resolved frame, ready to rasterise or export.
//!
//! Producers (`anim-timeline` for Animate documents, the `CharCraft` puppet engine for characters, artwork importers
//! for previews) turn their models into a [`DisplayList`]; consumers (`anim-render`, the SVG/SWF/video exporters)
//! never look at the models. Text is already shaped into glyph outlines by the time it gets here.
//!
//! Coordinates are stage pixels, y down. Each node's `transform` maps its local space into its parent's space.
//! Pixels for bitmaps live in an [`ImageStore`] and are referenced by [`ImageId`], so display lists stay small and
//! cheap to clone.

use std::collections::BTreeMap;

use anim_core::{Affine, BezPath, BlendMode, ColorTransform, Point, Rgba8, Size};
use serde::{Deserialize, Serialize};

/// A resolved frame.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DisplayList {
    /// Stage size in pixels.
    pub size: Size,
    /// Stage background, painted under everything (`None` = transparent).
    pub background: Option<Rgba8>,
    /// Bottom-most first (painter's order).
    pub nodes: Vec<Node>,
}

/// One drawable thing or a group of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Group(Group),
    Fill(Fill),
    Stroke(Stroke),
    Bitmap(Bitmap),
    Mesh(Mesh),
}

/// A transformed, colour-transformed, blended group with an optional mask and filters.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Group {
    pub transform: Affine,
    /// A perspective projection of the group's plane (an instance rotated or moved in 3D): applied after
    /// `transform`, it maps the group's content into the parent's space. `None` is the ordinary affine group.
    /// Renderers draw the content flat and warp it; exporters without perspective use
    /// [`Homography::affine_at`](anim_core::Homography::affine_at) at the content's centre.
    #[serde(default)]
    pub projection: Option<anim_core::Homography>,
    pub color: ColorTransform,
    pub blend: BlendMode,
    /// Children are drawn only where the mask's alpha is non-zero (Animate mask layers, puppet clipping).
    /// The mask lives in the same local space as the children.
    pub mask: Option<Vec<Node>>,
    /// Applied to the composited group, in order.
    pub filters: Vec<Filter>,
    /// Bottom-most first.
    pub children: Vec<Node>,
    /// Optional stable identity for render caches and hit testing (an element or puppet-layer id).
    pub key: Option<u64>,
}

/// A filled path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub transform: Affine,
    pub path: BezPath,
    pub rule: FillRule,
    pub paint: Paint,
}

/// A stroked path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub transform: Affine,
    pub path: BezPath,
    pub style: StrokeStyle,
    pub paint: Paint,
}

/// A bitmap drawn with its top-left at the local origin, one image pixel per local unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bitmap {
    pub transform: Affine,
    pub image: ImageId,
    pub smoothing: bool,
}

/// A textured triangle mesh: `content` is rendered in its own local space, then warped so that each source
/// vertex lands on the matching target vertex (puppet deformation, Animate asset warp).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub transform: Affine,
    pub content: Vec<Node>,
    /// Vertex positions in `content`'s local space.
    pub source: Vec<Point>,
    /// Deformed vertex positions in this node's local space (same length as `source`).
    pub target: Vec<Point>,
    /// Indices into `source`/`target`.
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

/// What a fill or stroke is painted with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    Solid(Rgba8),
    /// Gradient along the local x axis from -819.2 to 819.2, mapped through `transform` (Flash convention); see
    /// [`Gradient`].
    Linear(Gradient),
    /// Gradient over the circle of radius 819.2 at the local origin, mapped through `transform`.
    Radial(Gradient),
    /// A bitmap fill; `transform` maps image pixels into the path's local space.
    Bitmap {
        image: ImageId,
        transform: Affine,
        repeat: bool,
        smoothing: bool,
    },
}

/// The extent of Flash/Animate gradient space: gradients span -819.2 ..= 819.2 before their matrix is applied.
pub const GRADIENT_EXTENT: f64 = 819.2;

/// A gradient in Flash's normalised gradient space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    /// Maps gradient space (-819.2 ..= 819.2) into the path's local space.
    pub transform: Affine,
    /// Sorted by `offset` (0.0 ..= 1.0).
    pub stops: Vec<GradientStop>,
    pub spread: Spread,
    pub interpolation: Interpolation,
    /// Radial only: focal point position along the x axis, -1.0 ..= 1.0.
    pub focal: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Rgba8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Spread {
    #[default]
    Pad,
    Reflect,
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Interpolation {
    #[default]
    Rgb,
    LinearRgb,
}

/// Stroke geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    /// Width in local units (before `transform`), or in pixels when `scale` is [`StrokeScale::None`].
    pub width: f64,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    pub scale: StrokeScale,
    /// Dash lengths in local units, alternating on/off (empty = solid).
    pub dashes: Vec<f64>,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self { width: 1.0, cap: Cap::Round, join: Join::Round, miter_limit: 3.0, scale: StrokeScale::Normal, dashes: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Cap {
    None,
    #[default]
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Join {
    Miter,
    #[default]
    Round,
    Bevel,
}

/// How stroke width reacts to transforms (Animate "scale" stroke option).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeScale {
    #[default]
    Normal,
    Horizontal,
    Vertical,
    /// Constant width in device pixels; a width of 0 is a one-pixel hairline.
    None,
}

/// Post-composite filters (Animate filters; a subset of Photoshop layer effects).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    Blur {
        x: f64,
        y: f64,
        quality: u8,
    },
    DropShadow {
        color: Rgba8,
        angle_deg: f64,
        distance: f64,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        inner: bool,
        knockout: bool,
        hide_object: bool,
    },
    Glow {
        color: Rgba8,
        blur_x: f64,
        blur_y: f64,
        strength: f64,
        inner: bool,
        knockout: bool,
    },
    /// Row-major 4x5 matrix applied to straight-alpha RGBA in 0 ..= 255 units (Flash `ColorMatrixFilter`).
    ColorMatrix([f32; 20]),
}

/// Handle to a bitmap in an [`ImageStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImageId(pub u64);

/// An RGBA8 image with premultiplied alpha, rows top to bottom, no padding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes.
    pub premultiplied_rgba: Vec<u8>,
}

/// Errors building images.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    #[error("image of {width}x{height} needs {expected} bytes, got {actual}")]
    SizeMismatch { width: u32, height: u32, expected: usize, actual: usize },
    #[error("image of {width}x{height} is too large")]
    TooLarge { width: u32, height: u32 },
}

impl Image {
    /// Validates the buffer length against the dimensions.
    ///
    /// # Errors
    /// [`ImageError::TooLarge`] if the byte count overflows, [`ImageError::SizeMismatch`] if the buffer is the
    /// wrong length.
    pub fn new(width: u32, height: u32, premultiplied_rgba: Vec<u8>) -> Result<Self, ImageError> {
        let expected = Self::byte_len(width, height).ok_or(ImageError::TooLarge { width, height })?;
        if premultiplied_rgba.len() != expected {
            return Err(ImageError::SizeMismatch { width, height, expected, actual: premultiplied_rgba.len() });
        }
        Ok(Self { width, height, premultiplied_rgba })
    }

    /// `width * height * 4`, or `None` on overflow.
    #[must_use]
    pub fn byte_len(width: u32, height: u32) -> Option<usize> {
        usize::try_from(width).ok()?.checked_mul(usize::try_from(height).ok()?)?.checked_mul(4)
    }
}

/// Owns the pixels that display lists refer to. Ids are allocated in insertion order and never reused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageStore {
    images: BTreeMap<ImageId, Image>,
    next: u64,
}

impl ImageStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `image` and returns its id.
    pub fn insert(&mut self, image: Image) -> ImageId {
        let id = ImageId(self.next);
        self.next = self.next.saturating_add(1);
        self.images.insert(id, image);
        id
    }

    #[must_use]
    pub fn get(&self, id: ImageId) -> Option<&Image> {
        self.images.get(&id)
    }

    pub fn remove(&mut self, id: ImageId) -> Option<Image> {
        self.images.remove(&id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.images.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

impl DisplayList {
    /// An empty list for a stage of `size`.
    #[must_use]
    pub const fn new(size: Size, background: Option<Rgba8>) -> Self {
        Self { size, background, nodes: Vec::new() }
    }

    /// Visits every node depth-first (groups before their children, masks before children, mesh content after
    /// the mesh itself).
    pub fn visit(&self, f: &mut impl FnMut(&Node)) {
        fn walk(nodes: &[Node], f: &mut impl FnMut(&Node)) {
            for node in nodes {
                f(node);
                match node {
                    Node::Group(g) => {
                        if let Some(mask) = &g.mask {
                            walk(mask, f);
                        }
                        walk(&g.children, f);
                    }
                    Node::Mesh(m) => walk(&m.content, f),
                    Node::Fill(_) | Node::Stroke(_) | Node::Bitmap(_) => {}
                }
            }
        }
        walk(&self.nodes, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_validates_length() {
        assert!(Image::new(2, 2, vec![0; 16]).is_ok());
        assert!(matches!(Image::new(2, 2, vec![0; 15]), Err(ImageError::SizeMismatch { .. })));
        assert_eq!(Image::byte_len(u32::MAX, u32::MAX), None);
        assert_eq!(Image::byte_len(3, 5), Some(60));
    }

    #[test]
    fn store_ids_are_stable() {
        let mut store = ImageStore::new();
        let a = store.insert(Image::new(1, 1, vec![0; 4]).unwrap());
        let b = store.insert(Image::new(1, 1, vec![255; 4]).unwrap());
        assert_ne!(a, b);
        store.remove(a);
        assert!(store.get(a).is_none());
        assert_eq!(store.get(b).unwrap().premultiplied_rgba, vec![255; 4]);
    }

    #[test]
    fn visit_reaches_nested_nodes_and_serde_round_trips() {
        let fill = Node::Fill(Fill { transform: Affine::IDENTITY, path: BezPath::new(), rule: FillRule::NonZero, paint: Paint::Solid(Rgba8::BLACK) });
        let group = Node::Group(Group { mask: Some(vec![fill.clone()]), children: vec![fill.clone(), fill], ..Group::default() });
        let list = DisplayList { size: Size::new(10.0, 10.0), background: None, nodes: vec![group] };
        let mut count = 0;
        list.visit(&mut |_| count += 1);
        assert_eq!(count, 4);
        let json = serde_json::to_string(&list).unwrap();
        assert_eq!(serde_json::from_str::<DisplayList>(&json).unwrap(), list);
    }
}
