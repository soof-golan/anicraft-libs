//! Things that sit in a keyframe: shapes, symbol instances, bitmaps, text and groups.
//!
//! Flash matrices are `a b c d tx ty` with `tx`/`ty` in pixels, which is exactly `kurbo::Affine::new([a, b, c, d,
//! tx, ty])`. All geometry here is in pixels, y down.

use anim_core::{Affine, BlendMode, ColorTransform, Matrix3D, Point, Rgba8};
use anim_shape::EdgeShape;
use serde::{Deserialize, Serialize};

use crate::extra::{Extra, RawXml};

/// What kind of symbol: how its timeline plays inside its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SymbolKind {
    /// Timeline slaved to the parent (loop / play once / single frame).
    Graphic,
    /// Own timeline that keeps running with the parent. XFL's default when `symbolType` is absent.
    #[default]
    MovieClip,
    /// Four frames: up, over, down, hit.
    Button,
}

impl SymbolKind {
    /// The XFL `symbolType` attribute value.
    #[must_use]
    pub const fn xfl_name(self) -> &'static str {
        match self {
            Self::Graphic => "graphic",
            Self::MovieClip => "movie clip",
            Self::Button => "button",
        }
    }

    /// Parses an XFL `symbolType` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "graphic" => Some(Self::Graphic),
            "movie clip" => Some(Self::MovieClip),
            "button" => Some(Self::Button),
            _ => None,
        }
    }
}

/// How a graphic instance plays its timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LoopMode {
    #[default]
    Loop,
    PlayOnce,
    SingleFrame,
}

impl LoopMode {
    /// The XFL `loop` attribute value.
    #[must_use]
    pub const fn xfl_name(self) -> &'static str {
        match self {
            Self::Loop => "loop",
            Self::PlayOnce => "play once",
            Self::SingleFrame => "single frame",
        }
    }

    /// Parses an XFL `loop` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "loop" => Some(Self::Loop),
            "play once" => Some(Self::PlayOnce),
            "single frame" => Some(Self::SingleFrame),
            _ => None,
        }
    }
}

/// The Animate "Color" property style an instance was saved with. The numbers are the style's own parameters; the
/// [`ColorTransform`] they imply is [`ColorEffect::to_transform`].
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum ColorEffect {
    /// No colour effect.
    #[default]
    None,
    /// `brightness` in -1.0 ..= 1.0.
    Brightness(f32),
    /// Tint towards `color` by `amount` in 0.0 ..= 1.0.
    Tint { color: Rgba8, amount: f32 },
    /// Alpha multiplier in 0.0 ..= 1.0.
    Alpha(f32),
    /// Free multipliers and offsets.
    Advanced(ColorTransform),
}

impl ColorEffect {
    /// The transform this style means.
    #[must_use]
    pub fn to_transform(self) -> ColorTransform {
        match self {
            Self::None => ColorTransform::IDENTITY,
            Self::Brightness(b) => ColorTransform::brightness(b),
            Self::Tint { color, amount } => ColorTransform::tint(color, amount),
            Self::Alpha(a) => ColorTransform::alpha(a),
            Self::Advanced(t) => t,
        }
    }
}

/// A filter on an instance: one that maps onto an [`anim_display::Filter`], or an element kept verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InstanceFilter {
    /// Blur, drop shadow or glow; `extra` keeps attributes such as `alpha` or `enabled` the mapping has no slot for.
    Mapped { filter: anim_display::Filter, extra: Extra },
    /// Bevel, gradient glow, adjust colour, ... kept as raw XML (rendered as no-op).
    Raw(RawXml),
}

impl InstanceFilter {
    /// The filter the renderer should apply, if this one maps onto the display model.
    #[must_use]
    pub const fn to_display(&self) -> Option<&anim_display::Filter> {
        match self {
            Self::Mapped { filter, .. } => Some(filter),
            Self::Raw(_) => None,
        }
    }

    /// Wraps a display filter with nothing extra.
    #[must_use]
    pub const fn mapped(filter: anim_display::Filter) -> Self {
        Self::Mapped { filter, extra: Extra::new() }
    }
}

