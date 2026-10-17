//! Editing helpers: pure functions on `&mut Document` that tools and commands call.
//!
//! Every function validates first and mutates second, so an `Err` leaves the document as it was (apart from
//! normalising the order of a layer's keyframes, which is harmless). Frame indices are 0-based.

use std::collections::HashMap;

use anim_core::{Affine, Point, Rect, Rgba8, Vec2};
use anim_shape::{FillStyle, StrokeStyle};

use crate::bounds::elements_bounds;
use crate::document::{Document, LAYER_COLORS, TimelineId};
use crate::element::{Element, LoopMode, SymbolInstance, SymbolKind};
use crate::error::SceneError;
use crate::library::{FolderItem, Library, LibraryItem, Linkage, Symbol, validate_item_name};
use crate::timeline::{Frame, KEY_MODE_CLASSIC_TWEEN, KEY_MODE_SHAPE_TWEEN, LabelType, Layer, LayerKind, Timeline, TweenType};

/// Most frames a timeline may grow to through the editing helpers (Animate's own cap is 16 000).
pub const MAX_FRAMES: u32 = 100_000;
/// Most layers a timeline may hold through the editing helpers.
pub const MAX_LAYERS: usize = 10_000;

/// Where the registration point of a new symbol goes: the stage origin, or one of the nine points of the grid in the
/// Convert to Symbol dialog, taken on the bounds of the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Registration {
    /// The stage origin: children keep their coordinates.
    #[default]
    Origin,
    /// The centre of the selection's bounds.
    Center,
    /// The top-left corner of the selection's bounds.
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Registration {
    /// The nine grid points in reading order (the dialog's 3 by 3 grid).
    pub const GRID: [Self; 9] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::MiddleLeft,
        Self::Center,
        Self::MiddleRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    /// Lowercase name for commands: `origin`, `top_left`, `top_center`, ..., `center`, ..., `bottom_right`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Origin => "origin",
            Self::Center => "center",
            Self::TopLeft => "top_left",
            Self::TopCenter => "top_center",
            Self::TopRight => "top_right",
            Self::MiddleLeft => "middle_left",
            Self::MiddleRight => "middle_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomCenter => "bottom_center",
            Self::BottomRight => "bottom_right",
        }
    }

    /// Parses [`Registration::name`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::GRID.into_iter().chain([Self::Origin]).find(|r| r.name() == name)
    }

    /// The label a person reads: `Top left`, `Center`, ...
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Origin => "Stage origin",
            Self::Center => "Center",
            Self::TopLeft => "Top left",
            Self::TopCenter => "Top center",
            Self::TopRight => "Top right",
            Self::MiddleLeft => "Middle left",
            Self::MiddleRight => "Middle right",
            Self::BottomLeft => "Bottom left",
            Self::BottomCenter => "Bottom center",
            Self::BottomRight => "Bottom right",
        }
    }

    /// The point of `bounds` this setting names; `None` for [`Registration::Origin`], which is not on the bounds.
    #[must_use]
    pub const fn point(self, bounds: Rect) -> Option<Point> {
        let centre = bounds.center();
        let (x, y) = match self {
            Self::Origin => return None,
            Self::TopLeft => (bounds.x0, bounds.y0),
            Self::TopCenter => (centre.x, bounds.y0),
            Self::TopRight => (bounds.x1, bounds.y0),
            Self::MiddleLeft => (bounds.x0, centre.y),
            Self::Center => (centre.x, centre.y),
            Self::MiddleRight => (bounds.x1, centre.y),
            Self::BottomLeft => (bounds.x0, bounds.y1),
            Self::BottomCenter => (centre.x, bounds.y1),
            Self::BottomRight => (bounds.x1, bounds.y1),
        };
        Some(Point::new(x, y))
    }
}

fn timeline_mut<'a>(doc: &'a mut Document, id: &TimelineId) -> Result<&'a mut Timeline, SceneError> {
    doc.timeline_mut(id).ok_or_else(|| SceneError::NoSuchTimeline(id.clone()))
}

fn layer_mut<'a>(doc: &'a mut Document, id: &TimelineId, layer: usize) -> Result<&'a mut Layer, SceneError> {
    let tl = timeline_mut(doc, id)?;
    let count = tl.layers.len();
    tl.layers.get_mut(layer).ok_or(SceneError::NoSuchLayer { layer, count })
}

/// The keyframe covering `frame` on `layer`, mutably.
///
/// # Errors
/// Unknown timeline, layer, or no keyframe covering the frame.
pub fn keyframe_mut<'a>(doc: &'a mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<&'a mut Frame, SceneError> {
    let l = layer_mut(doc, id, layer)?;
    l.keyframe_at_mut(frame).ok_or(SceneError::NoFrame { layer, frame })
}

const fn check_frame(frame: u32) -> Result<(), SceneError> {
    if frame >= MAX_FRAMES { Err(SceneError::LimitExceeded("frame count")) } else { Ok(()) }
}

// ---------------------------------------------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------------------------------------------

