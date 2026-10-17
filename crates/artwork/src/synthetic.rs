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

/// The flat colour of the `index`-th mouth shape of [`face`]: every shape differs, so a render shows which one is up.
#[must_use]
pub fn mouth_colour(index: usize) -> [u8; 3] {
    let step = u8::try_from(index).unwrap_or(u8::MAX);
    [20_u8.saturating_add(step.saturating_mul(15)), 220_u8.saturating_sub(step.saturating_mul(12)), 40_u8.saturating_add(step.saturating_mul(5))]
}

/// A 200 x 200 face of rectangles named like a Character Animator puppet.
///
/// A body with a torso, and a head with a face, left and right eyes (a white, a pupil and a hidden blink lid), brows and
/// a `mouth` group with one rectangle per name in `mouth_shapes`, coloured by [`mouth_colour`] and hidden except the one
/// called `shown`. The pieces sit where a 200 pixel stage wants them: face at 50, 20 (100 x 100), eyes at x 110 and 60,
/// mouth at 85, 90 (30 x 15).
#[must_use]
pub fn face(mouth_shapes: &[&str], shown: &str) -> ArtworkTree {
    let eye = |side: &str, x: f64| {
        ArtNode::group(
            format!("{side} Eye"),
            vec![
                rect("White", x, 50.0, 30, 20, [255, 255, 255]),
                rect(&format!("+ {side} Pupil"), x + 10.0, 55.0, 10, 10, [10, 10, 10]),
                hidden(rect(&format!("{side} Blink"), x, 58.0, 30, 4, [30, 60, 220])),
            ],
        )
    };
    let shapes = mouth_shapes
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let node = rect(name, 85.0, 90.0, 30, 15, mouth_colour(index));
            if *name == shown { node } else { hidden(node) }
        })
        .collect();
    let head = ArtNode::group(
        "Head",
        vec![
            rect("Face", 50.0, 20.0, 100, 100, [240, 190, 160]),
            eye("Left", 110.0),
            eye("Right", 60.0),
            rect("+Left Eyebrow", 110.0, 36.0, 30, 10, [90, 40, 10]),
            rect("+ right eyebrow", 60.0, 36.0, 30, 10, [90, 40, 10]),
            ArtNode::group("mouth", shapes),
        ],
    );
    let body = ArtNode::group("Body", vec![rect("Torso", 60.0, 120.0, 80, 80, [120, 120, 130])]);
    tree(200, 200, vec![body, head])
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
    fn the_face_shows_one_mouth_shape() {
        let art = face(&["Neutral", "Ah", "Oh"], "Ah");
        assert_eq!(art.size, Size::new(200.0, 200.0));
        let mouth = art.find("mouth").unwrap();
        let shown: Vec<(&str, bool)> = mouth.children().iter().map(|n| (n.name.as_str(), n.visible)).collect();
        assert_eq!(shown, [("Neutral", false), ("Ah", true), ("Oh", false)]);
        assert!(art.find("+ Left Pupil").is_some() && art.find("Right Blink").is_some_and(|n| !n.visible));
        assert_eq!(mouth_colour(0), [20, 220, 40]);
        assert_eq!(mouth_colour(13), [215, 64, 105]);
    }

    #[test]
    fn an_impossible_size_is_an_empty_layer() {
        let ArtKind::Raster { image, .. } = rect("huge", 0.0, 0.0, u32::MAX, u32::MAX, [1; 3]).kind else { panic!("a raster") };
        assert_eq!((image.width, image.height), (0, 0));
    }
}
