//! Consistency checks. Validation never panics and never changes the document; it reports [`Warning`]s.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::document::{Document, TimelineId};
use crate::library::LibraryItem;
use crate::query::{RefKind, Reference};
use crate::timeline::{LayerKind, TweenType};

/// Most warnings one validation run reports; a hostile file cannot make the list grow without bound.
pub const MAX_WARNINGS: usize = 10_000;

/// Where a warning applies.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Location {
    pub timeline: Option<TimelineId>,
    pub layer: Option<usize>,
    pub frame: Option<u32>,
}

/// What is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// A symbol instance names a library item that does not exist, or is not a symbol.
    DanglingSymbol(String),
    /// A bitmap instance or bitmap fill names a missing library bitmap.
    DanglingBitmap(String),
    /// A frame sound names a missing library sound.
    DanglingSound(String),
    /// Keyframes are not sorted by index.
    UnsortedFrames,
    /// A keyframe starts before the previous one ends.
    OverlappingFrames { previous_index: u32, previous_end: u32 },
    /// A keyframe lasts zero frames.
    ZeroDuration,
    /// `parent_layer_index` is out of range or not above the layer.
    BadLayerParent(usize),
    /// The parent layer is not a folder, mask or guide layer.
    ParentNotContainer(usize),
    /// A tween has no keyframe after it.
    TweenWithoutEnd,
    /// A shape tween on a keyframe that holds something other than plain shapes.
    ShapeTweenNeedsShapes,
    /// A classic tween on a keyframe that holds raw shapes.
    ClassicTweenNeedsSymbols,
    /// `acceleration` outside -100 ..= 100.
    AccelerationOutOfRange(i32),
    /// A symbol contains itself, directly or through other symbols (`from` uses `to`).
    SymbolCycle { from: String, to: String },
    /// Two library items share a name.
    DuplicateLibraryName(String),
    /// A bitmap or sound item has no data loaded.
    MissingMedia(String),
    /// `current_timeline` does not name a scene.
    CurrentTimelineOutOfRange(usize),
    /// The document has no scene.
    NoScenes,
    /// Width, height or frame rate is not a positive finite number.
    BadStageSettings,
    /// Two scenes share a name.
    DuplicateSceneName(String),
}

/// One finding of [`Document::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub location: Location,
    pub kind: WarningKind,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location.timeline {
            Some(TimelineId::Scene(i)) => write!(f, "scene {i}")?,
            Some(TimelineId::Symbol(n)) => write!(f, "symbol {n:?}")?,
            None => write!(f, "document")?,
        }
        if let Some(l) = self.location.layer {
            write!(f, ", layer {l}")?;
        }
        if let Some(fr) = self.location.frame {
            write!(f, ", frame {fr}")?;
        }
        write!(f, ": ")?;
        match &self.kind {
            WarningKind::DanglingSymbol(n) => write!(f, "instance of missing symbol {n:?}"),
            WarningKind::DanglingBitmap(n) => write!(f, "use of missing bitmap {n:?}"),
            WarningKind::DanglingSound(n) => write!(f, "use of missing sound {n:?}"),
            WarningKind::UnsortedFrames => write!(f, "keyframes are not sorted by index"),
            WarningKind::OverlappingFrames { previous_index, previous_end } => {
                write!(f, "keyframe overlaps the one at {previous_index} (which ends at {previous_end})")
            }
            WarningKind::ZeroDuration => write!(f, "keyframe has zero duration"),
            WarningKind::BadLayerParent(p) => write!(f, "parent layer index {p} is invalid"),
            WarningKind::ParentNotContainer(p) => write!(f, "parent layer {p} is not a folder, mask or guide"),
            WarningKind::TweenWithoutEnd => write!(f, "tween has no keyframe after it"),
            WarningKind::ShapeTweenNeedsShapes => write!(f, "shape tween on content that is not plain shapes"),
            WarningKind::ClassicTweenNeedsSymbols => write!(f, "classic tween on a raw shape (convert it to a symbol)"),
            WarningKind::AccelerationOutOfRange(a) => write!(f, "ease acceleration {a} is outside -100..100"),
            WarningKind::SymbolCycle { from, to } => write!(f, "symbol {from:?} uses {to:?}, which leads back to it"),
            WarningKind::DuplicateLibraryName(n) => write!(f, "duplicate library name {n:?}"),
            WarningKind::MissingMedia(n) => write!(f, "library item {n:?} has no data loaded"),
            WarningKind::CurrentTimelineOutOfRange(i) => write!(f, "current timeline {i} does not exist"),
            WarningKind::NoScenes => write!(f, "no scenes"),
            WarningKind::BadStageSettings => write!(f, "stage size or frame rate is not a positive number"),
            WarningKind::DuplicateSceneName(n) => write!(f, "duplicate scene name {n:?}"),
        }
    }
}

