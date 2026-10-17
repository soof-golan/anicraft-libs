//! Symbol-level edits: the registration point, 9-slice guides, linkage, the type of a symbol and of an instance,
//! swapping the symbol or bitmap an instance shows, and the four states of a button.
//!
//! Like [`crate::edit`], every function validates first and mutates second, so an `Err` leaves the document as it
//! was. Frame indices are 0-based.

use anim_core::{Affine, Matrix3D, Point, Rect};
use serde::{Deserialize, Serialize};

use crate::document::{Document, TimelineId};
use crate::edit;
use crate::element::{Element, LoopMode, SymbolKind};
use crate::error::SceneError;
use crate::library::{Library, Linkage};
use crate::timeline::Timeline;

/// How many frames a button symbol uses: Up, Over, Down and Hit.
pub const BUTTON_FRAMES: u32 = 4;

/// The four frames of a button symbol's timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ButtonState {
    /// The pointer is elsewhere.
    #[default]
    Up,
    /// The pointer is over the button.
    Over,
    /// The button is pressed.
    Down,
    /// The area that reacts to the pointer; never drawn.
    Hit,
}

impl ButtonState {
    /// Every state in timeline order.
    pub const ALL: [Self; 4] = [Self::Up, Self::Over, Self::Down, Self::Hit];

    /// The 0-based frame of the button's timeline that holds this state.
    #[must_use]
    pub const fn frame(self) -> u32 {
        match self {
            Self::Up => 0,
            Self::Over => 1,
            Self::Down => 2,
            Self::Hit => 3,
        }
    }

    /// The state a frame of a button timeline stands for; `None` past the fourth frame.
    #[must_use]
    pub const fn from_frame(frame: u32) -> Option<Self> {
        match frame {
            0 => Some(Self::Up),
            1 => Some(Self::Over),
            2 => Some(Self::Down),
            3 => Some(Self::Hit),
            _ => None,
        }
    }

    /// Lowercase name for commands and files: `up`, `over`, `down`, `hit`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Over => "over",
            Self::Down => "down",
            Self::Hit => "hit",
        }
    }

    /// The label the timeline header shows: `Up`, `Over`, `Down`, `Hit`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Over => "Over",
            Self::Down => "Down",
            Self::Hit => "Hit",
        }
    }

    /// Parses [`ButtonState::name`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The state a frame falls back to when a layer has nothing at it: Over and Down show what the state before
    /// them shows, and the Hit area is the Up shape. `None` for Up.
    #[must_use]
    pub const fn fallback(self) -> Option<Self> {
        match self {
            Self::Up => None,
            Self::Over | Self::Hit => Some(Self::Up),
            Self::Down => Some(Self::Over),
        }
    }
}

/// The frame of `layer` a button state shows: the keyframe covering the state's frame, else (Over, Down, Hit) the
/// keyframe the fallback state shows. `None` when the layer shows nothing in that state.
#[must_use]
pub fn button_state_keyframe(layer: &crate::timeline::Layer, state: ButtonState) -> Option<&crate::timeline::Frame> {
    let mut at = state;
    loop {
        if let Some(frame) = layer.keyframe_at(at.frame()) {
            return Some(frame);
        }
        at = at.fallback()?;
    }
}

fn symbol_id(name: &str) -> TimelineId {
    TimelineId::Symbol(name.to_owned())
}

fn element_at<'a>(doc: &'a mut Document, id: &TimelineId, layer: usize, frame: u32, index: usize) -> Result<&'a mut Element, SceneError> {
    let keyframe = edit::keyframe_mut(doc, id, layer, frame)?;
    let count = keyframe.elements.len();
    keyframe.elements.get_mut(index).ok_or(SceneError::NoSuchElement { element: index, count })
}

// ---------------------------------------------------------------------------------------------------------------
// Registration point
// ---------------------------------------------------------------------------------------------------------------

/// The bounds of what a symbol shows at its first frame, or `None` for an empty symbol.
#[must_use]
pub fn symbol_bounds(library: &Library, name: &str) -> Option<Rect> {
    let symbol = library.find_symbol(name)?;
    crate::bounds::timeline_bounds(&symbol.timeline, 0, library)
}

