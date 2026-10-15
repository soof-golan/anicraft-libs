//! Artwork trees to display lists.

use anim_core::{Affine, BlendMode, ColorTransform, Rgba8};
use anim_display::{Bitmap, DisplayList, Group, ImageStore, Node};

use crate::tree::{ArtKind, ArtMask, ArtNode, ArtworkTree};

/// Deepest nesting converted; a hand-built tree deeper than this is cut off rather than overflowing the stack.
const MAX_DEPTH: usize = 256;

/// Converts a whole tree: hidden nodes are left out, PSD clipping runs become masked groups, bitmaps go into
/// `store`.
#[must_use]
pub fn to_display(tree: &ArtworkTree, store: &mut ImageStore) -> DisplayList {
    let mut list = DisplayList::new(tree.size, None::<Rgba8>);
    list.nodes = match &tree.root.kind {
        ArtKind::Group(children) => children_to_nodes(children, store, 0),
        _ => node_to_display(&tree.root, store).into_iter().collect(),
    };
    list
}

/// Converts one subtree (one puppet layer, say). `None` if the node is hidden.
#[must_use]
pub fn node_to_display(node: &ArtNode, store: &mut ImageStore) -> Option<Node> {
    convert(node, store, 0)
}

fn convert(node: &ArtNode, store: &mut ImageStore, depth: usize) -> Option<Node> {
    if !node.visible || depth > MAX_DEPTH {
        return None;
    }
    let mut group = Group {
        color: ColorTransform::alpha(node.opacity.clamp(0.0, 1.0)),
        blend: node.blend,
        ..Group::default()
    };
    match &node.kind {
        ArtKind::Group(children) => group.children = children_to_nodes(children, store, depth.saturating_add(1)),
        ArtKind::Raster { image, offset } => {
            let id = store.insert(image.clone());
            group.children = vec![Node::Bitmap(Bitmap { transform: Affine::translate(offset.to_vec2()), image: id, smoothing: true })];
        }
        ArtKind::Vector { nodes, .. } => group.children.clone_from(nodes),
    }
    group.mask = node.mask.as_ref().map(|mask| match mask {
        ArtMask::Raster { image, offset } => {
            let id = store.insert(image.clone());
            vec![Node::Bitmap(Bitmap { transform: Affine::translate(offset.to_vec2()), image: id, smoothing: true })]
        }
        ArtMask::Vector { nodes } => nodes.clone(),
    });
    Some(Node::Group(group))
}

/// Converts siblings; a run of `clipping` nodes after a base node is drawn through the base's shape.
fn children_to_nodes(children: &[ArtNode], store: &mut ImageStore, depth: usize) -> Vec<Node> {
    let mut out = Vec::new();
    let mut iter = children.iter().peekable();
    while let Some(base) = iter.next() {
        let mut clipped = Vec::new();
        if !base.clipping {
            while let Some(next) = iter.next_if(|c| c.clipping) {
                clipped.push(next);
            }
        }
        if clipped.is_empty() {
            out.extend(convert(base, store, depth));
            continue;
        }
        let Some(base_node) = convert(base, store, depth) else {
            // A hidden base hides what is clipped to it.
            continue;
        };
        // The clip shape is the base as it is on its own: layer opacity and blend mode don't shrink or move it.
        let mut shape = base.clone();
        shape.opacity = 1.0;
        shape.blend = BlendMode::Normal;
        shape.clipping = false;
        let clip_nodes: Vec<Node> = convert(&shape, store, depth).into_iter().collect();
        let content: Vec<Node> = clipped.iter().filter_map(|c| convert(c, store, depth)).collect();
        out.push(Node::Group(Group {
            children: vec![
                base_node,
                Node::Group(Group { mask: Some(clip_nodes), children: content, ..Group::default() }),
            ],
            ..Group::default()
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use anim_core::{Point, Size};
    use anim_display::Image;

    use super::*;

    fn px(w: u32, h: u32, rgba: [u8; 4]) -> Image {
        let n = usize::try_from(w * h).unwrap();
        Image::new(w, h, rgba.iter().copied().cycle().take(n * 4).collect()).unwrap()
    }

    fn tree(children: Vec<ArtNode>) -> ArtworkTree {
        ArtworkTree { size: Size::new(8.0, 8.0), root: ArtNode::group("root", children), warnings: Vec::new() }
    }

    #[test]
    fn hidden_nodes_are_skipped_and_images_stored() {
        let mut hidden = ArtNode::raster("hidden", px(2, 2, [255, 0, 0, 255]), Point::ORIGIN);
        hidden.visible = false;
        let shown = ArtNode::raster("shown", px(2, 2, [0, 255, 0, 255]), Point::new(3.0, 4.0));
        let mut store = ImageStore::new();
        let list = to_display(&tree(vec![hidden, shown]), &mut store);
        assert_eq!(list.nodes.len(), 1);
        assert_eq!(store.len(), 1);
        let Node::Group(g) = &list.nodes[0] else { panic!("group expected") };
        let Node::Bitmap(b) = &g.children[0] else { panic!("bitmap expected") };
        assert_eq!(b.transform.translation().x, 3.0);
    }

    #[test]
    fn opacity_and_blend_reach_the_group() {
        let mut n = ArtNode::raster("n", px(1, 1, [1, 2, 3, 255]), Point::ORIGIN);
        n.opacity = 0.5;
        n.blend = BlendMode::Multiply;
        let mut store = ImageStore::new();
        let list = to_display(&tree(vec![n]), &mut store);
        let Node::Group(g) = &list.nodes[0] else { panic!("group expected") };
        assert_eq!(g.color.mul[3], 0.5);
        assert_eq!(g.blend, BlendMode::Multiply);
    }

    #[test]
    fn clipping_run_becomes_a_masked_group() {
        let base = ArtNode::raster("base", px(2, 2, [255, 255, 255, 255]), Point::ORIGIN);
        let mut clipped = ArtNode::raster("clipped", px(4, 4, [0, 0, 255, 255]), Point::ORIGIN);
        clipped.clipping = true;
        let mut store = ImageStore::new();
        let list = to_display(&tree(vec![base, clipped]), &mut store);
        assert_eq!(list.nodes.len(), 1);
        let Node::Group(outer) = &list.nodes[0] else { panic!("group expected") };
        assert_eq!(outer.children.len(), 2);
        let Node::Group(inner) = &outer.children[1] else { panic!("group expected") };
        assert!(inner.mask.is_some());
        assert_eq!(inner.children.len(), 1);
    }

    #[test]
    fn hidden_base_hides_clipped_layers() {
        let mut base = ArtNode::raster("base", px(2, 2, [255, 255, 255, 255]), Point::ORIGIN);
        base.visible = false;
        let mut clipped = ArtNode::raster("clipped", px(2, 2, [0, 0, 255, 255]), Point::ORIGIN);
        clipped.clipping = true;
        let list = to_display(&tree(vec![base, clipped]), &mut ImageStore::new());
        assert!(list.nodes.is_empty());
    }

    #[test]
    fn deep_trees_do_not_overflow() {
        let mut node = ArtNode::raster("leaf", px(1, 1, [0, 0, 0, 255]), Point::ORIGIN);
        for _ in 0..2000 {
            node = ArtNode::group("g", vec![node]);
        }
        let list = to_display(&tree(vec![node]), &mut ImageStore::new());
        assert_eq!(list.nodes.len(), 1);
    }
}
