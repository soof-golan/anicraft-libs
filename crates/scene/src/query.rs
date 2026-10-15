//! Read-only queries: which keyframe covers a frame, the layer tree, library references.

use anim_core::num::usize_to_u32_sat;
use anim_shape::{EdgeShape, FillStyle, StrokeStyle};

use crate::document::{Document, TimelineId};
use crate::element::Element;
use crate::library::{LibraryItem, Symbol};
use crate::timeline::{Frame, Layer, LayerKind, Timeline};

/// Deepest layer nesting the tree builder follows; a layer nested deeper is listed as a root.
pub const MAX_LAYER_DEPTH: usize = 64;

/// One node of [`Timeline::layer_tree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerNode {
    /// Index into [`Timeline::layers`].
    pub index: usize,
    /// Layers inside this folder (or masked / guided by this layer), top to bottom.
    pub children: Vec<Self>,
}

/// What a [`Reference`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    /// A symbol instance names a library item.
    SymbolInstance,
    /// A bitmap instance names a library bitmap.
    BitmapInstance,
    /// A shape's bitmap fill names a library bitmap.
    BitmapFill,
    /// A frame sound names a library sound.
    Sound,
}

/// A use of a library item somewhere in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub timeline: TimelineId,
    pub layer: usize,
    /// Index of the keyframe holding the reference.
    pub frame: u32,
    pub kind: RefKind,
    /// The library name used.
    pub name: String,
}

/// Library bitmap names used by the fills and strokes of a shape.
#[must_use]
pub fn shape_bitmap_paths(shape: &EdgeShape) -> Vec<&str> {
    let mut out = Vec::new();
    for fill in &shape.fills {
        if let FillStyle::Bitmap(b) = fill {
            out.push(b.path.as_str());
        }
    }
    for stroke in &shape.strokes {
        let base = match stroke {
            StrokeStyle::Solid(s) => s,
            StrokeStyle::Other(o) => &o.base,
        };
        if let FillStyle::Bitmap(b) = &base.fill {
            out.push(b.path.as_str());
        }
    }
    out
}

impl Timeline {
    /// Length of the timeline in frames: the end of the last span on any layer.
    #[must_use]
    pub fn frame_count(&self) -> u32 {
        self.layers.iter().map(Layer::frame_count).max().unwrap_or(0)
    }

    /// The keyframe on `layer` whose span covers `frame`.
    #[must_use]
    pub fn keyframe_at(&self, layer: usize, frame: u32) -> Option<&Frame> {
        self.layers.get(layer)?.keyframe_at(frame)
    }

    /// Every layer's covering keyframe at `frame`, as `(layer index, keyframe)`, top to bottom.
    #[must_use]
    pub fn keyframes_at(&self, frame: u32) -> Vec<(usize, &Frame)> {
        self.layers.iter().enumerate().filter_map(|(i, l)| Some((i, l.keyframe_at(frame)?))).collect()
    }

    /// The layer's parent, when the stored index is valid (smaller than the layer's own index).
    #[must_use]
    pub fn parent_of(&self, layer: usize) -> Option<usize> {
        let parent = self.layers.get(layer)?.parent_layer_index?;
        (parent < layer).then_some(parent)
    }

    /// Indices of the layers whose parent is `layer`, in order.
    #[must_use]
    pub fn children_of(&self, layer: usize) -> Vec<usize> {
        (0..self.layers.len()).filter(|&i| self.parent_of(i) == Some(layer)).collect()
    }

    /// Indices of `layer`'s parents, nearest first (iterative, so hostile nesting is harmless).
    #[must_use]
    pub fn ancestors_of(&self, layer: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut cur = layer;
        while let Some(p) = self.parent_of(cur) {
            out.push(p);
            cur = p;
        }
        out
    }