/// A vector shape in a keyframe (`DOMShape`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ShapeElement {
    pub shape: EdgeShape,
    /// Object-drawing mode (`isDrawingObject`): the shape does not merge with its neighbours.
    pub is_drawing_object: bool,
    /// Shape-to-parent matrix (absent in XFL for merge-mode shapes, which is the identity here).
    pub matrix: Affine,
    /// Transformation point, present for drawing objects.
    pub transformation_point: Option<Point>,
    pub extra: Extra,
}

impl ShapeElement {
    /// A merge-mode shape at the origin.
    #[must_use]
    pub const fn new(shape: EdgeShape) -> Self {
        Self { shape, is_drawing_object: false, matrix: Affine::IDENTITY, transformation_point: None, extra: Extra::new() }
    }
}

/// An instance of a library symbol (`DOMSymbolInstance`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolInstance {
    /// Library path of the symbol, for example `folder/wing`.
    pub library_item_name: String,
    /// The instance's own `symbolType`; `None` when absent from the file (then the library item decides).
    pub symbol_type: Option<SymbolKind>,
    /// Graphic playback mode; `None` when absent from the file (graphics then loop).
    pub loop_mode: Option<LoopMode>,
    /// First frame shown for graphics (`firstFrame`, 0-based).
    pub first_frame: u32,
    /// Last frame for play-once graphics (`lastFrame`).
    pub last_frame: Option<u32>,
    pub matrix: Affine,
    /// The instance's 3D placement (`<matrix3D>`), set by the 3D Rotation and 3D Translation tools. When present it
    /// replaces `matrix` for drawing; `matrix` keeps its flattened z = 0 plane so 2D tools still see a position.
    pub matrix_3d: Option<Matrix3D>,
    /// The point the 3D tools rotate about, in the instance's parent space (`centerPoint3DX`, `centerPoint3DY`).
    /// `None` is the instance's own origin.
    pub center_point_3d: Option<Point>,
    /// Registration/transformation point in the symbol's own space.
    pub transformation_point: Point,
    /// Colour transform applied to the instance.
    pub color: ColorTransform,
    /// Which Color style produced `color`, so it is saved as the same style.
    pub color_effect: ColorEffect,
    pub blend_mode: BlendMode,
    pub filters: Vec<InstanceFilter>,
    pub cache_as_bitmap: bool,
    /// `isVisible`; false hides the instance on stage.
    pub visible: bool,
    /// Instance name (`name`), used by `ActionScript`.
    pub name: Option<String>,
    pub extra: Extra,
}

impl SymbolInstance {
    /// A visible instance of `library_item_name` with the given matrix and every other property at its default.
    #[must_use]
    pub fn new(library_item_name: impl Into<String>, matrix: Affine) -> Self {
        Self {
            library_item_name: library_item_name.into(),
            symbol_type: None,
            loop_mode: None,
            first_frame: 0,
            last_frame: None,
            matrix,
            matrix_3d: None,
            center_point_3d: None,
            transformation_point: Point::ORIGIN,
            color: ColorTransform::IDENTITY,
            color_effect: ColorEffect::None,
            blend_mode: BlendMode::Normal,
            filters: Vec::new(),
            cache_as_bitmap: false,
            visible: true,
            name: None,
            extra: Extra::new(),
        }
    }

    /// Gives the instance a 3D placement, keeping `matrix` as its flattened plane. A planar matrix (no depth) drops
    /// the 3D placement again, as Animate does when the last 3D property is reset.
    pub fn set_matrix_3d(&mut self, matrix_3d: Matrix3D) {
        self.matrix = matrix_3d.plane_affine();
        self.matrix_3d = (!matrix_3d.is_planar()).then_some(matrix_3d);
    }

    /// The matrix the instance is drawn with: its 3D matrix, or `matrix` embedded in the z = 0 plane.
    #[must_use]
    pub fn placement_3d(&self) -> Matrix3D {
        self.matrix_3d.unwrap_or_else(|| Matrix3D::from_affine(self.matrix))
    }