/// Insert Frame (F5): adds `count` frames at `at`, lengthening the keyframe span that covers it and pushing later
/// keyframes right. Past the end of the layer the last keyframe is extended to reach `at + count`.
///
/// # Errors
/// Unknown timeline or layer, `count` of zero, or the frame limit.
pub fn insert_frames(doc: &mut Document, id: &TimelineId, layer: usize, at: u32, count: u32) -> Result<(), SceneError> {
    if count == 0 {
        return Err(SceneError::InvalidRange("count must be at least 1"));
    }
    check_frame(at.saturating_add(count))?;
    let l = layer_mut(doc, id, layer)?;
    l.sort_frames();
    if let Some(pos) = l.keyframe_position_at(at) {
        let mut end = 0;
        if let Some(k) = l.frames.get_mut(pos) {
            end = k.end();
            k.duration = k.duration.saturating_add(count);
        }
        for f in l.frames.iter_mut().skip(pos.saturating_add(1)).filter(|f| f.index >= end) {
            f.index = f.index.saturating_add(count);
        }
    } else if at >= l.frame_count() {
        let target = at.saturating_add(count);
        if let Some(last) = l.frames.last_mut() {
            last.duration = target.saturating_sub(last.index).max(1);
        } else {
            l.frames.push(Frame::blank(0, target));
        }
    } else {
        for f in l.frames.iter_mut().filter(|f| f.index >= at) {
            f.index = f.index.saturating_add(count);
        }
    }
    Ok(())
}

/// Remove Frames (Shift+F5): deletes `count` frames starting at `at`.
///
/// Later keyframes move left. Keyframes that lie wholly inside the range disappear; spans that are cut keep their
/// content. Tweens that lose their end keyframe are cleared.
///
/// # Errors
/// Unknown timeline or layer, or `count` of zero.
pub fn remove_frames(doc: &mut Document, id: &TimelineId, layer: usize, at: u32, count: u32) -> Result<(), SceneError> {
    if count == 0 {
        return Err(SceneError::InvalidRange("count must be at least 1"));
    }
    let l = layer_mut(doc, id, layer)?;
    l.sort_frames();
    let range_end = at.saturating_add(count);
    let mut kept = Vec::with_capacity(l.frames.len());
    for mut f in std::mem::take(&mut l.frames) {
        let (start, end) = (f.index, f.end());
        let overlap = end.min(range_end).saturating_sub(start.max(at));
        let duration = f.duration.saturating_sub(overlap);
        if duration == 0 {
            continue;
        }
        f.duration = duration;
        f.index = if start < at {
            start
        } else if start >= range_end {
            start.saturating_sub(count)
        } else {
            at
        };
        kept.push(f);
    }
    l.frames = kept;
    clear_dangling_tweens(l);
    Ok(())
}

fn clear_dangling_tweens(l: &mut Layer) {
    let n = l.frames.len();
    for (i, f) in l.frames.iter_mut().enumerate() {
        if f.is_tweened() && f.tween_type != TweenType::MotionObject && i.saturating_add(1) >= n {
            f.clear_tween();
        }
    }
}