    /// How deep each layer is nested (roots are 0).
    #[must_use]
    pub fn layer_depths(&self) -> Vec<usize> {
        let mut depths: Vec<usize> = Vec::with_capacity(self.layers.len());
        for i in 0..self.layers.len() {
            let d = self.parent_of(i).map_or(0_usize, |p| depths.get(p).copied().unwrap_or(0).saturating_add(1));
            depths.push(d);
        }
        depths
    }

    /// The layer's kind with `Guided` / `Masked` filled in from its parent: a normal layer whose parent is a guide
    /// layer is guided, one whose parent is a mask layer is masked.
    #[must_use]
    pub fn resolved_kind(&self, layer: usize) -> Option<LayerKind> {
        let l = self.layers.get(layer)?;
        if l.kind != LayerKind::Normal {
            return Some(l.kind);
        }
        Some(match self.parent_of(layer).and_then(|p| self.layers.get(p)).map(|p| p.kind) {
            Some(LayerKind::Guide) => LayerKind::Guided,
            Some(LayerKind::Mask) => LayerKind::Masked,
            _ => LayerKind::Normal,
        })
    }

    /// False when the layer or any parent folder is hidden.
    #[must_use]
    pub fn is_effectively_visible(&self, layer: usize) -> bool {
        self.layers.get(layer).is_some_and(|l| l.visible) && self.ancestors_of(layer).iter().all(|&p| self.layers.get(p).is_some_and(|l| l.visible))
    }

    /// True when the layer or any parent folder is locked.
    #[must_use]
    pub fn is_effectively_locked(&self, layer: usize) -> bool {
        self.layers.get(layer).is_some_and(|l| l.locked) || self.ancestors_of(layer).iter().any(|&p| self.layers.get(p).is_some_and(|l| l.locked))
    }

    /// The layers as a forest: folders (and mask / guide layers) own the layers whose `parent_layer_index` names
    /// them. Layers with an invalid parent, or nested deeper than [`MAX_LAYER_DEPTH`], are roots.
    #[must_use]
    pub fn layer_tree(&self) -> Vec<LayerNode> {
        let n = self.layers.len();
        let depths = self.layer_depths();
        let mut pending: Vec<Vec<LayerNode>> = (0..n).map(|_| Vec::new()).collect();
        let mut roots = Vec::new();
        for i in (0..n).rev() {
            let mut children = pending.get_mut(i).map(std::mem::take).unwrap_or_default();
            children.reverse();
            let node = LayerNode { index: i, children };
            let parent = self.parent_of(i).filter(|_| depths.get(i).copied().unwrap_or(0) <= MAX_LAYER_DEPTH);
            match parent.and_then(|p| pending.get_mut(p)) {
                Some(list) => list.push(node),
                None => roots.push(node),
            }
        }
        roots.reverse();
        roots
    }

    /// The first layer called `name`.
    #[must_use]
    pub fn layer_index_by_name(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.name == name)
    }

    /// Frame labels of the timeline as `(frame index, label)`, sorted by frame.
    #[must_use]
    pub fn labels(&self) -> Vec<(u32, &str)> {
        let mut out: Vec<(u32, &str)> =
            self.layers.iter().flat_map(|l| l.frames.iter()).filter_map(|f| f.label.as_deref().map(|label| (f.index, label))).collect();
        out.sort_by_key(|&(i, _)| i);
        out
    }
}

impl Document {
    /// Every use of a library item in the document: instances, bitmap fills and frame sounds, in timeline order.
    #[must_use]
    pub fn references(&self) -> Vec<Reference> {
        let mut out = Vec::new();
        for id in self.timeline_ids() {
            if let Some(tl) = self.timeline(&id) {
                collect_references(&id, tl, &mut out);
            }
        }
        out
    }

    /// Uses of the library item `name`.
    #[must_use]
    pub fn references_to(&self, name: &str) -> Vec<Reference> {
        self.references().into_iter().filter(|r| r.name == name).collect()
    }