    /// Replaces the 2D matrix. A 3D instance moves by the change, applied in its parent's plane after its 3D placement.
    pub fn set_matrix_2d(&mut self, matrix: Affine) {
        if let Some(m3) = self.matrix_3d
            && self.matrix.determinant().abs() > 1.0e-12
        {
            let delta = matrix * self.matrix.inverse();
            self.matrix_3d = Some(m3.then(&Matrix3D::from_affine(delta)));
            self.matrix = matrix;
        } else {
            self.matrix = matrix;
            self.matrix_3d = None;
        }
    }

    /// Sets the colour style and the transform it implies.
    pub fn set_color_effect(&mut self, effect: ColorEffect) {
        self.color_effect = effect;
        self.color = effect.to_transform();
    }

    /// The symbol kind that decides how the instance plays: its own `symbolType`, else the library item's kind, else
    /// a movie clip (XFL's default).
    #[must_use]
    pub fn effective_kind(&self, library: &crate::Library) -> SymbolKind {
        self.symbol_type.or_else(|| library.find_symbol(&self.library_item_name).map(|s| s.kind)).unwrap_or_default()
    }

    /// The playback mode graphics use: the stored one, or [`LoopMode::Loop`].
    #[must_use]
    pub fn effective_loop_mode(&self) -> LoopMode {
        self.loop_mode.unwrap_or_default()
    }
}

/// An instance of a library bitmap (`DOMBitmapInstance`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BitmapInstance {
    pub library_item_name: String,
    pub matrix: Affine,
    pub extra: Extra,
}

impl BitmapInstance {
    /// A bitmap instance with an extra-free body.
    #[must_use]
    pub fn new(library_item_name: impl Into<String>, matrix: Affine) -> Self {
        Self { library_item_name: library_item_name.into(), matrix, extra: Extra::new() }
    }
}

/// Which text tool made a text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextKind {
    #[default]
    Static,
    Dynamic,
    Input,
}

impl TextKind {
    /// The XFL element name (`DOMStaticText`, `DOMDynamicText`, `DOMInputText`).
    #[must_use]
    pub const fn element_name(self) -> &'static str {
        match self {
            Self::Static => "DOMStaticText",
            Self::Dynamic => "DOMDynamicText",
            Self::Input => "DOMInputText",
        }
    }
}

/// Text flow direction of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextOrientation {
    #[default]
    Horizontal,
    VerticalLeftToRight,
    VerticalRightToLeft,
}

impl TextOrientation {
    /// The XFL `orientation` attribute value, `None` for horizontal (which XFL omits).
    #[must_use]
    pub const fn xfl_name(self) -> Option<&'static str> {
        match self {
            Self::Horizontal => None,
            Self::VerticalLeftToRight => Some("vertical left to right"),
            Self::VerticalRightToLeft => Some("vertical"),
        }
    }

    /// Parses an XFL `orientation` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "horizontal" => Some(Self::Horizontal),
            "vertical left to right" => Some(Self::VerticalLeftToRight),
            "vertical" => Some(Self::VerticalRightToLeft),
            _ => None,
        }
    }
}

/// Paragraph alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Right,
    Center,
    Justify,
}

impl TextAlign {
    /// The XFL `alignment` attribute value.
    #[must_use]
    pub const fn xfl_name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Center => "center",
            Self::Justify => "justify",
        }
    }

    /// Parses an XFL `alignment` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "center" => Some(Self::Center),
            "justify" => Some(Self::Justify),
            _ => None,
        }
    }
}

/// Paragraph base direction. `Auto` follows the first strong character of the text (the bidi default), which is
/// what makes Hebrew and Arabic runs come out right-to-left without any flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextDirection {
    #[default]
    Auto,
    LeftToRight,
    RightToLeft,
}

/// One run of uniformly styled characters (`DOMTextRun`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    /// The text, any Unicode (bidi mixing is allowed inside one run).
    pub characters: String,
    /// Font face as Animate names it (a PostScript name such as `TimesNewRomanPSMT`, or a family).
    pub face: String,
    /// Size in points/pixels.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    /// Fill colour; its alpha byte is kept at 255 and [`TextRun::alpha`] carries the opacity.
    pub fill_color: Rgba8,
    /// Opacity 0.0 ..= 1.0 (`alpha`).
    pub alpha: f32,
    /// Extra advance between glyphs (`letterSpacing`).
    pub letter_spacing: f32,
    pub alignment: TextAlign,
    pub direction: TextDirection,
    pub extra: Extra,
}

