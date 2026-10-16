//! The evaluator: a document, a frame number, a display list.

use std::collections::HashMap;

use anim_core::kurbo::Shape as _;
use anim_core::{Affine, ColorTransform, Rect, Rgba8, Size};
use anim_display::{Bitmap, Fill, FillRule, Group, Image, ImageId, ImageStore, Node, Paint};
use anim_morph::{MorphMode, MorphOptions, MorphPlan};
use anim_scene::{
    Document, EaseTarget, Element, Frame, Layer, LayerKind, LoopMode, MAX_GROUP_DEPTH, MediaStore, SymbolInstance, SymbolKind, Timeline, TweenType,
    query::LayerNode, shape_bitmap_paths,
};
use anim_shape::EdgeShape;
use anim_text::TextEngine;

use crate::ease;
use crate::guide::Guide;
use crate::images::{ImageProvider, NoDecoder};
use crate::memo::Memo;
use crate::motion::MotionTracks;
use crate::text::text_nodes;
use crate::tween::{self, Placed};

/// Most cached shape-tween plans before the cache is emptied.
const MAX_PLANS: usize = 512;

/// Most document shapes whose display nodes, and most timelines whose layer trees, are kept.
const MAX_ITEMS: usize = 4096;

/// Grey used for a bitmap whose pixels are not available.
const MISSING_BITMAP: Rgba8 = Rgba8::new(128, 128, 128, 160);

/// What the evaluator includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalOptions {
    /// Draw layers that are hidden (and the content of hidden folders). Default false.
    pub include_hidden: bool,
    /// Draw the contents of guide layers, which are editing aids and normally invisible. Default false.
    pub include_guides: bool,
    /// Movie clips advance with the global frame (export and "Test Movie"). The default shows the clip's
    /// `first_frame`, which is what the authoring stage does.
    pub advance_movie_clips: bool,
    /// Deepest nesting of symbols followed. Default 16.
    pub max_symbol_depth: usize,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self { include_hidden: false, include_guides: false, advance_movie_clips: false, max_symbol_depth: 16 }
    }
}

/// Which element of which layer a top-level display node came from: layer index in the high 32 bits (plus one, so
/// zero means "none"), element index in the low 32 bits. Pair with [`split_key`].
#[must_use]
pub fn element_key(layer: usize, element: usize) -> u64 {
    let layer = u64::from(u32::try_from(layer).unwrap_or(u32::MAX).saturating_add(1));
    let element = u64::from(u32::try_from(element).unwrap_or(u32::MAX));
    (layer << 32) | element
}

/// Inverse of [`element_key`]: `(layer index, element index)`.
#[must_use]
pub fn split_key(key: u64) -> Option<(usize, usize)> {
    let layer = u32::try_from(key >> 32).ok()?.checked_sub(1)?;
    let element = u32::try_from(key & 0xffff_ffff).ok()?;
    Some((usize::try_from(layer).ok()?, usize::try_from(element).ok()?))
}

#[derive(Clone, Copy)]
struct Time {
    /// The frame being drawn in the timeline at hand.
    frame: u32,
    /// The global frame, which movie clips follow.
    clock: u32,
}

#[derive(Default)]
struct State<'a> {
    /// Symbols being drawn, outermost first (cycle protection and depth limit).
    stack: Vec<&'a str>,
}

/// A thing to draw: a stored element with tweened properties, or a freshly interpolated shape.
enum Item<'a> {
    Placed(Placed<'a>),
    Morphed { shape: EdgeShape, matrix: Affine },
}

/// A shape tween of a document: the timeline (by address), the layer, the keyframe and which of its shapes.
type PlanKey = (usize, usize, u32, usize);