/// Moves the registration point of symbol `name` to `at`, a point in the symbol's current coordinates.
///
/// The content of every frame moves by `-at`, and every instance of the symbol (in scenes and in other symbols) is
/// moved the other way, so nothing changes on the stage.
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `name` is not a symbol, [`SceneError::InvalidRange`] for a point that is not
/// finite.
pub fn set_registration(doc: &mut Document, name: &str, at: Point) -> Result<(), SceneError> {
    if !(at.x.is_finite() && at.y.is_finite()) {
        return Err(SceneError::InvalidRange("the registration point must be finite"));
    }
    if doc.library.find_symbol(name).is_none() {
        return Err(SceneError::NoSuchItem(name.to_owned()));
    }
    if at == Point::ORIGIN {
        return Ok(());
    }
    let shift = Affine::translate((-at.x, -at.y));
    let back = Affine::translate((at.x, at.y));
    if let Some(symbol) = doc.library.find_symbol_mut(name) {
        shift_content(&mut symbol.timeline, shift);
        // The guides are in the symbol's pixels too.
        if let Some(grid) = symbol.scale9_grid.as_mut() {
            *grid = Rect::new(grid.x0 - at.x, grid.y0 - at.y, grid.x1 - at.x, grid.y1 - at.y);
        }
    }
    for scene in &mut doc.scenes {
        compensate_instances(scene, name, back, at);
    }
    for item in &mut doc.library.items {
        if let crate::library::LibraryItem::Symbol(symbol) = item {
            compensate_instances(&mut symbol.timeline, name, back, at);
        }
    }
    Ok(())
}

fn shift_content(timeline: &mut Timeline, shift: Affine) {
    for layer in &mut timeline.layers {
        for frame in &mut layer.frames {
            for element in &mut frame.elements {
                if let Some(m) = element.matrix() {
                    element.set_matrix(shift * m);
                }
            }
        }
    }
}

fn compensate_instances(timeline: &mut Timeline, name: &str, back: Affine, at: Point) {
    for layer in &mut timeline.layers {
        for frame in &mut layer.frames {
            for element in &mut frame.elements {
                element.walk_mut(&mut |e| {
                    let Element::SymbolInstance(instance) = e else { return };
                    if instance.library_item_name != name {
                        return;
                    }
                    // The instance's own matrix keeps its meaning for the shifted content: the old origin is `at`
                    // in the new coordinates.
                    match instance.matrix_3d {
                        Some(m3) => instance.set_matrix_3d(Matrix3D::from_affine(back).then(&m3)),
                        None => instance.matrix *= back,
                    }
                    instance.transformation_point = Point::new(instance.transformation_point.x - at.x, instance.transformation_point.y - at.y);
                });
            }
        }
    }
}