    /// Names of the symbols directly used by the symbol (or scene) `id`, deduplicated, in first-use order.
    #[must_use]
    pub fn direct_dependencies(&self, id: &TimelineId) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(tl) = self.timeline(id) {
            let mut refs = Vec::new();
            collect_references(id, tl, &mut refs);
            for r in refs {
                if r.kind == RefKind::SymbolInstance && !out.contains(&r.name) {
                    out.push(r.name);
                }
            }
        }
        out
    }

    /// Library items nothing refers to (symbols, bitmaps and sounds; folders and fonts are never reported).
    #[must_use]
    pub fn unused_items(&self) -> Vec<&str> {
        let used: std::collections::HashSet<String> = self.references().into_iter().map(|r| r.name).collect();
        self.library
            .items
            .iter()
            .filter(|i| matches!(i, LibraryItem::Symbol(_) | LibraryItem::Bitmap(_) | LibraryItem::Sound(_)))
            .map(LibraryItem::name)
            .filter(|n| !used.contains(*n))
            .collect()
    }

    /// The symbol an instance resolves to.
    #[must_use]
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.library.find_symbol(name)
    }

    /// Keyframe count over the whole document, for statistics.
    #[must_use]
    pub fn keyframe_count(&self) -> u32 {
        let n: usize = self
            .timeline_ids()
            .iter()
            .filter_map(|id| self.timeline(id))
            .flat_map(|t| t.layers.iter())
            .map(|l| l.frames.len())
            .fold(0_usize, usize::saturating_add);
        usize_to_u32_sat(n)
    }
}