/// Insert Keyframe (F6): makes `frame` a keyframe holding a copy of what is shown there.
///
/// # Errors
/// [`SceneError::AlreadyKeyframe`] when one starts at `frame`; unknown timeline or layer; the frame limit.
pub fn insert_keyframe(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<(), SceneError> {
    make_keyframe(doc, id, layer, frame, false)
}

/// Insert Blank Keyframe (F7): makes `frame` a keyframe with nothing in it.
///
/// # Errors
/// As [`insert_keyframe`].
pub fn insert_blank_keyframe(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<(), SceneError> {
    make_keyframe(doc, id, layer, frame, true)
}

fn make_keyframe(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, blank: bool) -> Result<(), SceneError> {
    check_frame(frame)?;
    let l = layer_mut(doc, id, layer)?;
    l.sort_frames();
    if let Some(pos) = l.keyframe_position_at(frame) {
        let Some(k) = l.frames.get_mut(pos) else { return Err(SceneError::NoFrame { layer, frame }) };
        if k.index == frame {
            return Err(SceneError::AlreadyKeyframe { layer, frame });
        }
        let end = k.end();
        let mut new = if blank {
            Frame::blank(frame, end.saturating_sub(frame))
        } else {
            let mut copy = k.clone();
            copy.index = frame;
            copy.duration = end.saturating_sub(frame);
            copy.label = None;
            copy.label_type = None;
            copy.sound = None;
            copy.actionscript = None;
            copy
        };
        k.duration = frame.saturating_sub(k.index);
        if k.tween_type == TweenType::Shape {
            k.morph_shape_xml = None;
            new.morph_shape_xml = None;
        }
        l.frames.insert(pos.saturating_add(1), new);
        return Ok(());
    }
    let insert_at = l.frames.partition_point(|f| f.index <= frame);
    let previous = insert_at.checked_sub(1).and_then(|p| l.frames.get(p));
    let elements = match (blank, previous) {
        (false, Some(p)) => p.elements.clone(),
        _ => Vec::new(),
    };
    if frame >= l.frame_count() {
        match l.frames.last_mut() {
            Some(last) => {
                last.duration = frame.saturating_sub(last.index).max(1);
            }
            None if frame > 0 => l.frames.push(Frame::blank(0, frame)),
            None => {}
        }
        let at = l.frames.len();
        l.frames.insert(at, Frame::new(frame, 1, elements));
    } else {
        let duration = l.frames.get(insert_at).map_or(1, |next| next.index.saturating_sub(frame).max(1));
        l.frames.insert(insert_at, Frame::new(frame, duration, elements));
    }
    Ok(())
}

/// Clear Keyframe (Shift+F6): turns the keyframe starting at `frame` into ordinary frames of the previous keyframe,
/// or blanks it when nothing precedes it.
///
/// # Errors
/// Unknown timeline or layer, or no keyframe starting at `frame`.
pub fn clear_keyframe(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<(), SceneError> {
    let l = layer_mut(doc, id, layer)?;
    l.sort_frames();
    let pos = l.keyframe_position_at(frame).filter(|&p| l.frames.get(p).is_some_and(|f| f.index == frame)).ok_or(SceneError::NoFrame { layer, frame })?;
    let adjacent_previous = pos.checked_sub(1).and_then(|p| l.frames.get(p)).is_some_and(|prev| prev.end() == frame);
    if adjacent_previous {
        let removed = l.frames.remove(pos);
        if let Some(prev) = pos.checked_sub(1).and_then(|p| l.frames.get_mut(p)) {
            prev.duration = prev.duration.saturating_add(removed.duration);
        }
    } else if let Some(k) = l.frames.get_mut(pos) {
        k.elements.clear();
        k.clear_tween();
        k.label = None;
        k.label_type = None;
        k.sound = None;
        k.actionscript = None;
    }
    clear_dangling_tweens(l);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Tweens
// ---------------------------------------------------------------------------------------------------------------

fn tween_positions(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<(usize, usize), SceneError> {
    let l = layer_mut(doc, id, layer)?;
    l.sort_frames();
    let pos = l.keyframe_position_at(frame).ok_or(SceneError::NoFrame { layer, frame })?;
    let start_index = l.frames.get(pos).map_or(frame, |f| f.index);
    let next = pos.saturating_add(1);
    if l.frames.get(next).is_none() {
        return Err(SceneError::NoEndKeyframe { layer, frame: start_index });
    }
    Ok((pos, next))
}

/// Create Classic Tween: the keyframe covering `frame` tweens to the next keyframe. The content must be a single
/// symbol instance, group or text field (convert raw shapes first with [`convert_to_symbol`]).
///
/// # Errors
/// [`SceneError::NoEndKeyframe`] without a following keyframe, [`SceneError::NotTweenable`] for unsuitable content.
pub fn create_classic_tween(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<(), SceneError> {
    let (pos, _) = tween_positions(doc, id, layer, frame)?;
    let l = layer_mut(doc, id, layer)?;
    let Some(k) = l.frames.get_mut(pos) else { return Err(SceneError::NoFrame { layer, frame }) };
    match k.elements.as_slice() {
        [Element::SymbolInstance(_) | Element::Group(_) | Element::Text(_)] => {}
        [Element::Shape(_)] => return Err(SceneError::NotTweenable("convert the shape to a symbol first")),
        [Element::Bitmap(_)] => return Err(SceneError::NotTweenable("convert the bitmap to a symbol first")),
        [] => return Err(SceneError::NotTweenable("the keyframe is empty")),
        [Element::Unknown(_)] => return Err(SceneError::NotTweenable("unknown content cannot be tweened")),
        _ => return Err(SceneError::NotTweenable("a classic tween needs exactly one object per layer")),
    }
    k.clear_tween();
    k.tween_type = TweenType::Classic;
    k.key_mode = KEY_MODE_CLASSIC_TWEEN;
    k.motion_tween_snap = true;
    Ok(())
}

/// Create Shape Tween: both the keyframe covering `frame` and the next one must hold only shapes.
///
/// # Errors
/// [`SceneError::NoEndKeyframe`], [`SceneError::NotTweenable`].
pub fn create_shape_tween(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<(), SceneError> {
    let (pos, next) = tween_positions(doc, id, layer, frame)?;
    let l = layer_mut(doc, id, layer)?;
    let only_shapes = |f: Option<&Frame>| f.is_some_and(|f| !f.elements.is_empty() && f.elements.iter().all(|e| matches!(e, Element::Shape(_))));
    if !only_shapes(l.frames.get(pos)) {
        return Err(SceneError::NotTweenable("a shape tween needs shapes in the start keyframe"));
    }
    if !only_shapes(l.frames.get(next)) {
        return Err(SceneError::NotTweenable("a shape tween needs shapes in the end keyframe"));
    }
    let Some(k) = l.frames.get_mut(pos) else { return Err(SceneError::NoFrame { layer, frame }) };
    k.clear_tween();
    k.tween_type = TweenType::Shape;
    k.key_mode = KEY_MODE_SHAPE_TWEEN;
    Ok(())
}

/// Remove Tween: clears every tween setting of the keyframe covering `frame`.
///
/// # Errors
/// Unknown timeline or layer, or no keyframe at `frame`.
pub fn remove_tween(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<(), SceneError> {
    keyframe_mut(doc, id, layer, frame)?.clear_tween();
    Ok(())
}

/// Sets the classic ease acceleration (-100 ..= 100) of the keyframe covering `frame`.
///
/// # Errors
/// [`SceneError::InvalidRange`] outside -100 ..= 100, or no such keyframe.
pub fn set_classic_ease(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, acceleration: i32) -> Result<(), SceneError> {
    if !(-100..=100).contains(&acceleration) {
        return Err(SceneError::InvalidRange("acceleration must be between -100 and 100"));
    }
    keyframe_mut(doc, id, layer, frame)?.acceleration = acceleration;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Elements
// ---------------------------------------------------------------------------------------------------------------

/// Adds an element to the keyframe covering `frame` (creating a blank keyframe when nothing covers it), on top of
/// the stack, and returns its index in the keyframe.
///
/// # Errors
/// Unknown timeline or layer; the frame limit.
pub fn add_element(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, element: Element) -> Result<usize, SceneError> {
    if layer_mut(doc, id, layer)?.keyframe_at(frame).is_none() {
        insert_blank_keyframe(doc, id, layer, frame)?;
    }
    let k = keyframe_mut(doc, id, layer, frame)?;
    k.elements.push(element);
    Ok(k.elements.len().saturating_sub(1))
}

/// Places an instance of the library symbol `symbol` at `matrix`.
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `symbol` is not a symbol, [`SceneError::Cycle`] when the instance would make a
/// symbol contain itself, plus the errors of [`add_element`].
pub fn add_symbol_instance(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, symbol: &str, matrix: Affine) -> Result<usize, SceneError> {
    let Some(sym) = doc.library.find_symbol(symbol) else { return Err(SceneError::NoSuchItem(symbol.to_owned())) };
    let kind = sym.kind;
    if let TimelineId::Symbol(container) = id
        && doc.symbol_reaches(symbol, container)
    {
        return Err(SceneError::Cycle(container.clone()));
    }
    let mut inst = SymbolInstance::new(symbol, matrix);
    inst.symbol_type = Some(kind);
    if kind == SymbolKind::Graphic {
        inst.loop_mode = Some(LoopMode::Loop);
    }
    add_element(doc, id, layer, frame, Element::SymbolInstance(Box::new(inst)))
}

/// Removes the listed elements (indices into the keyframe) and returns them in their original order.
///
/// # Errors
/// [`SceneError::NoSuchElement`] for an index out of range, [`SceneError::EmptySelection`] for an empty list.
pub fn remove_elements(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, indices: &[usize]) -> Result<Vec<Element>, SceneError> {
    let k = keyframe_mut(doc, id, layer, frame)?;
    let sorted = checked_selection(indices, k.elements.len())?;
    let mut out = Vec::with_capacity(sorted.len());
    for &i in sorted.iter().rev() {
        out.push(k.elements.remove(i));
    }
    out.reverse();
    Ok(out)
}

fn checked_selection(indices: &[usize], count: usize) -> Result<Vec<usize>, SceneError> {
    if indices.is_empty() {
        return Err(SceneError::EmptySelection);
    }
    let mut sorted = indices.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if let Some(&bad) = sorted.iter().find(|&&i| i >= count) {
        return Err(SceneError::NoSuchElement { element: bad, count });
    }
    Ok(sorted)
}

/// What [`convert_to_symbol`] makes.
#[derive(Debug, Clone, Copy)]
pub struct SymbolSpec<'a> {
    /// Name of the new symbol; with a `folder` it is the name inside that folder.
    pub name: &'a str,
    pub kind: SymbolKind,
    pub registration: Registration,
    /// Library folder (`a/b`) to put the symbol in; created when missing. `None` or empty is the library root.
    pub folder: Option<&'a str>,
    /// Linkage to set (Export for `ActionScript`, class, base class); `None` leaves the symbol unexported.
    pub linkage: Option<&'a Linkage>,
    /// Enable 9-slice scaling, with guides in the middle third of the symbol.
    pub scale9: bool,
}

impl<'a> SymbolSpec<'a> {
    /// A symbol in the library root, not exported and without 9-slice guides.
    #[must_use]
    pub const fn new(name: &'a str, kind: SymbolKind, registration: Registration) -> Self {
        Self { name, kind, registration, folder: None, linkage: None, scale9: false }
    }

    /// The full library path the symbol gets.
    #[must_use]
    pub fn full_name(&self) -> String {
        self.folder.map(|f| f.trim_matches('/')).filter(|f| !f.is_empty()).map_or_else(|| self.name.to_owned(), |folder| format!("{folder}/{}", self.name))
    }
}

/// Convert to Symbol (F8): moves the selected elements of the keyframe covering `frame` into a new library symbol.
///
/// An instance of the symbol takes their place, at the position of the lowest selected element. Returns the index
/// of the new instance in the keyframe.
///
/// # Errors
/// Name errors ([`SceneError::InvalidName`], [`SceneError::NameTaken`]), selection errors, unknown timeline, layer or
/// keyframe.
pub fn convert_to_symbol(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, indices: &[usize], spec: &SymbolSpec<'_>) -> Result<usize, SceneError> {
    let (kind, registration) = (spec.kind, spec.registration);
    let full = spec.full_name();
    let name = full.as_str();
    validate_item_name(spec.name)?;
    validate_item_name(name)?;
    if doc.library.find_index(name).is_some() {
        return Err(SceneError::NameTaken(name.to_owned()));
    }
    if spec.scale9 && kind == SymbolKind::Graphic {
        return Err(SceneError::InvalidRange("graphic symbols cannot use 9-slice scaling; make it a movie clip or a button"));
    }
    if let Some(linkage) = spec.linkage {
        crate::symbols::validate_linkage(kind, linkage)?;
    }
    let (sorted, offset, bounds) = {
        let tl = doc.timeline(id).ok_or_else(|| SceneError::NoSuchTimeline(id.clone()))?;
        let count = tl.layers.len();
        let l = tl.layers.get(layer).ok_or(SceneError::NoSuchLayer { layer, count })?;
        let k = l.keyframe_at(frame).ok_or(SceneError::NoFrame { layer, frame })?;
        let sorted = checked_selection(indices, k.elements.len())?;
        let selected = sorted.iter().filter_map(|&i| k.elements.get(i));
        let bounds = elements_bounds(selected, &doc.library);
        let offset = bounds.and_then(|r| registration.point(r)).map_or(Vec2::ZERO, Point::to_vec2);
        (sorted, offset, bounds)
    };
    let mut children = remove_elements(doc, id, layer, frame, &sorted)?;
    let shift = Affine::translate(-offset);
    for child in &mut children {
        if let Some(m) = child.matrix() {
            child.set_matrix(shift * m);
        }
    }
    ensure_folders(&mut doc.library, name);
    let item_id = doc.library.fresh_item_id(name);
    let mut symbol = Symbol::new(name, kind, item_id);
    let mut sym_layer = Layer::new("Layer_1", LAYER_COLORS[0]);
    sym_layer.autoname = true;
    sym_layer.frames.push(Frame::new(0, 1, children));
    symbol.timeline.layers.push(sym_layer);
    symbol.linkage = spec.linkage.cloned();
    if spec.scale9 {
        let own = bounds.map(|b| Rect::new(b.x0 - offset.x, b.y0 - offset.y, b.x1 - offset.x, b.y1 - offset.y));
        symbol.scale9_grid = own.filter(|b| b.width() > 0.0 && b.height() > 0.0).map(crate::symbols::default_scale9_grid);
    }
    doc.library.items.push(LibraryItem::Symbol(Box::new(symbol)));

    let mut inst = SymbolInstance::new(name, Affine::translate(offset));
    inst.symbol_type = Some(kind);
    if kind == SymbolKind::Graphic {
        inst.loop_mode = Some(LoopMode::Loop);
    }
    inst.transformation_point = Point::ORIGIN;
    let at = sorted.first().copied().unwrap_or(0);
    let k = keyframe_mut(doc, id, layer, frame)?;
    let at = at.min(k.elements.len());
    k.elements.insert(at, Element::SymbolInstance(Box::new(inst)));
    Ok(at)
}

// ---------------------------------------------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------------------------------------------

fn next_layer_name(tl: &Timeline) -> String {
    let highest = tl.layers.iter().filter_map(|l| l.name.strip_prefix("Layer_")?.parse::<u32>().ok()).max().unwrap_or(0);
    let mut n = highest.saturating_add(1);
    loop {
        let candidate = format!("Layer_{n}");
        if !tl.layers.iter().any(|l| l.name == candidate) {
            return candidate;
        }
        n = n.saturating_add(1);
    }
}

/// Adds a layer at position `at` (0 = top, clamped to the end). It starts with a blank keyframe as long as the
/// timeline. `name` of `None` picks the next free `Layer_N`. Returns the layer's index.
///
/// # Errors
/// Unknown timeline, [`SceneError::InvalidName`] for an empty name, the layer limit.
pub fn add_layer(doc: &mut Document, id: &TimelineId, at: usize, name: Option<&str>, kind: LayerKind) -> Result<usize, SceneError> {
    let tl = timeline_mut(doc, id)?;
    if tl.layers.len() >= MAX_LAYERS {
        return Err(SceneError::LimitExceeded("layer count"));
    }
    if name.is_some_and(str::is_empty) {
        return Err(SceneError::InvalidName(String::new()));
    }
    let at = at.min(tl.layers.len());
    let color = tl.layers.len().checked_rem(LAYER_COLORS.len()).and_then(|i| LAYER_COLORS.get(i)).copied().unwrap_or(Rgba8::BLACK);
    let mut layer = Layer::new(name.map_or_else(|| next_layer_name(tl), str::to_owned), color);
    layer.autoname = name.is_none();
    layer.kind = kind;
    layer.frames.push(Frame::blank(0, tl.frame_count().max(1)));
    for l in &mut tl.layers {
        if let Some(p) = l.parent_layer_index.as_mut()
            && *p >= at
        {
            *p = p.saturating_add(1);
        }
    }
    tl.layers.insert(at, layer);
    Ok(at)
}

/// Indices of `layer` and every layer nested under it (folder contents, masked / guided layers), ascending.
fn layer_block(tl: &Timeline, layer: usize) -> Vec<usize> {
    let mut block = vec![layer];
    for i in layer.saturating_add(1)..tl.layers.len() {
        if tl.parent_of(i).is_some_and(|p| block.contains(&p)) {
            block.push(i);
        }
    }
    block
}

/// Deletes a layer together with the layers nested under it. Returns how many layers were removed.
///
/// # Errors
/// Unknown timeline or layer.
pub fn remove_layer(doc: &mut Document, id: &TimelineId, layer: usize) -> Result<usize, SceneError> {
    let tl = timeline_mut(doc, id)?;
    if layer >= tl.layers.len() {
        return Err(SceneError::NoSuchLayer { layer, count: tl.layers.len() });
    }
    let block = layer_block(tl, layer);
    let mut map: Vec<Option<usize>> = Vec::with_capacity(tl.layers.len());
    let mut next = 0_usize;
    for i in 0..tl.layers.len() {
        if block.contains(&i) {
            map.push(None);
        } else {
            map.push(Some(next));
            next = next.saturating_add(1);
        }
    }
    let old = std::mem::take(&mut tl.layers);
    for (i, mut l) in old.into_iter().enumerate() {
        if map.get(i).copied().flatten().is_none() {
            continue;
        }
        l.parent_layer_index = l.parent_layer_index.and_then(|p| map.get(p).copied().flatten());
        tl.layers.push(l);
    }
    Ok(block.len())
}

/// Moves a layer and the layers nested under it.
///
/// The block starts at position `to` among the remaining layers (0 = top, clamped). A layer that ends up above its
/// parent loses the parent link. Returns the new index of the moved layer.
///
/// # Errors
/// Unknown timeline or layer.
pub fn move_layer(doc: &mut Document, id: &TimelineId, from: usize, to: usize) -> Result<usize, SceneError> {
    let tl = timeline_mut(doc, id)?;
    let n = tl.layers.len();
    if from >= n {
        return Err(SceneError::NoSuchLayer { layer: from, count: n });
    }
    let block = layer_block(tl, from);
    let remaining: Vec<usize> = (0..n).filter(|i| !block.contains(i)).collect();
    let to = to.min(remaining.len());
    let mut order: Vec<usize> = Vec::with_capacity(n);
    order.extend(remaining.iter().take(to));
    order.extend(block.iter());
    order.extend(remaining.iter().skip(to));
    let mut new_index = vec![0_usize; n];
    for (new, &old) in order.iter().enumerate() {
        if let Some(slot) = new_index.get_mut(old) {
            *slot = new;
        }
    }
    let mut slots: Vec<Option<Layer>> = std::mem::take(&mut tl.layers).into_iter().map(Some).collect();
    for (new, &old) in order.iter().enumerate() {
        if let Some(mut l) = slots.get_mut(old).and_then(Option::take) {
            l.parent_layer_index = l.parent_layer_index.and_then(|p| new_index.get(p).copied()).filter(|&p| p < new);
            tl.layers.push(l);
        }
    }
    Ok(new_index.get(from).copied().unwrap_or(0))
}

/// Puts a layer in a folder, mask or guide layer (or back at the top level with `None`).
///
/// # Errors
/// [`SceneError::InvalidParent`] when the parent is not above the layer, is not a folder / mask / guide layer, or the
/// layer is a folder being put under a mask or guide.
pub fn set_layer_parent(doc: &mut Document, id: &TimelineId, layer: usize, parent: Option<usize>) -> Result<(), SceneError> {
    let tl = timeline_mut(doc, id)?;
    let count = tl.layers.len();
    let kind = tl.layers.get(layer).ok_or(SceneError::NoSuchLayer { layer, count })?.kind;
    if let Some(p) = parent {
        let parent_kind = tl.layers.get(p).ok_or(SceneError::NoSuchLayer { layer: p, count })?.kind;
        if p >= layer {
            return Err(SceneError::InvalidParent("the parent must be above the layer"));
        }
        match parent_kind {
            LayerKind::Folder => {}
            LayerKind::Mask | LayerKind::Guide => {
                if kind != LayerKind::Normal {
                    return Err(SceneError::InvalidParent("only normal layers can be masked or guided"));
                }
            }
            _ => return Err(SceneError::InvalidParent("the parent must be a folder, mask or guide layer")),
        }
    }
    if let Some(l) = tl.layers.get_mut(layer) {
        l.parent_layer_index = parent;
    }
    Ok(())
}

/// Renames a layer and marks the name as user-chosen.
///
/// # Errors
/// [`SceneError::InvalidName`] for an empty name, unknown timeline or layer.
pub fn rename_layer(doc: &mut Document, id: &TimelineId, layer: usize, name: &str) -> Result<(), SceneError> {
    if name.is_empty() {
        return Err(SceneError::InvalidName(String::new()));
    }
    let l = layer_mut(doc, id, layer)?;
    name.clone_into(&mut l.name);
    l.autoname = false;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------------------------------------------

/// Adds a scene with one layer and a one-frame blank keyframe at position `at` (clamped). Returns its index.
///
/// # Errors
/// [`SceneError::NameTaken`] or [`SceneError::InvalidName`].
pub fn add_scene(doc: &mut Document, name: &str, at: usize) -> Result<usize, SceneError> {
    if name.is_empty() {
        return Err(SceneError::InvalidName(String::new()));
    }
    if doc.scene_index(name).is_some() {
        return Err(SceneError::NameTaken(name.to_owned()));
    }
    let at = at.min(doc.scenes.len());
    let mut scene = Timeline::new(name);
    let mut layer = Layer::new("Layer_1", LAYER_COLORS[0]);
    layer.autoname = true;
    layer.frames.push(Frame::blank(0, 1));
    scene.layers.push(layer);
    doc.scenes.insert(at, scene);
    if doc.current_timeline >= at && doc.scenes.len() > 1 {
        doc.current_timeline = doc.current_timeline.saturating_add(1);
    }
    Ok(at)
}

/// Deletes a scene. The last scene cannot be deleted.
///
/// # Errors
/// [`SceneError::NoSuchTimeline`], or [`SceneError::InvalidRange`] for the last scene.
pub fn remove_scene(doc: &mut Document, index: usize) -> Result<Timeline, SceneError> {
    if index >= doc.scenes.len() {
        return Err(SceneError::NoSuchTimeline(TimelineId::Scene(index)));
    }
    if doc.scenes.len() == 1 {
        return Err(SceneError::InvalidRange("a document needs at least one scene"));
    }
    let removed = doc.scenes.remove(index);
    if doc.current_timeline > index || doc.current_timeline >= doc.scenes.len() {
        doc.current_timeline = doc.current_timeline.saturating_sub(1);
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------------------------------------------
// Library
// ---------------------------------------------------------------------------------------------------------------

/// Makes sure every folder on the path of `name` exists as a library folder.
pub fn ensure_folders(library: &mut Library, name: &str) {
    for folder in Library::parent_folder_paths(name) {
        if library.find_index(&folder).is_none() {
            let item_id = library.fresh_item_id(&folder);
            library.items.push(LibraryItem::Folder(FolderItem { name: folder, item_id, is_expanded: true, extra: crate::Extra::new() }));
        }
    }
}

/// Renames a library item and every reference to it (instances, bitmap fills, frame sounds). Renaming a folder
/// renames everything inside it.
///
/// # Errors
/// [`SceneError::NoSuchItem`], [`SceneError::InvalidName`], [`SceneError::NameTaken`] when the new name (or one of
/// the renamed children's new names) is in use.
pub fn rename_library_item(doc: &mut Document, old: &str, new: &str) -> Result<(), SceneError> {
    validate_item_name(new)?;
    let Some(item) = doc.library.find(old) else { return Err(SceneError::NoSuchItem(old.to_owned())) };
    if old == new {
        return Ok(());
    }
    let mut renames: HashMap<String, String> = HashMap::new();
    renames.insert(old.to_owned(), new.to_owned());
    if matches!(item, LibraryItem::Folder(_)) {
        let prefix = format!("{old}/");
        for it in &doc.library.items {
            if let Some(rest) = it.name().strip_prefix(&prefix) {
                renames.insert(it.name().to_owned(), format!("{new}/{rest}"));
            }
        }
    }
    for target in renames.values() {
        if doc.library.find_index(target).is_some() && !renames.contains_key(target) {
            return Err(SceneError::NameTaken(target.clone()));
        }
    }
    for it in &mut doc.library.items {
        if let Some(target) = renames.get(it.name()) {
            if let LibraryItem::Symbol(s) = it {
                let old_leaf = s.name.rsplit('/').next().unwrap_or_default();
                if s.timeline.name == old_leaf {
                    target.rsplit('/').next().unwrap_or_default().clone_into(&mut s.timeline.name);
                }
            }
            it.set_name(target.clone());
        }
    }
    ensure_folders(&mut doc.library, new);
    for scene in &mut doc.scenes {
        rename_references(scene, &renames);
    }
    for it in &mut doc.library.items {
        if let LibraryItem::Symbol(s) = it {
            rename_references(&mut s.timeline, &renames);
        }
    }
    Ok(())
}

fn rename_references(tl: &mut Timeline, renames: &HashMap<String, String>) {
    let fix = |name: &mut String| {
        if let Some(n) = renames.get(name.as_str()) {
            n.clone_into(name);
        }
    };
    for layer in &mut tl.layers {
        for frame in &mut layer.frames {
            if let Some(sound) = frame.sound.as_mut() {
                fix(&mut sound.name);
            }
            for element in &mut frame.elements {
                element.walk_mut(&mut |e| match e {
                    Element::SymbolInstance(s) => fix(&mut s.library_item_name),
                    Element::Bitmap(b) => fix(&mut b.library_item_name),
                    Element::Shape(s) => {
                        let fix_fill = |fill: &mut FillStyle| {
                            if let FillStyle::Bitmap(b) = fill {
                                fix(&mut b.path);
                            }
                        };
                        for fill in &mut s.shape.fills {
                            fix_fill(fill);
                        }
                        for stroke in &mut s.shape.strokes {
                            match stroke {
                                StrokeStyle::Solid(st) => fix_fill(&mut st.fill),
                                StrokeStyle::Other(o) => fix_fill(&mut o.base.fill),
                            }
                        }
                    }
                    Element::Text(_) | Element::Group(_) | Element::Unknown(_) => {}
                });
            }
        }
    }
}

/// Deletes a library item that nothing refers to. Folders must be empty. Returns the removed item (its media stays
/// in the [`crate::MediaStore`]; call [`crate::MediaStore::remove`] to free it).
///
/// # Errors
/// [`SceneError::NoSuchItem`], or [`SceneError::InUse`] when the item is referenced or the folder is not empty.
pub fn remove_library_item(doc: &mut Document, name: &str) -> Result<LibraryItem, SceneError> {
    let Some(item) = doc.library.find(name) else { return Err(SceneError::NoSuchItem(name.to_owned())) };
    if matches!(item, LibraryItem::Folder(_)) {
        let prefix = format!("{name}/");
        if doc.library.items.iter().any(|i| i.name().starts_with(&prefix)) {
            return Err(SceneError::InUse(name.to_owned()));
        }
    } else if !doc.references_to(name).is_empty() {
        return Err(SceneError::InUse(name.to_owned()));
    }
    doc.library.remove(name).ok_or_else(|| SceneError::NoSuchItem(name.to_owned()))
}

/// Copies a symbol under a new name (with a fresh `itemID`); the copy shares nothing with the original.
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `source` is not a symbol, name errors for `new_name`.
pub fn duplicate_symbol(doc: &mut Document, source: &str, new_name: &str) -> Result<(), SceneError> {
    validate_item_name(new_name)?;
    if doc.library.find_index(new_name).is_some() {
        return Err(SceneError::NameTaken(new_name.to_owned()));
    }
    let Some(original) = doc.library.find_symbol(source) else { return Err(SceneError::NoSuchItem(source.to_owned())) };
    let mut copy = original.clone();
    new_name.clone_into(&mut copy.name);
    copy.item_id = doc.library.fresh_item_id(new_name);
    copy.href = None;
    new_name.rsplit('/').next().unwrap_or(new_name).clone_into(&mut copy.timeline.name);
    ensure_folders(&mut doc.library, new_name);
    doc.library.items.push(LibraryItem::Symbol(Box::new(copy)));
    Ok(())
}

/// Sets or clears the label of the keyframe covering `frame`.
///
/// # Errors
/// Unknown timeline, layer or keyframe.
pub fn set_frame_label(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, label: Option<(&str, LabelType)>) -> Result<(), SceneError> {
    let k = keyframe_mut(doc, id, layer, frame)?;
    if let Some((text, kind)) = label {
        k.label = Some(text.to_owned());
        k.label_type = Some(kind);
    } else {
        k.label = None;
        k.label_type = None;
    }
    Ok(())
}

/// Moves the keyframe starting at `from` so it starts at `to`, keeping its length.
///
/// # Errors
/// [`SceneError::NoFrame`] when no keyframe starts at `from`, [`SceneError::Occupied`] when the new span would
/// overlap another keyframe, the frame limit.
pub fn move_keyframe(doc: &mut Document, id: &TimelineId, layer: usize, from: u32, to: u32) -> Result<(), SceneError> {
    check_frame(to)?;
    let l = layer_mut(doc, id, layer)?;
    l.sort_frames();
    let pos = l.frames.iter().position(|f| f.index == from).ok_or(SceneError::NoFrame { layer, frame: from })?;
    let duration = l.frames.get(pos).map_or(1, |f| f.duration);
    let new_end = to.saturating_add(duration);
    if l.frames.iter().enumerate().any(|(i, f)| i != pos && f.index < new_end && to < f.end()) {
        return Err(SceneError::Occupied { layer, frame: to });
    }
    let mut frame = l.frames.remove(pos);
    frame.index = to;
    let at = l.frames.partition_point(|f| f.index < to);
    l.frames.insert(at, frame);
    clear_dangling_tweens(l);
    Ok(())
}

/// Copies a layer (frames, elements and flags) and inserts the copy directly above it, named `"<name> copy"`.
/// Returns the index of the copy. Layers nested under a folder are not copied with it.
///
/// # Errors
/// Unknown timeline or layer, the layer limit.
pub fn duplicate_layer(doc: &mut Document, id: &TimelineId, layer: usize) -> Result<usize, SceneError> {
    let tl = timeline_mut(doc, id)?;
    if tl.layers.len() >= MAX_LAYERS {
        return Err(SceneError::LimitExceeded("layer count"));
    }
    let count = tl.layers.len();
    let mut copy = tl.layers.get(layer).ok_or(SceneError::NoSuchLayer { layer, count })?.clone();
    copy.name = format!("{} copy", copy.name);
    copy.autoname = false;
    for l in &mut tl.layers {
        if let Some(p) = l.parent_layer_index.as_mut()
            && *p >= layer
        {
            *p = p.saturating_add(1);
        }
    }
    tl.layers.insert(layer, copy);
    Ok(layer)
}

// ---------------------------------------------------------------------------------------------------------------
// Scripts and components
// ---------------------------------------------------------------------------------------------------------------

/// Longest frame script accepted, in bytes.
pub const MAX_SCRIPT_BYTES: usize = 1 << 20;

/// Sets (or, with `None` or empty text, removes) the frame script of the keyframe at `frame`.
///
/// A script belongs to a keyframe, so when `frame` lies inside a span a blank keyframe is made there first, as
/// Animate does when you type into the Actions panel on a plain frame.
///
/// # Errors
/// Unknown timeline or layer, a script over [`MAX_SCRIPT_BYTES`], or the frame limit.
pub fn set_frame_script(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, script: Option<&str>) -> Result<(), SceneError> {
    let text = script.filter(|s| !s.trim().is_empty());
    if text.is_some_and(|s| s.len() > MAX_SCRIPT_BYTES) {
        return Err(SceneError::LimitExceeded("script length"));
    }
    check_frame(frame)?;
    let is_key = layer_mut(doc, id, layer)?.is_keyframe(frame);
    if !is_key {
        if text.is_none() {
            return Ok(());
        }
        make_keyframe(doc, id, layer, frame, true)?;
    }
    keyframe_mut(doc, id, layer, frame)?.actionscript = text.map(str::to_owned);
    Ok(())
}

/// Sets the value of parameter `param` of the component instance `element` in the keyframe at `frame`.
///
/// # Errors
/// Unknown timeline, layer, keyframe or element; [`SceneError::NoSuchItem`] when the element is not a component or
/// has no such parameter.
pub fn set_component_param(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, element: usize, param: &str, value: &str) -> Result<(), SceneError> {
    let k = keyframe_mut(doc, id, layer, frame)?;
    let count = k.elements.len();
    let el = k.elements.get_mut(element).ok_or(SceneError::NoSuchElement { element, count })?;
    let Element::SymbolInstance(inst) = el else { return Err(SceneError::NoSuchItem(format!("element {element} is not a component"))) };
    let Some(component) = inst.component.as_mut() else { return Err(SceneError::NoSuchItem(format!("element {element} is not a component"))) };
    if component.set_param(param, value) { Ok(()) } else { Err(SceneError::NoSuchItem(format!("component parameter {param}"))) }
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