/// Turns a [`Document`] into [`DisplayList`](anim_display::DisplayList)s.
///
/// The evaluator owns the things that make evaluation cheap across frames: the decoded bitmaps (an
/// [`ImageStore`] the renderer reads), the text engine and the shape-tween plans. Build one per document revision.
pub struct Evaluator<'a> {
    doc: &'a Document,
    media: Option<&'a MediaStore>,
    provider: Box<dyn ImageProvider>,
    images: ImageStore,
    image_ids: HashMap<String, Option<ImageId>>,
    /// Created on first use: loading the system fonts is by far the most expensive part of building an evaluator.
    text: Option<TextEngine>,
    options: EvalOptions,
    plans: HashMap<PlanKey, MorphPlan>,
    /// Display nodes of the shapes stored in the document. Tweened shapes are temporaries and never enter.
    shapes: Memo<Vec<Node>>,
    /// Motion guides by the keyframe that holds them.
    guides: Memo<Option<Guide>>,
    /// Motion-object tracks by the keyframe that holds them.
    motion: Memo<Option<MotionTracks>>,
    /// Layer trees by timeline, taken out while the timeline is being drawn.
    trees: Memo<Vec<LayerNode>>,
}

impl std::fmt::Debug for Evaluator<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Evaluator").field("images", &self.images.len()).field("options", &self.options).finish_non_exhaustive()
    }
}