fn collect_references(id: &TimelineId, tl: &Timeline, out: &mut Vec<Reference>) {
    for (li, layer) in tl.layers.iter().enumerate() {
        for frame in &layer.frames {
            let mut push = |kind: RefKind, name: &str| {
                out.push(Reference { timeline: id.clone(), layer: li, frame: frame.index, kind, name: name.to_owned() });
            };
            if let Some(sound) = &frame.sound {
                push(RefKind::Sound, &sound.name);
            }
            for element in &frame.elements {
                element.walk(&mut |e| match e {
                    Element::SymbolInstance(s) => push(RefKind::SymbolInstance, &s.library_item_name),
                    Element::Bitmap(b) => push(RefKind::BitmapInstance, &b.library_item_name),
                    Element::Shape(s) => {
                        for path in shape_bitmap_paths(&s.shape) {
                            push(RefKind::BitmapFill, path);
                        }
                    }
                    Element::Text(_) | Element::Group(_) | Element::Unknown(_) => {}
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use anim_core::{Affine, Rgba8};

    use super::*;
    use crate::element::SymbolInstance;
    use crate::timeline::FrameSound;

    fn doc_with_layers(kinds: &[(LayerKind, Option<usize>)]) -> Timeline {
        let mut tl = Timeline::new("t");
        for (i, (k, p)) in kinds.iter().enumerate() {
            let mut l = Layer::new(format!("L{i}"), Rgba8::BLACK);
            l.kind = *k;
            l.parent_layer_index = *p;
            l.frames.push(Frame::blank(0, 4));
            tl.layers.push(l);
        }
        tl
    }

    #[test]
    fn layer_tree_nests_folders_and_masks() {
        let tl = doc_with_layers(&[
            (LayerKind::Folder, None),
            (LayerKind::Normal, Some(0)),
            (LayerKind::Mask, Some(0)),
            (LayerKind::Normal, Some(2)),
            (LayerKind::Normal, None),
        ]);
        let tree = tl.layer_tree();
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].index, 0);
        assert_eq!(tree[0].children.iter().map(|c| c.index).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(tree[0].children[1].children[0].index, 3);
        assert_eq!(tree[1].index, 4);
        assert_eq!(tl.children_of(0), vec![1, 2]);
        assert_eq!(tl.ancestors_of(3), vec![2, 0]);
        assert_eq!(tl.layer_depths(), vec![0, 1, 1, 2, 0]);
        assert_eq!(tl.resolved_kind(3), Some(LayerKind::Masked));
        assert_eq!(tl.resolved_kind(1), Some(LayerKind::Normal));
        assert_eq!(tl.resolved_kind(2), Some(LayerKind::Mask));
        assert_eq!(tl.resolved_kind(9), None);
    }

    #[test]
    fn invalid_parents_become_roots() {
        let tl = doc_with_layers(&[(LayerKind::Normal, Some(0)), (LayerKind::Normal, Some(7)), (LayerKind::Normal, Some(2))]);
        assert_eq!(tl.layer_tree().iter().map(|n| n.index).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(tl.parent_of(0), None);
    }

    #[test]
    fn deep_nesting_is_capped() {
        let mut spec = vec![(LayerKind::Folder, None)];
        for i in 0..(MAX_LAYER_DEPTH * 3) {
            spec.push((LayerKind::Folder, Some(i)));
        }
        let tl = doc_with_layers(&spec);
        let tree = tl.layer_tree();
        let mut count = 0;
        let mut stack: Vec<&LayerNode> = tree.iter().collect();
        while let Some(n) = stack.pop() {
            count += 1;
            stack.extend(n.children.iter());
        }
        assert_eq!(count, spec.len());
        assert!(tree.len() > 1);
    }

    #[test]
    fn effective_visibility_and_lock_follow_folders() {
        let mut tl = doc_with_layers(&[(LayerKind::Folder, None), (LayerKind::Normal, Some(0))]);
        assert!(tl.is_effectively_visible(1));
        tl.layers[0].visible = false;
        tl.layers[0].locked = true;
        assert!(!tl.is_effectively_visible(1));
        assert!(tl.is_effectively_locked(1));
        assert!(!tl.is_effectively_visible(99));
    }

    #[test]
    fn timeline_frame_counts_and_lookup() {
        let mut tl = doc_with_layers(&[(LayerKind::Normal, None), (LayerKind::Normal, None)]);
        tl.layers[1].frames = vec![Frame::blank(0, 2), Frame::blank(2, 10)];
        assert_eq!(tl.frame_count(), 12);
        assert_eq!(tl.keyframe_at(1, 5).map(|f| f.index), Some(2));
        assert!(tl.keyframe_at(0, 5).is_none());
        assert!(tl.keyframe_at(9, 0).is_none());
        assert_eq!(tl.keyframes_at(1).len(), 2);
        assert_eq!(tl.keyframes_at(5).len(), 1);
        tl.layers[0].frames[0].label = Some("start".into());
        assert_eq!(tl.labels(), vec![(0, "start")]);
        assert_eq!(tl.layer_index_by_name("L1"), Some(1));
    }

    #[test]
    fn references_find_instances_and_sounds() {
        let mut doc = Document::default();
        if let Some(layer) = doc.scenes[0].layers.get_mut(0) {
            let f = &mut layer.frames[0];
            f.elements.push(Element::SymbolInstance(Box::new(SymbolInstance::new("a", Affine::IDENTITY))));
            f.elements.push(Element::Group(crate::element::Group::new(vec![Element::SymbolInstance(Box::new(SymbolInstance::new("b", Affine::IDENTITY)))])));
            f.sound = Some(FrameSound::new("snd"));
        }
        let refs = doc.references();
        assert_eq!(refs.len(), 3);
        assert_eq!(doc.references_to("b").len(), 1);
        assert_eq!(doc.direct_dependencies(&TimelineId::Scene(0)), vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(doc.keyframe_count(), 1);
        doc.add_symbol("a", crate::SymbolKind::Graphic).unwrap();
        doc.add_symbol("c", crate::SymbolKind::Graphic).unwrap();
        assert_eq!(doc.unused_items(), vec!["c"]);
    }
}