impl TextRun {
    /// A black 12 point run with default styling.
    #[must_use]
    pub fn new(characters: impl Into<String>, face: impl Into<String>, size: f32) -> Self {
        Self {
            characters: characters.into(),
            face: face.into(),
            size,
            bold: false,
            italic: false,
            fill_color: Rgba8::BLACK,
            alpha: 1.0,
            letter_spacing: 0.0,
            alignment: TextAlign::Left,
            direction: TextDirection::Auto,
            extra: Extra::new(),
        }
    }
}

/// A text field (`DOMStaticText`, `DOMDynamicText`, `DOMInputText`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextElement {
    pub kind: TextKind,
    pub matrix: Affine,
    pub width: f64,
    pub height: f64,
    pub auto_expand: bool,
    pub orientation: TextOrientation,
    /// Instance name for dynamic and input fields.
    pub instance_name: Option<String>,
    pub text_runs: Vec<TextRun>,
    pub extra: Extra,
}

impl TextElement {
    /// A static text field with the given runs.
    #[must_use]
    pub const fn new(kind: TextKind, matrix: Affine, width: f64, height: f64, text_runs: Vec<TextRun>) -> Self {
        Self { kind, matrix, width, height, auto_expand: false, orientation: TextOrientation::Horizontal, instance_name: None, text_runs, extra: Extra::new() }
    }

    /// The concatenated characters of every run.
    #[must_use]
    pub fn plain_text(&self) -> String {
        self.text_runs.iter().map(|r| r.characters.as_str()).collect()
    }
}

/// A group of elements (`DOMGroup`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Group {
    pub members: Vec<Element>,
    pub matrix: Affine,
    pub transformation_point: Option<Point>,
    pub extra: Extra,
}

impl Group {
    /// A group at the origin.
    #[must_use]
    pub const fn new(members: Vec<Element>) -> Self {
        Self { members, matrix: Affine::IDENTITY, transformation_point: None, extra: Extra::new() }
    }
}

/// One thing in a keyframe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Element {
    Shape(ShapeElement),
    SymbolInstance(Box<SymbolInstance>),
    Bitmap(BitmapInstance),
    Text(Box<TextElement>),
    Group(Group),
    /// An element kind the model does not know (video instance, component, primitive...), kept verbatim.
    Unknown(RawXml),
}

impl Element {
    /// The element's matrix, or `None` for [`Element::Unknown`].
    #[must_use]
    pub const fn matrix(&self) -> Option<Affine> {
        match self {
            Self::Shape(s) => Some(s.matrix),
            Self::SymbolInstance(s) => Some(s.matrix),
            Self::Bitmap(b) => Some(b.matrix),
            Self::Text(t) => Some(t.matrix),
            Self::Group(g) => Some(g.matrix),
            Self::Unknown(_) => None,
        }
    }

    /// Replaces the matrix; returns false (and changes nothing) for [`Element::Unknown`].
    pub fn set_matrix(&mut self, matrix: Affine) -> bool {
        match self {
            Self::Shape(s) => s.matrix = matrix,
            Self::SymbolInstance(s) => s.set_matrix_2d(matrix),
            Self::Bitmap(b) => b.matrix = matrix,
            Self::Text(t) => t.matrix = matrix,
            Self::Group(g) => g.matrix = matrix,
            Self::Unknown(_) => return false,
        }
        true
    }