impl<'a> Evaluator<'a> {
    /// An evaluator for `doc` with no bitmap decoder and system fonts.
    #[must_use]
    pub fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            media: None,
            provider: Box::new(NoDecoder),
            images: ImageStore::new(),
            image_ids: HashMap::new(),
            text: None,
            options: EvalOptions::default(),
            plans: HashMap::new(),
            shapes: Memo::new(MAX_ITEMS),
            guides: Memo::new(MAX_PLANS),
            motion: Memo::new(MAX_PLANS),
            trees: Memo::new(MAX_ITEMS),
        }
    }

    /// Reads bitmap bytes from this media store.
    #[must_use]
    pub const fn with_media(mut self, media: &'a MediaStore) -> Self {
        self.media = Some(media);
        self
    }

    /// Decodes bitmaps with `provider`.
    #[must_use]
    pub fn with_provider(mut self, provider: impl ImageProvider + 'static) -> Self {
        self.provider = Box::new(provider);
        self
    }

    /// Replaces the text engine (to share registered fonts, or to use one without system fonts in tests).
    #[must_use]
    pub fn with_text_engine(mut self, engine: TextEngine) -> Self {
        self.text = Some(engine);
        self
    }

    /// Sets the options.
    #[must_use]
    pub const fn with_options(mut self, options: EvalOptions) -> Self {
        self.options = options;
        self
    }

    /// The options in force.
    #[must_use]
    pub const fn options(&self) -> &EvalOptions {
        &self.options
    }

    /// Changes the options.
    pub const fn set_options(&mut self, options: EvalOptions) {
        self.options = options;
    }

    /// The document being evaluated.
    #[must_use]
    pub const fn document(&self) -> &'a Document {
        self.doc
    }

    /// Mutable access to the text engine (register fonts, resolve names).
    pub fn text_engine_mut(&mut self) -> &mut TextEngine {
        self.text.get_or_insert_with(TextEngine::new)
    }

    /// The decoded bitmaps; hand this to the renderer together with the display lists.
    #[must_use]
    pub const fn images(&self) -> &ImageStore {
        &self.images
    }

    /// Registers already-decoded pixels for the library bitmap `name`, replacing any earlier decode.
    pub fn add_image(&mut self, name: &str, image: Image) -> ImageId {
        if let Some(Some(old)) = self.image_ids.get(name) {
            self.images.remove(*old);
        }
        let id = self.images.insert(image);
        self.image_ids.insert(name.to_owned(), Some(id));
        self.shapes.clear();
        id
    }

    /// Forgets decoded bitmaps and shape-tween plans (after the media store changed).
    pub fn clear_caches(&mut self) {
        self.images = ImageStore::new();
        self.image_ids.clear();
        self.plans.clear();
        self.shapes.clear();
        self.guides.clear();
        self.motion.clear();
        self.trees.clear();
    }

    /// Number of frames of scene `scene`.
    #[must_use]
    pub fn scene_frames(&self, scene: usize) -> u32 {
        self.doc.scenes.get(scene).map_or(0, Timeline::frame_count)
    }

    /// The scene at `frame` (0-based) as a display list: stage size, background colour and the layers bottom to
    /// top. An unknown scene gives an empty stage.
    ///
    /// Each top-level element is a group whose [`Group::key`] comes from [`element_key`].
    #[must_use]
    pub fn frame(&mut self, scene: usize, frame: u32) -> anim_display::DisplayList {
        let doc = self.doc;
        let background = (doc.background_color.a > 0).then_some(doc.background_color);
        let mut list = anim_display::DisplayList::new(Size::new(doc.width, doc.height), background);
        if let Some(timeline) = doc.scenes.get(scene) {
            let mut state = State::default();
            self.timeline_nodes(timeline, Time { frame, clock: frame }, &mut state, &mut list.nodes);
        }
        list
    }

    /// A symbol's own timeline at `frame`, for previews and library thumbnails. The stage size is the document's,
    /// the background is transparent; use [`Evaluator::symbol_bounds`] to frame the content. An unknown symbol
    /// gives an empty list.
    #[must_use]
    pub fn frame_of_symbol(&mut self, name: &str, frame: u32) -> anim_display::DisplayList {
        let doc = self.doc;
        let mut list = anim_display::DisplayList::new(Size::new(doc.width, doc.height), None);
        if let Some(symbol) = doc.library.find_symbol(name) {
            let mut state = State { stack: vec![symbol.name.as_str()] };
            self.timeline_nodes(&symbol.timeline, Time { frame, clock: frame }, &mut state, &mut list.nodes);
        }
        list
    }

    /// Stage bounds of a symbol's content at `frame`.
    #[must_use]
    pub fn symbol_bounds(&self, name: &str, frame: u32) -> Option<Rect> {
        let symbol = self.doc.library.find_symbol(name)?;
        anim_scene::timeline_bounds(&symbol.timeline, frame, &self.doc.library)
    }

    /// Onion skin frames around `frame`: up to `before` frames before and `after` frames after it (never the
    /// frame itself, never outside the scene), each with an opacity that fades with distance: the nearest frame is
    /// 0.5, the farthest approaches 0. Ordered from the earliest frame to the latest.
    #[must_use]
    pub fn onion(&mut self, scene: usize, frame: u32, before: u32, after: u32) -> Vec<(anim_display::DisplayList, f32)> {
        let total = self.scene_frames(scene);
        let mut out = Vec::new();
        for d in (1..=before).rev() {
            if let Some(f) = frame.checked_sub(d).filter(|f| *f < total) {
                out.push((self.frame(scene, f), onion_alpha(d, before)));
            }
        }
        for d in 1..=after {
            if let Some(f) = frame.checked_add(d).filter(|f| *f < total) {
                out.push((self.frame(scene, f), onion_alpha(d, after)));
            }
        }
        out
    }

    // ---- timelines and layers ----

    /// Appends the layers of `timeline` at `time` to `out`.
    fn timeline_nodes(&mut self, timeline: &'a Timeline, time: Time, state: &mut State<'a>, out: &mut Vec<Node>) {
        let tree = self.trees.take(timeline).unwrap_or_else(|| timeline.layer_tree());
        self.emit_layers(timeline, &tree, time, state, out);
        self.trees.insert(timeline, tree);
    }

    /// Appends the layers bottom to top. `nodes` is top to bottom, as the tree lists them.
    fn emit_layers(&mut self, timeline: &'a Timeline, nodes: &[LayerNode], time: Time, state: &mut State<'a>, out: &mut Vec<Node>) {
        for node in nodes.iter().rev() {
            let Some(layer) = timeline.layers.get(node.index) else { continue };
            let shown = layer.visible || self.options.include_hidden;
            match layer.kind {
                LayerKind::Camera => {}
                LayerKind::Folder => {
                    if !shown {
                        continue;
                    }
                    let mut children = Vec::new();
                    self.emit_layers(timeline, &node.children, time, state, &mut children);
                    if !children.is_empty() {
                        out.push(Node::Group(Group { children, ..Group::default() }));
                    }
                }
                LayerKind::Mask => {
                    if !shown {
                        // A hidden mask does not mask: the layers it covers show in full.
                        self.emit_layers(timeline, &node.children, time, state, out);
                        continue;
                    }
                    let mut mask = Vec::new();
                    self.layer_nodes(timeline, node.index, time, state, &mut mask);
                    let mut children = Vec::new();
                    self.emit_layers(timeline, &node.children, time, state, &mut children);
                    if !children.is_empty() {
                        out.push(Node::Group(Group { mask: Some(mask), children, ..Group::default() }));
                    }
                }
                LayerKind::Guide => {
                    if self.options.include_guides && shown {
                        self.layer_nodes(timeline, node.index, time, state, out);
                    }
                    // Layers guided by a motion guide are ordinary layers.
                    self.emit_layers(timeline, &node.children, time, state, out);
                }
                LayerKind::Normal | LayerKind::Guided | LayerKind::Masked => {
                    if shown {
                        self.layer_nodes(timeline, node.index, time, state, out);
                    }
                    self.emit_layers(timeline, &node.children, time, state, out);
                }
            }
        }
    }

    fn layer_nodes(&mut self, timeline: &'a Timeline, layer_index: usize, time: Time, state: &mut State<'a>, out: &mut Vec<Node>) {
        let Some(layer) = timeline.layers.get(layer_index) else { return };
        let Some(keyframe) = layer.keyframe_at(time.frame) else { return };
        let top = state.stack.is_empty();
        let (keyframe, items) = if layer.armature.is_some() {
            let rest = layer.rest_keyframe().unwrap_or(keyframe);
            let items = crate::armature::posed_elements(layer, time.frame)
                .into_iter()
                .map(|posed| match posed {
                    crate::armature::Posed::Element { element, matrix } => Item::Placed(Placed { matrix, ..Placed::hold(element) }),
                    crate::armature::Posed::Shape(shape) => Item::Morphed { shape, matrix: Affine::IDENTITY },
                })
                .collect();
            (rest, items)
        } else {
            let guided = guided_tween(timeline, layer, keyframe, time.frame);
            (keyframe, self.layer_items(timeline, layer, keyframe, layer_index, time.frame, guided))
        };
        let since = time.frame.saturating_sub(keyframe.index);
        for (i, item) in items.into_iter().enumerate() {
            let node_key = top.then(|| element_key(layer_index, i));
            out.extend(self.item_node(item, node_key, since, time, state, 0));
        }
    }

    /// The elements of `keyframe` as they are at `frame`, tweened when the keyframe starts a tween.
    fn layer_items(
        &mut self,
        timeline: &'a Timeline,
        layer: &'a Layer,
        keyframe: &'a Frame,
        layer_index: usize,
        frame: u32,
        guided: Option<GuidedTween<'a>>,
    ) -> Vec<Item<'a>> {
        let hold = || keyframe.elements.iter().map(|e| Item::Placed(Placed::hold(e))).collect::<Vec<_>>();
        if let Some(g) = guided
            && let Some(guide) = self.guide(g.guide_frame)
        {
            return tween::classic_along(g.start, g.end, g.t, Some(guide)).into_iter().map(Item::Placed).collect();
        }
        if frame <= keyframe.index {
            return hold();
        }
        match keyframe.tween_type {
            TweenType::Classic => {
                let Some(next) = layer.next_keyframe_after(keyframe.index) else { return hold() };
                let span = next.index.saturating_sub(keyframe.index);
                if span == 0 {
                    return hold();
                }
                let t = f64::from(frame.saturating_sub(keyframe.index)) / f64::from(span);
                tween::classic(keyframe, next, t).into_iter().map(Item::Placed).collect()
            }
            TweenType::Shape => {
                let Some(next) = layer.next_keyframe_after(keyframe.index) else { return hold() };
                self.shape_tween(keyframe, next, std::ptr::from_ref(timeline).addr(), layer_index, frame)
            }
            TweenType::MotionObject => {
                let Some(tracks) = self.motion_tracks(keyframe) else { return hold() };
                let steps = keyframe.duration.saturating_sub(1);
                if steps == 0 {
                    return hold();
                }
                let t = f64::from(frame.saturating_sub(keyframe.index).min(steps)) / f64::from(steps);
                let delta = tracks.delta_at(t);
                keyframe
                    .elements
                    .iter()
                    .map(|e| {
                        let mut placed = Placed::hold(e);
                        placed.matrix = delta.apply(placed.matrix);
                        Item::Placed(placed)
                    })
                    .collect()
            }
            TweenType::None => hold(),
        }
    }

    fn shape_tween(&mut self, keyframe: &'a Frame, next: &'a Frame, scope: usize, layer_index: usize, frame: u32) -> Vec<Item<'a>> {
        let span = next.index.saturating_sub(keyframe.index);
        let end_shapes: Vec<_> = next.elements.iter().filter_map(|e| if let Element::Shape(s) = e { Some(s) } else { None }).collect();
        if span == 0 {
            return keyframe.elements.iter().map(|e| Item::Placed(Placed::hold(e))).collect();
        }
        let t = f64::from(frame.saturating_sub(keyframe.index)) / f64::from(span);
        let eased = ease::for_target(keyframe, EaseTarget::All, t);
        let mut ordinal = 0usize;
        let mut items = Vec::with_capacity(keyframe.elements.len());
        for element in &keyframe.elements {
            let Element::Shape(from) = element else {
                items.push(Item::Placed(Placed::hold(element)));
                continue;
            };
            let this = ordinal;
            ordinal = ordinal.saturating_add(1);
            let Some(to) = end_shapes.get(this) else {
                items.push(Item::Placed(Placed::hold(element)));
                continue;
            };
            if self.plans.len() >= MAX_PLANS {
                self.plans.clear();
            }
            let plan_key = (scope, layer_index, keyframe.index, this);
            let plan = self.plans.entry(plan_key).or_insert_with(|| MorphPlan::new(&from.shape, &to.shape, &morph_options(keyframe)));
            let shape = plan.at(eased);
            let matrix = anim_morph::style::lerp_affine(from.matrix, to.matrix, eased);
            items.push(Item::Morphed { shape, matrix });
        }
        items
    }

    // ---- elements ----

    fn item_node(&mut self, item: Item<'a>, key: Option<u64>, since: u32, time: Time, state: &mut State<'a>, group_depth: usize) -> Option<Node> {
        match item {
            Item::Morphed { shape, matrix } => {
                let children = self.display_of(&shape);
                shape_group(children, matrix, key)
            }
            Item::Placed(placed) => self.placed_node(placed, key, since, time, state, group_depth),
        }
    }

    fn placed_node(&mut self, placed: Placed<'a>, key: Option<u64>, since: u32, time: Time, state: &mut State<'a>, group_depth: usize) -> Option<Node> {
        match placed.element {
            Element::Shape(s) => {
                let children = self.stored_display(&s.shape);
                shape_group(children, placed.matrix, key)
            }
            Element::SymbolInstance(instance) => {
                if !instance.visible {
                    return None;
                }
                let children = self.symbol_nodes(instance, since, time, state);
                Some(Node::Group(Group {
                    transform: placed.matrix,
                    color: placed.color,
                    blend: instance.blend_mode,
                    filters: placed.filters,
                    children,
                    key,
                    mask: None,
                }))
            }
            Element::Bitmap(b) => {
                let children = self.bitmap_nodes(&b.library_item_name);
                (!children.is_empty()).then(|| Node::Group(Group { transform: placed.matrix, children, key, ..Group::default() }))
            }
            Element::Text(t) => {
                let children = text_nodes(self.text_engine_mut(), t);
                (!children.is_empty()).then(|| Node::Group(Group { transform: placed.matrix, children, key, ..Group::default() }))
            }
            Element::Group(g) => {
                if group_depth >= MAX_GROUP_DEPTH {
                    return None;
                }
                let next_depth = group_depth.saturating_add(1);
                let mut children = Vec::new();
                for member in &g.members {
                    if let Some(node) = self.placed_node(Placed::hold(member), None, since, time, state, next_depth) {
                        children.push(node);
                    }
                }
                (!children.is_empty()).then(|| Node::Group(Group { transform: placed.matrix, children, key, ..Group::default() }))
            }
            Element::Unknown(_) => None,
        }
    }

    /// The property tracks of a motion-object keyframe, parsed once per evaluator; `None` when there are none.
    fn motion_tracks(&mut self, frame: &'a Frame) -> Option<&MotionTracks> {
        self.motion.get_or_insert_with(frame, || frame.motion_object_xml.as_deref().and_then(MotionTracks::parse)).as_ref()
    }

    /// The motion guide a keyframe holds, measured once per evaluator; `None` when it has no stroke.
    fn guide(&mut self, frame: &'a Frame) -> Option<&Guide> {
        self.guides.get_or_insert_with(frame, || Guide::from_keyframe(frame)).as_ref()
    }

    /// The nodes of a shape.
    fn display_of(&mut self, shape: &EdgeShape) -> Vec<Node> {
        let mut resolved: HashMap<String, ImageId> = HashMap::new();
        for path in shape_bitmap_paths(shape) {
            if !resolved.contains_key(path)
                && let Some(id) = self.image_for(path)
            {
                resolved.insert(path.to_owned(), id);
            }
        }
        shape.to_display(&|path: &str| resolved.get(path).copied())
    }

    /// The nodes of a shape stored in the document, converted once per evaluator.
    fn stored_display(&mut self, shape: &'a EdgeShape) -> Vec<Node> {
        if let Some(nodes) = self.shapes.get(shape) {
            return nodes.clone();
        }
        let nodes = self.display_of(shape);
        self.shapes.insert(shape, nodes.clone());
        nodes
    }

    fn bitmap_nodes(&mut self, name: &str) -> Vec<Node> {
        let doc = self.doc;
        let Some(item) = doc.library.find_bitmap(name) else { return Vec::new() };
        if let Some(image) = self.image_for(name) {
            return vec![Node::Bitmap(Bitmap { transform: Affine::IDENTITY, image, smoothing: item.smoothing })];
        }
        // No pixels: a grey placeholder of the right size keeps the layout visible.
        item.size
            .map(|[w, h]| Rect::new(0.0, 0.0, w, h))
            .filter(|r| r.area() > 0.0)
            .map(|r| vec![Node::Fill(Fill { transform: Affine::IDENTITY, path: r.to_path(0.1), rule: FillRule::NonZero, paint: Paint::Solid(MISSING_BITMAP) })])
            .unwrap_or_default()
    }

    /// The image of library bitmap `name`, decoding it on first use.
    fn image_for(&mut self, name: &str) -> Option<ImageId> {
        if let Some(cached) = self.image_ids.get(name) {
            return *cached;
        }
        let doc = self.doc;
        let decoded = doc
            .library
            .find_bitmap(name)
            .and_then(|item| item.media)
            .and_then(|id| self.media.and_then(|m| m.get(id)))
            .and_then(|bytes| self.provider.decode(name, bytes));
        let id = decoded.map(|image| self.images.insert(image));
        self.image_ids.insert(name.to_owned(), id);
        id
    }

    // ---- symbols ----

    /// The content of the symbol an instance shows.
    ///
    /// - Graphic: locked to the parent. `local = first_frame + since` where `since` is the number of frames since
    ///   the instance's keyframe started; loop wraps, play once holds the last frame, single frame holds
    ///   `first_frame`.
    /// - Movie clip: `first_frame` (authoring), or with `advance_movie_clips` the global frame, wrapped.
    /// - Button: the Up frame (label `_up`, else frame 0).
    fn symbol_nodes(&mut self, instance: &'a SymbolInstance, since: u32, time: Time, state: &mut State<'a>) -> Vec<Node> {
        let doc = self.doc;
        let Some(symbol) = doc.library.find_symbol(&instance.library_item_name) else { return Vec::new() };
        if state.stack.len() >= self.options.max_symbol_depth || state.stack.contains(&symbol.name.as_str()) {
            return Vec::new();
        }
        let length = symbol.timeline.frame_count().max(1);
        let last = length.saturating_sub(1);
        let local = match instance.effective_kind(&doc.library) {
            SymbolKind::Graphic => match instance.effective_loop_mode() {
                LoopMode::Loop => instance.first_frame.saturating_add(since).checked_rem(length).unwrap_or(0),
                LoopMode::PlayOnce => instance.first_frame.saturating_add(since).min(last),
                LoopMode::SingleFrame => instance.first_frame.min(last),
            },
            SymbolKind::MovieClip => {
                if self.options.advance_movie_clips {
                    instance.first_frame.saturating_add(time.clock).checked_rem(length).unwrap_or(0)
                } else {
                    instance.first_frame.min(last)
                }
            }
            SymbolKind::Button => symbol.timeline.labels().iter().find(|(_, l)| *l == "_up").map_or(0, |(f, _)| *f).min(last),
        };
        state.stack.push(symbol.name.as_str());
        let mut nodes = Vec::new();
        self.timeline_nodes(&symbol.timeline, Time { frame: local, clock: time.clock }, state, &mut nodes);
        state.stack.pop();
        nodes
    }
}

