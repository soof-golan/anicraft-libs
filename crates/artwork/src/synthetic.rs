//! Flat-colour layers for tests, benchmarks and examples.
//!
//! A puppet, a scene or a mask built from a few rectangles needs no file on disk and no importer, so every suite that
//! wanted one wrote the same three functions. They live here once.

use anim_core::{Point, Size};
use anim_display::Image;

use crate::{ArtNode, ArtworkTree};

/// A raster layer of `width` x `height` pixels in the opaque colour `rgb`, its top left corner at (`left`, `top`).
///
/// A size too large to hold in memory gives an empty layer.
#[must_use]
pub fn rect(name: &str, left: f64, top: f64, width: u32, height: u32, rgb: [u8; 3]) -> ArtNode {
    let [red, green, blue] = rgb;
    let pixels = Image::byte_len(width, height).map(|len| [red, green, blue, 255].into_iter().cycle().take(len).collect());
    let image = pixels.and_then(|pixels| Image::new(width, height, pixels).ok()).unwrap_or(Image { width: 0, height: 0, premultiplied_rgba: Vec::new() });
    ArtNode::raster(name, image, Point::new(left, top))
}

/// `node` with its visibility switched off.
#[must_use]
pub const fn hidden(mut node: ArtNode) -> ArtNode {
    node.visible = false;
    node
}

/// A `w` x `h` artwork tree whose unnamed root holds `children`.
#[must_use]
pub fn tree(w: u32, h: u32, children: Vec<ArtNode>) -> ArtworkTree {
    ArtworkTree { size: Size::new(f64::from(w), f64::from(h)), root: ArtNode::group("", children), warnings: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArtKind;

    #[test]
    fn rect_is_opaque_flat_colour_at_its_offset() {
        let node = rect("Face", 3.0, 4.0, 2, 3, [10, 20, 30]);
        let ArtKind::Raster { image, offset } = &node.kind else { panic!("a raster") };
        assert_eq!((image.width, image.height, *offset), (2, 3, Point::new(3.0, 4.0)));
        assert_eq!(image.premultiplied_rgba, [10, 20, 30, 255].repeat(6));
        assert_eq!(node.name, "Face");
        assert!(node.visible);
    }

    #[test]
    fn hidden_and_tree_compose() {
        let art = tree(40, 30, vec![hidden(rect("a", 0.0, 0.0, 1, 1, [0; 3])), rect("b", 0.0, 0.0, 1, 1, [0; 3])]);
        assert_eq!(art.size, Size::new(40.0, 30.0));
        assert_eq!(art.root.children().iter().map(|n| n.visible).collect::<Vec<_>>(), [false, true]);
        assert_eq!(art.names(), ["", "a", "b"]);
    }

    #[test]
    fn an_impossible_size_is_an_empty_layer() {
        let ArtKind::Raster { image, .. } = rect("huge", 0.0, 0.0, u32::MAX, u32::MAX, [1; 3]).kind else { panic!("a raster") };
        assert_eq!((image.width, image.height), (0, 0));
    }
}