struct Sink {
    out: Vec<Warning>,
}

impl Sink {
    fn push(&mut self, timeline: Option<&TimelineId>, layer: Option<usize>, frame: Option<u32>, kind: WarningKind) {
        if self.out.len() < MAX_WARNINGS {
            self.out.push(Warning { location: Location { timeline: timeline.cloned(), layer, frame }, kind });
        }
    }
}

impl Document {
    /// Checks the document and returns what looks wrong: dangling library references, overlapping or unsorted
    /// frames, bad layer parents, tweens that cannot play, symbol cycles, missing media, bad stage settings.
    #[must_use]
    pub fn validate(&self) -> Vec<Warning> {
        let mut sink = Sink { out: Vec::new() };
        self.validate_document(&mut sink);
        self.validate_library(&mut sink);
        for id in self.timeline_ids() {
            self.validate_timeline(&id, &mut sink);
        }
        self.validate_references(&mut sink);
        self.validate_cycles(&mut sink);
        sink.out
    }

    fn validate_document(&self, sink: &mut Sink) {
        if self.scenes.is_empty() {
            sink.push(None, None, None, WarningKind::NoScenes);
        } else if self.current_timeline >= self.scenes.len() {
            sink.push(None, None, None, WarningKind::CurrentTimelineOutOfRange(self.current_timeline));
        }
        let ok = |v: f64| v.is_finite() && v > 0.0;
        if !(ok(self.width) && ok(self.height) && ok(self.frame_rate)) {
            sink.push(None, None, None, WarningKind::BadStageSettings);
        }
        let mut seen = HashSet::new();
        for s in &self.scenes {
            if !seen.insert(s.name.as_str()) {
                sink.push(None, None, None, WarningKind::DuplicateSceneName(s.name.clone()));
            }
        }
    }

    fn validate_library(&self, sink: &mut Sink) {
        let mut seen = HashSet::new();
        for item in &self.library.items {
            if !seen.insert(item.name()) {
                sink.push(None, None, None, WarningKind::DuplicateLibraryName(item.name().to_owned()));
            }
            let needs_data = match item {
                LibraryItem::Bitmap(b) => b.media.is_none(),
                LibraryItem::Sound(s) => s.media.is_none(),
                _ => false,
            };
            if needs_data {
                sink.push(None, None, None, WarningKind::MissingMedia(item.name().to_owned()));
            }
        }
    }

    fn validate_timeline(&self, id: &TimelineId, sink: &mut Sink) {
        let Some(tl) = self.timeline(id) else { return };
        for (li, layer) in tl.layers.iter().enumerate() {
            if let Some(p) = layer.parent_layer_index {
                match tl.layers.get(p) {
                    Some(parent) if p < li => {
                        if !matches!(parent.kind, LayerKind::Folder | LayerKind::Mask | LayerKind::Guide) {
                            sink.push(Some(id), Some(li), None, WarningKind::ParentNotContainer(p));
                        }
                    }
                    _ => sink.push(Some(id), Some(li), None, WarningKind::BadLayerParent(p)),
                }
            }
            if layer.frames.windows(2).any(|w| matches!(w, [a, b] if a.index > b.index)) {
                sink.push(Some(id), Some(li), None, WarningKind::UnsortedFrames);
            }
            let mut sorted: Vec<&crate::timeline::Frame> = layer.frames.iter().collect();
            sorted.sort_by_key(|f| f.index);
            for w in sorted.windows(2) {
                if let [a, b] = w
                    && b.index < a.end()
                {
                    sink.push(Some(id), Some(li), Some(b.index), WarningKind::OverlappingFrames { previous_index: a.index, previous_end: a.end() });
                }
            }
            for frame in &layer.frames {
                if frame.duration == 0 {
                    sink.push(Some(id), Some(li), Some(frame.index), WarningKind::ZeroDuration);
                }
                if !(-100..=100).contains(&frame.acceleration) {
                    sink.push(Some(id), Some(li), Some(frame.index), WarningKind::AccelerationOutOfRange(frame.acceleration));
                }
                if frame.is_tweened() && layer.next_keyframe_after(frame.index).is_none() && frame.tween_type != TweenType::MotionObject {
                    sink.push(Some(id), Some(li), Some(frame.index), WarningKind::TweenWithoutEnd);
                }
                match frame.tween_type {
                    TweenType::Shape if !frame.elements.iter().all(|e| matches!(e, crate::Element::Shape(_) | crate::Element::Group(_))) => {
                        sink.push(Some(id), Some(li), Some(frame.index), WarningKind::ShapeTweenNeedsShapes);
                    }
                    TweenType::Classic if frame.elements.iter().any(|e| matches!(e, crate::Element::Shape(s) if !s.is_drawing_object)) => {
                        sink.push(Some(id), Some(li), Some(frame.index), WarningKind::ClassicTweenNeedsSymbols);
                    }
                    _ => {}
                }
            }
        }
    }