/// A shape's nodes under its matrix; nothing for a shape without paint.
fn shape_group(children: Vec<Node>, transform: Affine, key: Option<u64>) -> Option<Node> {
    (!children.is_empty()).then(|| Node::Group(Group { transform, children, key, ..Group::default() }))
}

/// A classic tween that follows a motion guide, evaluated at progress `t`.
struct GuidedTween<'a> {
    start: &'a Frame,
    end: &'a Frame,
    t: f64,
    /// The keyframe of the guide layer that holds the path.
    guide_frame: &'a Frame,
}

/// The guided classic tween that decides what the layer shows at `frame`, if its parent is a guide layer with a
/// stroke. That is the tween starting at `keyframe`, or, on a plain keyframe that ends a guided tween with the same
/// number of elements, the finished tween (so an oriented element does not pop at the last keyframe).
fn guided_tween<'a>(timeline: &'a Timeline, layer: &'a Layer, keyframe: &'a Frame, frame: u32) -> Option<GuidedTween<'a>> {
    let guide_layer = timeline.layers.get(layer.parent_layer_index?).filter(|p| p.kind == LayerKind::Guide)?;
    let (start, end) = if matches!(keyframe.tween_type, TweenType::Classic) {
        (keyframe, layer.next_keyframe_after(keyframe.index)?)
    } else {
        let prev = layer.frames.get(layer.keyframe_position_at(frame)?.checked_sub(1)?)?;
        (frame == keyframe.index && matches!(prev.tween_type, TweenType::Classic) && prev.elements.len() == keyframe.elements.len())
            .then_some((prev, keyframe))?
    };
    let span = end.index.checked_sub(start.index).filter(|s| *s > 0)?;
    let t = f64::from(frame.saturating_sub(start.index)) / f64::from(span);
    let guide_frame = guide_layer.keyframe_at(frame).or_else(|| guide_layer.frames.first())?;
    Some(GuidedTween { start, end, t, guide_frame })
}

fn onion_alpha(distance: u32, count: u32) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let fade = 1.0 - f64::from(distance.saturating_sub(1)) / f64::from(count);
    anim_core::num::f64_to_f32(0.5 * fade)
}

fn morph_options(keyframe: &Frame) -> MorphOptions {
    let mut hints: Vec<_> = keyframe.shape_hints.iter().collect();
    hints.sort_by_key(|h| h.id);
    MorphOptions { mode: MorphMode::Distributive, hints: hints.into_iter().map(|h| anim_morph::ShapeHint::new(h.start, h.end)).collect() }
}

/// Wraps a display list's nodes in one group with the given opacity, for drawing onion skins.
pub fn fade(list: &mut anim_display::DisplayList, alpha: f32) {
    let children = std::mem::take(&mut list.nodes);
    list.nodes = vec![Node::Group(Group { color: ColorTransform::alpha(alpha), children, ..Group::default() })];
}