/// Sets the transformation point of one symbol instance, in the symbol's own coordinates (Free Transform pivots
/// about it).
///
/// # Errors
/// Unknown timeline, layer, keyframe or element, [`SceneError::InvalidRange`] when the element is not a symbol
/// instance or the point is not finite.
pub fn set_transformation_point(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, index: usize, point: Point) -> Result<(), SceneError> {
    if !(point.x.is_finite() && point.y.is_finite()) {
        return Err(SceneError::InvalidRange("the transformation point must be finite"));
    }
    match element_at(doc, id, layer, frame, index)? {
        Element::SymbolInstance(instance) => {
            instance.transformation_point = point;
            Ok(())
        }
        _ => Err(SceneError::InvalidRange("only symbol instances have a transformation point")),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// 9-slice scaling, linkage and the symbol type
// ---------------------------------------------------------------------------------------------------------------

/// The guides Animate starts with when 9-slice scaling is switched on: the middle third of the bounds.
#[must_use]
pub fn default_scale9_grid(bounds: Rect) -> Rect {
    let (w, h) = (bounds.width(), bounds.height());
    Rect::new(bounds.x0 + w / 3.0, bounds.y0 + h / 3.0, bounds.x1 - w / 3.0, bounds.y1 - h / 3.0)
}

/// Switches 9-slice scaling on (with `grid`, in the symbol's pixels) or off (`None`).
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `name` is not a symbol; [`SceneError::InvalidRange`] for a graphic symbol (only
/// movie clips and buttons scale in nine slices) or a grid that is empty or not finite.
pub fn set_scale9_grid(doc: &mut Document, name: &str, grid: Option<Rect>) -> Result<(), SceneError> {
    let Some(symbol) = doc.library.find_symbol_mut(name) else { return Err(SceneError::NoSuchItem(name.to_owned())) };
    if let Some(g) = grid {
        if symbol.kind == SymbolKind::Graphic {
            return Err(SceneError::InvalidRange("graphic symbols cannot use 9-slice scaling; make it a movie clip or a button"));
        }
        let finite = [g.x0, g.y0, g.x1, g.y1].iter().all(|v| v.is_finite());
        if !finite || g.x1 <= g.x0 || g.y1 <= g.y0 {
            return Err(SceneError::InvalidRange("the 9-slice guides must enclose a non-empty rectangle"));
        }
    }
    symbol.scale9_grid = grid;
    Ok(())
}

/// The linkage Animate fills in when "Export for `ActionScript`" is ticked: the class is the symbol's name made
/// into an identifier, the base class the one the symbol's type extends.
#[must_use]
pub fn default_linkage(name: &str, kind: SymbolKind) -> Linkage {
    let leaf = name.rsplit('/').next().unwrap_or(name);
    let mut class: String = leaf.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '$' { c } else { '_' }).collect();
    if class.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        class.insert(0, '_');
    }
    let base = match kind {
        SymbolKind::Button => "flash.display.SimpleButton",
        SymbolKind::Graphic | SymbolKind::MovieClip => "flash.display.MovieClip",
    };
    Linkage {
        export_for_actionscript: true,
        export_in_first_frame: Some(true),
        identifier: None,
        class_name: Some(class),
        base_class: Some(base.to_owned()),
        import_for_runtime_sharing: false,
        url: None,
    }
}

/// True for a dotted `ActionScript` class name such as `com.example.Wing`.
#[must_use]
pub fn is_class_name(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|part| {
            let mut chars = part.chars();
            chars.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$') && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
}

/// Sets or clears the linkage (Export for `ActionScript`, class, base class, runtime sharing) of a symbol.
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `name` is not a symbol; [`SceneError::InvalidRange`] for a graphic symbol that is
/// exported, an export without a class name, or a class or base class that is not a valid name.
pub fn set_linkage(doc: &mut Document, name: &str, linkage: Option<Linkage>) -> Result<(), SceneError> {
    let Some(symbol) = doc.library.find_symbol_mut(name) else { return Err(SceneError::NoSuchItem(name.to_owned())) };
    if let Some(l) = &linkage {
        validate_linkage(symbol.kind, l)?;
    }
    symbol.linkage = linkage;
    Ok(())
}

/// Checks a linkage against the type of the symbol it is for.
///
/// # Errors
/// [`SceneError::InvalidRange`] for a graphic symbol that is exported, an export without a class name, or a class or
/// base class that is not a valid name.
pub fn validate_linkage(kind: SymbolKind, l: &Linkage) -> Result<(), SceneError> {
    if l.export_for_actionscript {
        if kind == SymbolKind::Graphic {
            return Err(SceneError::InvalidRange("graphic symbols cannot be exported for ActionScript; make it a movie clip or a button"));
        }
        if !l.class_name.as_deref().is_some_and(is_class_name) {
            return Err(SceneError::InvalidRange("Export for ActionScript needs a class name such as Wing or com.example.Wing"));
        }
    }
    if l.base_class.as_deref().is_some_and(|b| !b.is_empty() && !is_class_name(b)) {
        return Err(SceneError::InvalidRange("the base class is not a valid class name"));
    }
    Ok(())
}

/// Changes the type of a symbol (Library > Properties > Type). Instances keep the type they were made with.
/// Becoming a graphic drops the exports and the 9-slice guides a graphic cannot have.
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `name` is not a symbol.
pub fn set_symbol_kind(doc: &mut Document, name: &str, kind: SymbolKind) -> Result<(), SceneError> {
    let Some(symbol) = doc.library.find_symbol_mut(name) else { return Err(SceneError::NoSuchItem(name.to_owned())) };
    symbol.kind = kind;
    if kind == SymbolKind::Graphic {
        symbol.scale9_grid = None;
        if let Some(linkage) = symbol.linkage.as_mut() {
            linkage.export_for_actionscript = false;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------------------------------------------

/// Swap Symbol: makes the instance show another symbol.
///
/// The place, transform, colour effect, filters and name stay. An instance that followed its old symbol's type
/// follows the new one's; one that was given a type of its own keeps it. Component parameters belong to the old
/// symbol and are dropped.
///
/// # Errors
/// Unknown timeline, layer, keyframe or element; [`SceneError::InvalidRange`] when the element is not a symbol
/// instance; [`SceneError::NoSuchItem`] when `symbol` is not a symbol; [`SceneError::Cycle`] when the swap would make
/// a symbol contain itself.
pub fn swap_symbol(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, index: usize, symbol: &str) -> Result<(), SceneError> {
    let Some(new_kind) = doc.library.find_symbol(symbol).map(|s| s.kind) else { return Err(SceneError::NoSuchItem(symbol.to_owned())) };
    if let TimelineId::Symbol(container) = id
        && doc.symbol_reaches(symbol, container)
    {
        return Err(SceneError::Cycle(container.clone()));
    }
    let old_kind = {
        let Element::SymbolInstance(instance) = element_at(doc, id, layer, frame, index)? else {
            return Err(SceneError::InvalidRange("only symbol instances can be swapped for another symbol"));
        };
        let old = instance.library_item_name.clone();
        let own = instance.symbol_type;
        let old_symbol_kind = doc.library.find_symbol(&old).map(|s| s.kind);
        (own, old_symbol_kind)
    };
    let follows = old_kind.0.is_none() || old_kind.0 == old_kind.1;
    let Element::SymbolInstance(instance) = element_at(doc, id, layer, frame, index)? else { return Ok(()) };
    symbol.clone_into(&mut instance.library_item_name);
    instance.component = None;
    if follows {
        instance.symbol_type = Some(new_kind);
    }
    if instance.symbol_type.unwrap_or(new_kind) == SymbolKind::Graphic && instance.loop_mode.is_none() {
        instance.loop_mode = Some(LoopMode::Loop);
    }
    Ok(())
}

/// Sets the behaviour of one instance (Properties > Instance behavior): graphic, movie clip or button. The symbol
/// is not changed.
///
/// # Errors
/// Unknown timeline, layer, keyframe or element; [`SceneError::InvalidRange`] when the element is not a symbol
/// instance.
pub fn set_instance_kind(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, index: usize, kind: SymbolKind) -> Result<(), SceneError> {
    let Element::SymbolInstance(instance) = element_at(doc, id, layer, frame, index)? else {
        return Err(SceneError::InvalidRange("only symbol instances have a behavior"));
    };
    instance.symbol_type = Some(kind);
    if kind == SymbolKind::Graphic && instance.loop_mode.is_none() {
        instance.loop_mode = Some(LoopMode::Loop);
    }
    Ok(())
}

/// Swap Bitmap: makes a bitmap instance show another library bitmap, keeping its matrix.
///
/// # Errors
/// Unknown timeline, layer, keyframe or element; [`SceneError::InvalidRange`] when the element is not a bitmap
/// instance; [`SceneError::NoSuchItem`] when `bitmap` is not a library bitmap.
pub fn swap_bitmap(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, index: usize, bitmap: &str) -> Result<(), SceneError> {
    if doc.library.find_bitmap(bitmap).is_none() {
        return Err(SceneError::NoSuchItem(bitmap.to_owned()));
    }
    match element_at(doc, id, layer, frame, index)? {
        Element::Bitmap(instance) => {
            bitmap.clone_into(&mut instance.library_item_name);
            Ok(())
        }
        _ => Err(SceneError::InvalidRange("only bitmap instances can be swapped for another bitmap")),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------------------------------------------

/// Gives every layer of button symbol `name` a keyframe at each of the `states` that is not one yet.
///
/// The keyframe is a copy of what the layer shows there (Insert Keyframe), or with `blank` an empty one (Insert Blank
/// Keyframe). Returns how many keyframes were made. Folders and cameras are left alone.
///
/// # Errors
/// [`SceneError::NoSuchItem`] when `name` is not a symbol; [`SceneError::InvalidRange`] when it is not a button.
pub fn add_button_states(doc: &mut Document, name: &str, states: &[ButtonState], blank: bool) -> Result<usize, SceneError> {
    let Some(symbol) = doc.library.find_symbol(name) else { return Err(SceneError::NoSuchItem(name.to_owned())) };
    if symbol.kind != SymbolKind::Button {
        return Err(SceneError::InvalidRange("only button symbols have Up, Over, Down and Hit frames"));
    }
    let layers = symbol.timeline.layers.len();
    let id = symbol_id(name);
    let mut made = 0_usize;
    let mut sorted: Vec<ButtonState> = states.to_vec();
    sorted.sort_by_key(|s| s.frame());
    sorted.dedup();
    for layer in 0..layers {
        let editable = doc.library.find_symbol(name).and_then(|s| s.timeline.layers.get(layer)).is_some_and(|l| !l.kind.is_structural());
        if !editable {
            continue;
        }
        for state in &sorted {
            if *state == ButtonState::Up {
                continue;
            }
            let already =
                doc.library.find_symbol(name).and_then(|s| s.timeline.layers.get(layer)).is_some_and(|l| l.frames.iter().any(|f| f.index == state.frame()));
            if already {
                continue;
            }
            if blank {
                edit::insert_blank_keyframe(doc, &id, layer, state.frame())?;
            } else {
                edit::insert_keyframe(doc, &id, layer, state.frame())?;
            }
            made = made.saturating_add(1);
        }
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use anim_core::{Affine, Point, Rect};

    use super::*;
    use crate::element::SymbolInstance;
    use crate::timeline::Frame;

    fn doc_with(name: &str, kind: SymbolKind) -> Document {
        let mut d = Document::default();
        d.add_symbol(name, kind).unwrap();
        d
    }

    #[test]
    fn button_states_have_frames_names_and_fallbacks() {
        for s in ButtonState::ALL {
            assert_eq!(ButtonState::from_frame(s.frame()), Some(s));
            assert_eq!(ButtonState::from_name(s.name()), Some(s));
        }
        assert_eq!(ButtonState::from_frame(4), None);
        assert_eq!(ButtonState::Down.fallback(), Some(ButtonState::Over));
        assert_eq!(ButtonState::Hit.fallback(), Some(ButtonState::Up));
        assert_eq!(ButtonState::Up.fallback(), None);
    }

    #[test]
    fn a_layer_with_only_an_up_keyframe_shows_it_in_every_state() {
        let mut layer = crate::timeline::Layer::new("L", anim_core::Rgba8::BLACK);
        layer.frames.push(Frame::blank(0, 1));
        for s in ButtonState::ALL {
            assert_eq!(button_state_keyframe(&layer, s).map(|f| f.index), Some(0), "{s:?}");
        }
        layer.frames.push(Frame::blank(1, 1));
        assert_eq!(button_state_keyframe(&layer, ButtonState::Over).map(|f| f.index), Some(1));
        assert_eq!(button_state_keyframe(&layer, ButtonState::Down).map(|f| f.index), Some(1), "Down follows Over");
        assert_eq!(button_state_keyframe(&layer, ButtonState::Hit).map(|f| f.index), Some(0), "the Hit area is the Up shape unless Hit has its own");
    }

    #[test]
    fn the_scale9_grid_needs_a_scalable_symbol_and_a_real_rectangle() {
        let mut d = doc_with("g", SymbolKind::Graphic);
        let grid = Rect::new(10.0, 10.0, 90.0, 90.0);
        assert!(set_scale9_grid(&mut d, "g", Some(grid)).is_err());
        assert!(set_scale9_grid(&mut d, "nope", Some(grid)).is_err());
        set_symbol_kind(&mut d, "g", SymbolKind::MovieClip).unwrap();
        assert!(set_scale9_grid(&mut d, "g", Some(Rect::new(5.0, 5.0, 5.0, 9.0))).is_err());
        assert!(set_scale9_grid(&mut d, "g", Some(Rect::new(f64::NAN, 0.0, 1.0, 1.0))).is_err());
        set_scale9_grid(&mut d, "g", Some(grid)).unwrap();
        assert_eq!(d.library.find_symbol("g").and_then(|s| s.scale9_grid), Some(grid));
        set_symbol_kind(&mut d, "g", SymbolKind::Graphic).unwrap();
        assert_eq!(d.library.find_symbol("g").and_then(|s| s.scale9_grid), None, "a graphic has no guides");
    }

    #[test]
    fn default_guides_are_the_middle_third() {
        let g = default_scale9_grid(Rect::new(0.0, 0.0, 90.0, 60.0));
        assert_eq!(g, Rect::new(30.0, 20.0, 60.0, 40.0));
    }

    #[test]
    fn linkage_is_validated_and_defaults_follow_the_type() {
        let mut d = doc_with("ui/my wing", SymbolKind::MovieClip);
        let l = default_linkage("ui/my wing", SymbolKind::MovieClip);
        assert_eq!(l.class_name.as_deref(), Some("my_wing"));
        assert_eq!(l.base_class.as_deref(), Some("flash.display.MovieClip"));
        assert_eq!(default_linkage("1b", SymbolKind::Button).class_name.as_deref(), Some("_1b"));
        assert_eq!(default_linkage("b", SymbolKind::Button).base_class.as_deref(), Some("flash.display.SimpleButton"));
        set_linkage(&mut d, "ui/my wing", Some(l)).unwrap();
        assert!(d.library.find_symbol("ui/my wing").unwrap().linkage.as_ref().unwrap().export_for_actionscript);
        let mut bad = default_linkage("x", SymbolKind::MovieClip);
        bad.class_name = Some("not a class".into());
        assert!(set_linkage(&mut d, "ui/my wing", Some(bad)).is_err());
        let mut bad_base = default_linkage("x", SymbolKind::MovieClip);
        bad_base.base_class = Some("3d".into());
        assert!(set_linkage(&mut d, "ui/my wing", Some(bad_base)).is_err());
        set_symbol_kind(&mut d, "ui/my wing", SymbolKind::Graphic).unwrap();
        assert!(!d.library.find_symbol("ui/my wing").unwrap().linkage.as_ref().unwrap().export_for_actionscript, "becoming a graphic drops the export");
        assert!(set_linkage(&mut d, "ui/my wing", Some(default_linkage("x", SymbolKind::MovieClip))).is_err(), "a graphic cannot be exported");
        set_linkage(&mut d, "ui/my wing", None).unwrap();
        assert!(d.library.find_symbol("ui/my wing").unwrap().linkage.is_none());
    }

    #[test]
    fn class_names_are_dotted_identifiers() {
        for ok in ["Wing", "com.example.Wing", "_x", "$a.b1"] {
            assert!(is_class_name(ok), "{ok}");
        }
        for bad in ["", "1a", "a..b", "a b", ".a", "a."] {
            assert!(!is_class_name(bad), "{bad}");
        }
    }

    fn scene_with_instance(symbol: &str, matrix: Affine) -> Document {
        let mut d = Document::default();
        d.add_symbol("a", SymbolKind::MovieClip).unwrap();
        d.add_symbol("b", SymbolKind::Button).unwrap();
        let id = TimelineId::Scene(0);
        edit::add_symbol_instance(&mut d, &id, 0, 0, symbol, matrix).unwrap();
        d
    }

    #[test]
    fn swapping_keeps_the_placement_and_follows_the_type() {
        let mut d = scene_with_instance("a", Affine::translate((5.0, 6.0)));
        let id = TimelineId::Scene(0);
        if let Element::SymbolInstance(i) = element_at(&mut d, &id, 0, 0, 0).unwrap() {
            i.name = Some("hero".into());
        }
        swap_symbol(&mut d, &id, 0, 0, 0, "b").unwrap();
        let Element::SymbolInstance(i) = element_at(&mut d, &id, 0, 0, 0).unwrap() else { panic!() };
        assert_eq!(i.library_item_name, "b");
        assert_eq!(i.symbol_type, Some(SymbolKind::Button), "followed the symbol");
        assert_eq!(i.matrix, Affine::translate((5.0, 6.0)));
        assert_eq!(i.name.as_deref(), Some("hero"));
        // An instance given a type of its own keeps it.
        set_instance_kind(&mut d, &id, 0, 0, 0, SymbolKind::Graphic).unwrap();
        swap_symbol(&mut d, &id, 0, 0, 0, "a").unwrap();
        let Element::SymbolInstance(i) = element_at(&mut d, &id, 0, 0, 0).unwrap() else { panic!() };
        assert_eq!(i.symbol_type, Some(SymbolKind::Graphic));
        assert_eq!(i.loop_mode, Some(LoopMode::Loop));
        assert!(swap_symbol(&mut d, &id, 0, 0, 0, "missing").is_err());
        assert!(swap_symbol(&mut d, &id, 0, 0, 5, "a").is_err());
    }

    #[test]
    fn swapping_cannot_make_a_symbol_contain_itself() {
        let mut d = scene_with_instance("a", Affine::IDENTITY);
        let inside_b = TimelineId::Symbol("b".into());
        edit::add_symbol_instance(&mut d, &inside_b, 0, 0, "a", Affine::IDENTITY).unwrap();
        let inside_a = TimelineId::Symbol("a".into());
        edit::add_symbol_instance(&mut d, &inside_a, 0, 0, "a", Affine::IDENTITY).unwrap_err();
        // b holds a, so swapping the instance of a inside b for b itself would loop.
        assert_eq!(swap_symbol(&mut d, &inside_b, 0, 0, 0, "b"), Err(SceneError::Cycle("b".into())));
    }

    #[test]
    fn bitmaps_swap_only_to_bitmaps() {
        let mut d = Document::default();
        d.library.push(crate::LibraryItem::Bitmap(crate::BitmapItem::new("one", "i1"))).unwrap();
        d.library.push(crate::LibraryItem::Bitmap(crate::BitmapItem::new("two", "i2"))).unwrap();
        let id = TimelineId::Scene(0);
        edit::add_element(&mut d, &id, 0, 0, Element::Bitmap(crate::BitmapInstance::new("one", Affine::translate((3.0, 4.0))))).unwrap();
        swap_bitmap(&mut d, &id, 0, 0, 0, "two").unwrap();
        let Element::Bitmap(b) = element_at(&mut d, &id, 0, 0, 0).unwrap() else { panic!() };
        assert_eq!((b.library_item_name.as_str(), b.matrix), ("two", Affine::translate((3.0, 4.0))));
        assert!(swap_bitmap(&mut d, &id, 0, 0, 0, "nope").is_err());
        d.add_symbol("s", SymbolKind::Graphic).unwrap();
        assert!(swap_bitmap(&mut d, &id, 0, 0, 0, "s").is_err(), "a symbol is not a bitmap");
    }

    #[test]
    fn moving_the_registration_point_keeps_the_stage_as_it_was() {
        let mut d = Document::default();
        d.add_symbol("m", SymbolKind::MovieClip).unwrap();
        let inside = TimelineId::Symbol("m".into());
        let rect = crate::ShapeElement::new(anim_shape::EdgeShape::default());
        edit::add_element(&mut d, &inside, 0, 0, Element::Shape(rect)).unwrap();
        let scene = TimelineId::Scene(0);
        edit::add_symbol_instance(&mut d, &scene, 0, 0, "m", Affine::translate((100.0, 50.0)) * Affine::scale(2.0)).unwrap();
        let before = crate::bounds::element_bounds(d.scenes[0].layers[0].frames[0].elements.first().unwrap(), &d.library);
        set_registration(&mut d, "m", Point::new(10.0, 20.0)).unwrap();
        // The shape moved by (-10, -20) inside the symbol.
        let Element::Shape(s) = &d.library.find_symbol("m").unwrap().timeline.layers[0].frames[0].elements[0] else { panic!() };
        assert_eq!(s.matrix, Affine::translate((-10.0, -20.0)));
        // The instance compensates: a point at the symbol's new origin lands where the old (10, 20) did.
        let Element::SymbolInstance(i) = &d.scenes[0].layers[0].frames[0].elements[0] else { panic!() };
        let landed = i.matrix * Point::ORIGIN;
        assert!((landed.x - 120.0).abs() < 1e-9 && (landed.y - 90.0).abs() < 1e-9, "{landed:?}");
        assert_eq!(before, crate::bounds::element_bounds(d.scenes[0].layers[0].frames[0].elements.first().unwrap(), &d.library));
        assert!(set_registration(&mut d, "x", Point::ORIGIN).is_err());
        assert!(set_registration(&mut d, "m", Point::new(f64::NAN, 0.0)).is_err());
    }

    #[test]
    fn the_registration_move_also_moves_the_guides_and_nested_instances() {
        let mut d = Document::default();
        d.add_symbol("inner", SymbolKind::MovieClip).unwrap();
        d.add_symbol("outer", SymbolKind::MovieClip).unwrap();
        let outer = TimelineId::Symbol("outer".into());
        edit::add_symbol_instance(&mut d, &outer, 0, 0, "inner", Affine::translate((7.0, 8.0))).unwrap();
        set_scale9_grid(&mut d, "inner", Some(Rect::new(10.0, 10.0, 20.0, 20.0))).unwrap();
        set_registration(&mut d, "inner", Point::new(4.0, 4.0)).unwrap();
        assert_eq!(d.library.find_symbol("inner").unwrap().scale9_grid, Some(Rect::new(6.0, 6.0, 16.0, 16.0)));
        let Element::SymbolInstance(i) = &d.library.find_symbol("outer").unwrap().timeline.layers[0].frames[0].elements[0] else { panic!() };
        assert_eq!(i.matrix, Affine::translate((11.0, 12.0)));
    }

    #[test]
    fn a_transformation_point_is_set_on_instances_only() {
        let mut d = scene_with_instance("a", Affine::IDENTITY);
        let id = TimelineId::Scene(0);
        set_transformation_point(&mut d, &id, 0, 0, 0, Point::new(3.0, 4.0)).unwrap();
        let Element::SymbolInstance(i) = element_at(&mut d, &id, 0, 0, 0).unwrap() else { panic!() };
        assert_eq!(i.transformation_point, Point::new(3.0, 4.0));
        assert!(set_transformation_point(&mut d, &id, 0, 0, 0, Point::new(f64::INFINITY, 0.0)).is_err());
        edit::add_element(&mut d, &id, 0, 0, Element::Bitmap(crate::BitmapInstance::new("x", Affine::IDENTITY))).unwrap();
        assert!(set_transformation_point(&mut d, &id, 0, 0, 1, Point::ORIGIN).is_err());
    }

    #[test]
    fn button_states_get_keyframes_on_every_drawing_layer() {
        let mut d = doc_with("btn", SymbolKind::Button);
        let id = TimelineId::Symbol("btn".into());
        let rect = crate::ShapeElement::new(anim_shape::EdgeShape::default());
        edit::add_element(&mut d, &id, 0, 0, Element::Shape(rect)).unwrap();
        let made = add_button_states(&mut d, "btn", &[ButtonState::Over, ButtonState::Down, ButtonState::Hit], false).unwrap();
        assert_eq!(made, 3);
        let layer = &d.library.find_symbol("btn").unwrap().timeline.layers[0];
        assert_eq!(layer.frames.iter().map(|f| f.index).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert!(layer.frames.iter().all(|f| f.elements.len() == 1), "copies of the Up content");
        assert_eq!(add_button_states(&mut d, "btn", &[ButtonState::Over], false).unwrap(), 0, "already keyframes");
        d.add_symbol("clip", SymbolKind::MovieClip).unwrap();
        assert!(add_button_states(&mut d, "clip", &[ButtonState::Over], false).is_err());
        let blank = {
            let mut d2 = doc_with("b2", SymbolKind::Button);
            let id2 = TimelineId::Symbol("b2".into());
            edit::add_element(&mut d2, &id2, 0, 0, Element::Shape(crate::ShapeElement::new(anim_shape::EdgeShape::default()))).unwrap();
            add_button_states(&mut d2, "b2", &[ButtonState::Over], true).unwrap();
            d2.library.find_symbol("b2").unwrap().timeline.layers[0].frames.iter().map(|f| f.elements.len()).collect::<Vec<_>>()
        };
        assert_eq!(blank, vec![1, 0]);
    }

    #[test]
    fn instances_inside_groups_are_compensated_too() {
        let mut d = Document::default();
        d.add_symbol("m", SymbolKind::MovieClip).unwrap();
        let scene = TimelineId::Scene(0);
        let inst = SymbolInstance::new("m", Affine::IDENTITY);
        edit::add_element(&mut d, &scene, 0, 0, Element::Group(crate::Group::new(vec![Element::SymbolInstance(Box::new(inst))]))).unwrap();
        set_registration(&mut d, "m", Point::new(1.0, 2.0)).unwrap();
        let Element::Group(g) = &d.scenes[0].layers[0].frames[0].elements[0] else { panic!() };
        let Element::SymbolInstance(i) = &g.members[0] else { panic!() };
        assert_eq!(i.matrix, Affine::translate((1.0, 2.0)));
    }
}