    fn validate_references(&self, sink: &mut Sink) {
        let index = self.library.index();
        for Reference { timeline, layer, frame, kind, name } in self.references() {
            let found = index.get(name.as_str()).and_then(|&i| self.library.items.get(i));
            let resolves = matches!(
                (kind, found),
                (RefKind::SymbolInstance, Some(LibraryItem::Symbol(_)))
                    | (RefKind::BitmapInstance | RefKind::BitmapFill, Some(LibraryItem::Bitmap(_)))
                    | (RefKind::Sound, Some(LibraryItem::Sound(_)))
            );
            if resolves {
                continue;
            }
            let k = match kind {
                RefKind::SymbolInstance => WarningKind::DanglingSymbol(name),
                RefKind::BitmapInstance | RefKind::BitmapFill => WarningKind::DanglingBitmap(name),
                RefKind::Sound => WarningKind::DanglingSound(name),
            };
            sink.push(Some(&timeline), Some(layer), Some(frame), k);
        }
    }

    fn validate_cycles(&self, sink: &mut Sink) {
        // Iterative three-colour DFS over the symbol graph; every back edge is a cycle.
        let names: Vec<&str> = self.library.symbols().map(|s| s.name.as_str()).collect();
        let position: HashMap<&str, usize> = names.iter().enumerate().map(|(i, n)| (*n, i)).collect();
        let edges: Vec<Vec<usize>> = names
            .iter()
            .map(|n| self.direct_dependencies(&TimelineId::Symbol((*n).to_owned())).iter().filter_map(|d| position.get(d.as_str()).copied()).collect())
            .collect();
        let mut color = vec![0_u8; names.len()];
        for start in 0..names.len() {
            if color.get(start).copied() != Some(0) {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
            if let Some(c) = color.get_mut(start) {
                *c = 1;
            }
            while let Some(&(node, next)) = stack.last() {
                let target = edges.get(node).and_then(|e| e.get(next)).copied();
                match target {
                    None => {
                        if let Some(c) = color.get_mut(node) {
                            *c = 2;
                        }
                        stack.pop();
                    }
                    Some(t) => {
                        if let Some(top) = stack.last_mut() {
                            top.1 = top.1.saturating_add(1);
                        }
                        match color.get(t).copied() {
                            Some(0) => {
                                if let Some(c) = color.get_mut(t) {
                                    *c = 1;
                                }
                                stack.push((t, 0));
                            }
                            Some(1) => {
                                let from = names.get(node).copied().unwrap_or_default().to_owned();
                                let to = names.get(t).copied().unwrap_or_default().to_owned();
                                sink.push(Some(&TimelineId::Symbol(from.clone())), None, None, WarningKind::SymbolCycle { from, to });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// True when the symbol `from` uses `target` directly or through other symbols (or is `target`).
    #[must_use]
    pub fn symbol_reaches(&self, from: &str, target: &str) -> bool {
        if from == target {
            return true;
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue = vec![from.to_owned()];
        while let Some(cur) = queue.pop() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            for dep in self.direct_dependencies(&TimelineId::Symbol(cur)) {
                if dep == target {
                    return true;
                }
                queue.push(dep);
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use anim_core::Affine;

    use super::*;
    use crate::element::{Element, SymbolInstance, SymbolKind};
    use crate::library::BitmapItem;
    use crate::timeline::{Frame, FrameSound};

    fn instance(name: &str) -> Element {
        Element::SymbolInstance(Box::new(SymbolInstance::new(name, Affine::IDENTITY)))
    }

    fn kinds(doc: &Document) -> Vec<WarningKind> {
        doc.validate().into_iter().map(|w| w.kind).collect()
    }

    #[test]
    fn fresh_document_is_clean() {
        assert!(Document::default().validate().is_empty());
    }

    #[test]
    fn dangling_references_are_reported() {
        let mut doc = Document::default();
        doc.scenes[0].layers[0].frames[0].elements.push(instance("ghost"));
        doc.scenes[0].layers[0].frames[0].sound = Some(FrameSound::new("nosound"));
        doc.scenes[0].layers[0].frames[0].elements.push(Element::Bitmap(crate::BitmapInstance::new("nopic", Affine::IDENTITY)));
        let k = kinds(&doc);
        assert!(k.contains(&WarningKind::DanglingSymbol("ghost".into())));
        assert!(k.contains(&WarningKind::DanglingSound("nosound".into())));
        assert!(k.contains(&WarningKind::DanglingBitmap("nopic".into())));
        let text = doc.validate().iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
        assert!(text.contains("missing symbol \"ghost\""), "{text}");
    }

    #[test]
    fn instance_of_a_bitmap_name_is_dangling_symbol() {
        let mut doc = Document::default();
        doc.library.push(LibraryItem::Bitmap(BitmapItem::new("pic", "i"))).unwrap();
        doc.scenes[0].layers[0].frames[0].elements.push(instance("pic"));
        assert!(kinds(&doc).contains(&WarningKind::DanglingSymbol("pic".into())));
        assert!(kinds(&doc).contains(&WarningKind::MissingMedia("pic".into())));
    }

    #[test]
    fn overlap_unsorted_zero_and_tween_problems() {
        let mut doc = Document::default();
        let layer = &mut doc.scenes[0].layers[0];
        layer.frames = vec![Frame::blank(5, 3), Frame::blank(0, 7), Frame::blank(20, 0)];
        layer.frames[1].tween_type = TweenType::Classic;
        layer.frames[2].acceleration = 500;
        let k = kinds(&doc);
        assert!(k.contains(&WarningKind::UnsortedFrames));
        assert!(k.iter().any(|w| matches!(w, WarningKind::OverlappingFrames { previous_index: 0, previous_end: 7 })));
        assert!(k.contains(&WarningKind::ZeroDuration));
        assert!(k.contains(&WarningKind::AccelerationOutOfRange(500)));
        doc.scenes[0].layers[0].frames = vec![Frame::blank(0, 7)];
        doc.scenes[0].layers[0].frames[0].tween_type = TweenType::Classic;
        assert!(kinds(&doc).contains(&WarningKind::TweenWithoutEnd));
    }

    #[test]
    fn layer_parents_are_checked() {
        let mut doc = Document::default();
        let mut l2 = crate::Layer::new("b", anim_core::Rgba8::BLACK);
        l2.parent_layer_index = Some(0);
        let mut l3 = crate::Layer::new("c", anim_core::Rgba8::BLACK);
        l3.parent_layer_index = Some(9);
        doc.scenes[0].layers.push(l2);
        doc.scenes[0].layers.push(l3);
        let k = kinds(&doc);
        assert!(k.contains(&WarningKind::ParentNotContainer(0)));
        assert!(k.contains(&WarningKind::BadLayerParent(9)));
    }

    #[test]
    fn symbol_cycles_are_found_and_reach_queries_work() {
        let mut doc = Document::default();
        for n in ["a", "b", "c"] {
            doc.add_symbol(n, SymbolKind::Graphic).unwrap();
        }
        let put = |doc: &mut Document, owner: &str, child: &str| {
            if let Some(s) = doc.library.find_symbol_mut(owner) {
                s.timeline.layers[0].frames[0].elements.push(instance(child));
            }
        };
        put(&mut doc, "a", "b");
        put(&mut doc, "b", "c");
        assert!(doc.validate().is_empty());
        assert!(doc.symbol_reaches("a", "c"));
        assert!(!doc.symbol_reaches("c", "a"));
        put(&mut doc, "c", "a");
        assert!(kinds(&doc).iter().any(|w| matches!(w, WarningKind::SymbolCycle { .. })));
        assert!(doc.symbol_reaches("c", "b"));
        put(&mut doc, "a", "a");
        assert!(doc.symbol_reaches("a", "a"));
    }

    #[test]
    fn document_level_problems() {
        let mut doc = Document::default();
        doc.current_timeline = 4;
        doc.width = 0.0;
        doc.scenes.push(doc.scenes[0].clone());
        let k = kinds(&doc);
        assert!(k.contains(&WarningKind::CurrentTimelineOutOfRange(4)));
        assert!(k.contains(&WarningKind::BadStageSettings));
        assert!(k.contains(&WarningKind::DuplicateSceneName("Scene 1".into())));
        doc.scenes.clear();
        assert!(kinds(&doc).contains(&WarningKind::NoScenes));
    }
}