    /// Short lowercase name of the variant, for messages and tool output.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Shape(_) => "shape",
            Self::SymbolInstance(_) => "symbol instance",
            Self::Bitmap(_) => "bitmap",
            Self::Text(_) => "text",
            Self::Group(_) => "group",
            Self::Unknown(_) => "unknown",
        }
    }

    /// The symbol instance, if this is one.
    #[must_use]
    pub fn as_symbol_instance(&self) -> Option<&SymbolInstance> {
        match self {
            Self::SymbolInstance(s) => Some(s),
            _ => None,
        }
    }

    /// Mutable access to the symbol instance, if this is one.
    pub fn as_symbol_instance_mut(&mut self) -> Option<&mut SymbolInstance> {
        match self {
            Self::SymbolInstance(s) => Some(s),
            _ => None,
        }
    }

    /// Calls `f` on this element and on every element nested in groups, depth first, never deeper than
    /// [`crate::MAX_GROUP_DEPTH`].
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Self)) {
        self.walk_depth(f, 0);
    }

    fn walk_depth<'a>(&'a self, f: &mut impl FnMut(&'a Self), depth: usize) {
        f(self);
        if let Self::Group(g) = self
            && depth < crate::MAX_GROUP_DEPTH
        {
            for m in &g.members {
                m.walk_depth(f, depth.saturating_add(1));
            }
        }
    }

    /// Mutable counterpart of [`Element::walk`].
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Self)) {
        self.walk_mut_depth(f, 0);
    }

    fn walk_mut_depth(&mut self, f: &mut impl FnMut(&mut Self), depth: usize) {
        f(self);
        if let Self::Group(g) = self
            && depth < crate::MAX_GROUP_DEPTH
        {
            for m in &mut g.members {
                m.walk_mut_depth(f, depth.saturating_add(1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_effects_imply_transforms() {
        let mut i = SymbolInstance::new("a", Affine::IDENTITY);
        assert!(i.color.is_identity());
        i.set_color_effect(ColorEffect::Alpha(0.5));
        assert!((i.color.mul[3] - 0.5).abs() < 1e-6);
        i.set_color_effect(ColorEffect::None);
        assert!(i.color.is_identity());
        i.set_color_effect(ColorEffect::Brightness(-1.0));
        assert!(i.color.mul[0].abs() < 1e-6);
    }

    #[test]
    fn xfl_names_round_trip() {
        for k in [SymbolKind::Graphic, SymbolKind::MovieClip, SymbolKind::Button] {
            assert_eq!(SymbolKind::from_xfl_name(k.xfl_name()), Some(k));
        }
        for l in [LoopMode::Loop, LoopMode::PlayOnce, LoopMode::SingleFrame] {
            assert_eq!(LoopMode::from_xfl_name(l.xfl_name()), Some(l));
        }
        for a in [TextAlign::Left, TextAlign::Right, TextAlign::Center, TextAlign::Justify] {
            assert_eq!(TextAlign::from_xfl_name(a.xfl_name()), Some(a));
        }
        for o in [TextOrientation::VerticalLeftToRight, TextOrientation::VerticalRightToLeft] {
            assert_eq!(o.xfl_name().and_then(TextOrientation::from_xfl_name), Some(o));
        }
        assert_eq!(SymbolKind::from_xfl_name("nope"), None);
    }

    #[test]
    fn walk_visits_nested_groups_with_a_depth_cap() {
        let mut e = Element::SymbolInstance(Box::new(SymbolInstance::new("x", Affine::IDENTITY)));
        for _ in 0..(crate::MAX_GROUP_DEPTH * 2) {
            e = Element::Group(Group::new(vec![e]));
        }
        let mut n = 0;
        e.walk(&mut |_| n += 1);
        assert_eq!(n, crate::MAX_GROUP_DEPTH + 1);
        let mut m = 0;
        e.walk_mut(&mut |_| m += 1);
        assert_eq!(m, n);
    }

    #[test]
    fn matrix_accessors() {
        let mut e = Element::Bitmap(BitmapInstance::new("b", Affine::translate((3.0, 4.0))));
        assert_eq!(e.matrix(), Some(Affine::translate((3.0, 4.0))));
        assert!(e.set_matrix(Affine::IDENTITY));
        let mut u = Element::Unknown(RawXml { tag: "X".into(), xml: "<X/>".into() });
        assert!(!u.set_matrix(Affine::IDENTITY));
        assert_eq!(u.matrix(), None);
        assert_eq!(u.kind_name(), "unknown");
    }

    #[test]
    fn text_plain_text_and_rtl_survive() {
        let t =
            TextElement::new(TextKind::Dynamic, Affine::IDENTITY, 100.0, 20.0, vec![TextRun::new("שלום ", "Arial", 12.0), TextRun::new("abc", "Arial", 12.0)]);
        assert_eq!(t.plain_text(), "שלום abc");
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<TextElement>(&json).unwrap(), t);
    }
}
