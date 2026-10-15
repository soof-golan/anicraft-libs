//! Timelines, layers and frames.
//!
//! A [`Layer`] holds [`Frame`]s sorted by `index`; frame `i` covers `index .. index + duration`. Gaps are legal in
//! files written by other tools, so queries never assume the spans are contiguous.

use anim_core::{Point, Rgba8};
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::extra::Extra;

/// `keyMode` Animate writes for a plain keyframe.
pub const KEY_MODE_NORMAL: u32 = 9728;
/// `keyMode` Animate writes for a keyframe that starts a classic tween.
pub const KEY_MODE_CLASSIC_TWEEN: u32 = 22017;
/// `keyMode` Animate writes for a keyframe that starts a shape tween.
pub const KEY_MODE_SHAPE_TWEEN: u32 = 17922;

/// What a layer is.
///
/// XFL stores `layerType="guide" | "mask" | "folder" | "camera"` and expresses guided and masked layers as normal
/// layers whose `parentLayerIndex` points at the guide or mask layer. Readers may therefore store `Normal` for them;
/// [`Timeline::resolved_kind`] reports `Guided` / `Masked` by looking at the parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LayerKind {
    #[default]
    Normal,
    Guide,
    Guided,
    Mask,
    Masked,
    Folder,
    Camera,
}

impl LayerKind {
    /// The XFL `layerType` value, or `None` when XFL omits the attribute (normal, guided and masked layers).
    #[must_use]
    pub const fn xfl_name(self) -> Option<&'static str> {
        match self {
            Self::Normal | Self::Guided | Self::Masked => None,
            Self::Guide => Some("guide"),
            Self::Mask => Some("mask"),
            Self::Folder => Some("folder"),
            Self::Camera => Some("camera"),
        }
    }

    /// Parses an XFL `layerType` value (`normal` is accepted); unknown values give `None`.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "normal" => Some(Self::Normal),
            "guide" => Some(Self::Guide),
            "mask" => Some(Self::Mask),
            "folder" => Some(Self::Folder),
            "camera" => Some(Self::Camera),
            _ => None,
        }
    }

    /// True for layers that hold no frames' artwork of their own (folders and cameras).
    #[must_use]
    pub const fn is_structural(self) -> bool {
        matches!(self, Self::Folder | Self::Camera)
    }
}

/// Which kind of tween a keyframe starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TweenType {
    #[default]
    None,
    /// Classic tween (XFL `tweenType="motion"`).
    Classic,
    /// Shape tween (`tweenType="shape"`).
    Shape,
    /// Motion-object tween, stored in `<motionObjectXML>` (`tweenType="motion object"`).
    MotionObject,
}

impl TweenType {
    /// The XFL `tweenType` value, `None` when there is no tween.
    #[must_use]
    pub const fn xfl_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Classic => Some("motion"),
            Self::Shape => Some("shape"),
            Self::MotionObject => Some("motion object"),
        }
    }

    /// Parses an XFL `tweenType` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "motion" => Some(Self::Classic),
            "shape" => Some(Self::Shape),
            "motion object" => Some(Self::MotionObject),
            _ => None,
        }
    }
}

/// Rotation applied during a classic tween.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TweenRotate {
    /// No extra rotation (XFL omits `motionTweenRotate`).
    #[default]
    Auto,
    None,
    Clockwise,
    CounterClockwise,
}

impl TweenRotate {
    /// The XFL `motionTweenRotate` value, `None` for [`TweenRotate::Auto`].
    #[must_use]
    pub const fn xfl_name(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::None => Some("none"),
            Self::Clockwise => Some("clockwise"),
            Self::CounterClockwise => Some("counter-clockwise"),
        }
    }

    /// Parses an XFL `motionTweenRotate` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(Self::Auto),
            "none" => Some(Self::None),
            "clockwise" => Some(Self::Clockwise),
            "counter-clockwise" => Some(Self::CounterClockwise),
            _ => None,
        }
    }
}

/// What a `<tweens>` ease entry applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum EaseTarget {
    #[default]
    All,
    Position,
    Rotation,
    Scale,
    Color,
    Filters,
}

impl EaseTarget {
    /// The XFL `target` value.
    #[must_use]
    pub const fn xfl_name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Position => "position",
            Self::Rotation => "rotation",
            Self::Scale => "scale",
            Self::Color => "color",
            Self::Filters => "filters",
        }
    }

    /// Parses an XFL `target` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "all" => Some(Self::All),
            "position" => Some(Self::Position),
            "rotation" => Some(Self::Rotation),
            "scale" => Some(Self::Scale),
            "color" => Some(Self::Color),
            "filters" => Some(Self::Filters),
            _ => None,
        }
    }
}

/// The curve of one ease entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EaseCurve {
    /// A named preset such as `cubicIn` or `circInOut` (`<Ease method=...>`).
    Named(String),
    /// A custom cubic-bezier-chain: the points of `<CustomEase>`, `(0,0)` first and `(1,1)` last, as
    /// `[time, value]` pairs in 0.0 ..= 1.0 (control points between anchors, three per segment).
    Custom(Vec<[f64; 2]>),
}

/// One entry of a keyframe's `<tweens>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TweenEase {
    pub target: EaseTarget,
    pub curve: EaseCurve,
}

/// A shape hint pair (Modify > Shape > Add Shape Hint): a point on the start shape matched to a point on the end
/// shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapeHint {
    /// Hint letter index, 0 = `a`.
    pub id: u32,
    pub start: Point,
    pub end: Point,
}

/// Whether a frame label is a name, a comment or an anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LabelType {
    #[default]
    Name,
    Comment,
    Anchor,
}

impl LabelType {
    /// The XFL `labelType` value. [`Frame::label_type`] is an `Option` so a missing attribute stays missing.
    #[must_use]
    pub const fn xfl_name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Comment => "comment",
            Self::Anchor => "anchor",
        }
    }

    /// Parses an XFL `labelType` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "name" => Some(Self::Name),
            "comment" => Some(Self::Comment),
            "anchor" => Some(Self::Anchor),
            _ => None,
        }
    }
}

/// How a frame sound plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SoundSync {
    #[default]
    Event,
    Start,
    Stop,
    Stream,
}

impl SoundSync {
    /// The XFL `soundSync` value.
    #[must_use]
    pub const fn xfl_name(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Stream => "stream",
        }
    }

    /// Parses an XFL `soundSync` value.
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Option<Self> {
        match name {
            "event" => Some(Self::Event),
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "stream" => Some(Self::Stream),
            _ => None,
        }
    }
}

/// How often a frame sound repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SoundLoop {
    #[default]
    Once,
    /// Repeat this many times in total.
    Repeat(u32),
    /// Loop forever.
    Forever,
}

/// A point of a sound envelope (`SoundEnvelopePoint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopePoint {
    /// Sample position at 44.1 kHz.
    pub mark44: u32,
    /// Left level 0 ..= 32768.
    pub level0: u32,
    /// Right level 0 ..= 32768.
    pub level1: u32,
}

/// The sound attached to a keyframe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameSound {
    /// Library name of the sound item (`soundName`).
    pub name: String,
    pub sync: SoundSync,
    pub loop_mode: SoundLoop,
    /// Preset effect such as `fade in`, or `custom` when `envelope` is edited.
    pub effect: Option<String>,
    pub envelope: Vec<EnvelopePoint>,
    pub extra: Extra,
}

impl FrameSound {
    /// An event sound playing `name` once.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), sync: SoundSync::Event, loop_mode: SoundLoop::Once, effect: None, envelope: Vec::new(), extra: Extra::new() }
    }
}

/// A keyframe and the frames it holds until the next keyframe (`DOMFrame`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(clippy::struct_excessive_bools, reason = "mirrors the independent boolean attributes of XFL's DOMFrame")]
pub struct Frame {
    /// First frame of the span (0-based).
    pub index: u32,
    /// Number of frames in the span (XFL omits 1).
    pub duration: u32,
    /// Animate's `keyMode` bit field, kept as stored.
    pub key_mode: u32,
    pub tween_type: TweenType,
    pub motion_tween_rotate: TweenRotate,
    /// Extra full turns for clockwise / counter-clockwise rotation (`motionTweenRotateTimes`).
    pub motion_tween_rotate_times: Option<f64>,
    /// Scale during the tween (`motionTweenScale`, true by default).
    pub motion_tween_scale: bool,
    pub motion_tween_snap: bool,
    pub motion_tween_orient_to_path: bool,
    pub motion_tween_sync: bool,
    /// Classic ease "acceleration" in -100 ..= 100 (negative eases in, positive eases out).
    pub acceleration: i32,
    /// Named ease preset of the legacy `easeMethodName` attribute.
    pub ease_method_name: Option<String>,
    /// `<tweens>` entries: named and custom (bezier) eases per property target.
    pub eases: Vec<TweenEase>,
    pub shape_hints: Vec<ShapeHint>,
    /// The `<MorphShape>` element Animate caches for shape tweens, verbatim. Cleared by edits to the end shapes.
    pub morph_shape_xml: Option<String>,
    pub label: Option<String>,
    pub label_type: Option<LabelType>,
    pub sound: Option<FrameSound>,
    /// Frame script (`<Actionscript><script>`), verbatim.
    pub actionscript: Option<String>,
    /// `<motionObjectXML>` of a motion-object tween, verbatim.
    pub motion_object_xml: Option<String>,
    pub elements: Vec<Element>,
    pub extra: Extra,
}

impl Frame {
    /// A keyframe at `index` lasting `duration` frames, holding `elements`, with no tween.
    #[must_use]
    pub const fn new(index: u32, duration: u32, elements: Vec<Element>) -> Self {
        Self {
            index,
            duration,
            key_mode: KEY_MODE_NORMAL,
            tween_type: TweenType::None,
            motion_tween_rotate: TweenRotate::Auto,
            motion_tween_rotate_times: None,
            motion_tween_scale: true,
            motion_tween_snap: false,
            motion_tween_orient_to_path: false,
            motion_tween_sync: false,
            acceleration: 0,
            ease_method_name: None,
            eases: Vec::new(),
            shape_hints: Vec::new(),
            morph_shape_xml: None,
            label: None,
            label_type: None,
            sound: None,
            actionscript: None,
            motion_object_xml: None,
            elements,
            extra: Extra::new(),
        }
    }

    /// A keyframe with no content.
    #[must_use]
    pub const fn blank(index: u32, duration: u32) -> Self {
        Self::new(index, duration, Vec::new())
    }

    /// One past the last frame of the span.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.index.saturating_add(self.duration)
    }

    /// True when `frame` lies inside the span.
    #[must_use]
    pub const fn covers(&self, frame: u32) -> bool {
        frame >= self.index && frame < self.end()
    }

    /// The ease that applies to `target`: its own entry, else the `All` entry.
    #[must_use]
    pub fn ease_for(&self, target: EaseTarget) -> Option<&EaseCurve> {
        let own = self.eases.iter().find(|e| e.target == target);
        own.or_else(|| self.eases.iter().find(|e| e.target == EaseTarget::All)).map(|e| &e.curve)
    }

    /// True when the frame has no elements.
    #[must_use]
    pub const fn is_blank(&self) -> bool {
        self.elements.is_empty()
    }

    /// True when a tween of any kind starts here.
    #[must_use]
    pub const fn is_tweened(&self) -> bool {
        !matches!(self.tween_type, TweenType::None)
    }

    /// Removes every tween setting (type, ease, hints, cached morph data), keeping content and the label.
    pub fn clear_tween(&mut self) {
        self.tween_type = TweenType::None;
        self.key_mode = KEY_MODE_NORMAL;
        self.motion_tween_rotate = TweenRotate::Auto;
        self.motion_tween_rotate_times = None;
        self.motion_tween_scale = true;
        self.motion_tween_snap = false;
        self.motion_tween_orient_to_path = false;
        self.motion_tween_sync = false;
        self.acceleration = 0;
        self.ease_method_name = None;
        self.eases.clear();
        self.shape_hints.clear();
        self.morph_shape_xml = None;
        self.motion_object_xml = None;
    }

    /// A copy of this keyframe's content for a new keyframe at `index` lasting `duration`: elements are cloned,
    /// label, sound, script and tween settings are not (they belong to the original keyframe).
    #[must_use]
    pub fn duplicate_content_at(&self, index: u32, duration: u32) -> Self {
        Self::new(index, duration, self.elements.clone())
    }
}

/// A layer (`DOMLayer`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(clippy::struct_excessive_bools, reason = "mirrors the independent boolean attributes of XFL's DOMLayer")]
pub struct Layer {
    pub name: String,
    /// Outline colour shown in the timeline and on stage in outline mode.
    pub color: Rgba8,
    pub kind: LayerKind,
    /// Index of the folder, mask or guide layer this one belongs to; always smaller than this layer's own index.
    pub parent_layer_index: Option<usize>,
    pub locked: bool,
    pub visible: bool,
    /// Show as outlines only.
    pub outline: bool,
    /// The name was generated ("Layer 3") and not typed by the user (`autoNamed`).
    pub autoname: bool,
    /// Keyframes sorted by `index`.
    pub frames: Vec<Frame>,
    pub extra: Extra,
}

impl Layer {
    /// A visible, unlocked, normal layer without frames.
    #[must_use]
    pub fn new(name: impl Into<String>, color: Rgba8) -> Self {
        Self {
            name: name.into(),
            color,
            kind: LayerKind::Normal,
            parent_layer_index: None,
            locked: false,
            visible: true,
            outline: false,
            autoname: false,
            frames: Vec::new(),
            extra: Extra::new(),
        }
    }

    /// One past the last frame any keyframe covers.
    #[must_use]
    pub fn frame_count(&self) -> u32 {
        self.frames.iter().map(Frame::end).max().unwrap_or(0)
    }

    /// The keyframe whose span covers `frame`.
    ///
    /// Frames must be sorted by index (see [`Layer::sort_frames`]); the lookup is a binary search and never panics,
    /// whatever the data.
    #[must_use]
    pub fn keyframe_at(&self, frame: u32) -> Option<&Frame> {
        let pos = self.frames.partition_point(|f| f.index <= frame);
        let candidate = self.frames.get(pos.checked_sub(1)?)?;
        candidate.covers(frame).then_some(candidate)
    }

    /// Position in `frames` of the keyframe covering `frame`.
    #[must_use]
    pub fn keyframe_position_at(&self, frame: u32) -> Option<usize> {
        let pos = self.frames.partition_point(|f| f.index <= frame).checked_sub(1)?;
        self.frames.get(pos)?.covers(frame).then_some(pos)
    }

    /// Mutable counterpart of [`Layer::keyframe_at`].
    pub fn keyframe_at_mut(&mut self, frame: u32) -> Option<&mut Frame> {
        let pos = self.keyframe_position_at(frame)?;
        self.frames.get_mut(pos)
    }

    /// True when a keyframe starts exactly at `frame`.
    #[must_use]
    pub fn is_keyframe(&self, frame: u32) -> bool {
        self.keyframe_at(frame).is_some_and(|f| f.index == frame)
    }

    /// The keyframe that starts after the one covering `frame` (the tween target).
    #[must_use]
    pub fn next_keyframe_after(&self, frame: u32) -> Option<&Frame> {
        let pos = self.frames.partition_point(|f| f.index <= frame);
        self.frames.get(pos)
    }

    /// Sorts the keyframes by index (stable), which every lookup relies on.
    pub fn sort_frames(&mut self) {
        self.frames.sort_by_key(|f| f.index);
    }

    /// True when keyframes are sorted and none overlaps the next.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.frames.windows(2).all(|w| match w {
            [a, b] => a.index < b.index && a.end() <= b.index,
            _ => true,
        })
    }
}

/// One timeline: a scene or a symbol's own (`DOMTimeline`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub name: String,
    /// Layers top to bottom, as the timeline panel shows them.
    pub layers: Vec<Layer>,
    /// Playhead position saved with the file (`currentFrame`).
    pub current_frame: u32,
    /// `layerDepthEnabled`.
    pub layer_depth_enabled: Option<bool>,
    pub extra: Extra,
}

impl Timeline {
    /// An empty timeline.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), layers: Vec::new(), current_frame: 0, layer_depth_enabled: None, extra: Extra::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer_with(spans: &[(u32, u32)]) -> Layer {
        let mut l = Layer::new("L", Rgba8::BLACK);
        for &(i, d) in spans {
            l.frames.push(Frame::blank(i, d));
        }
        l
    }

    #[test]
    fn keyframe_lookup_handles_spans_gaps_and_ends() {
        let l = layer_with(&[(0, 3), (3, 1), (6, 4)]);
        assert_eq!(l.keyframe_at(0).map(|f| f.index), Some(0));
        assert_eq!(l.keyframe_at(2).map(|f| f.index), Some(0));
        assert_eq!(l.keyframe_at(3).map(|f| f.index), Some(3));
        assert!(l.keyframe_at(4).is_none());
        assert!(l.keyframe_at(5).is_none());
        assert_eq!(l.keyframe_at(9).map(|f| f.index), Some(6));
        assert!(l.keyframe_at(10).is_none());
        assert_eq!(l.frame_count(), 10);
        assert!(l.is_keyframe(3));
        assert!(!l.is_keyframe(2));
        assert_eq!(l.next_keyframe_after(0).map(|f| f.index), Some(3));
        assert_eq!(l.next_keyframe_after(4).map(|f| f.index), Some(6));
        assert!(l.next_keyframe_after(6).is_none());
        assert!(l.is_well_formed());
    }

    #[test]
    fn empty_layer_has_no_frames() {
        let l = layer_with(&[]);
        assert!(l.keyframe_at(0).is_none());
        assert_eq!(l.frame_count(), 0);
        assert!(l.is_well_formed());
    }

    #[test]
    fn hostile_spans_do_not_panic() {
        let mut l = layer_with(&[(u32::MAX, u32::MAX), (5, 0), (0, u32::MAX)]);
        l.sort_frames();
        let _ = l.keyframe_at(u32::MAX);
        let _ = l.keyframe_at(0);
        assert!(!l.is_well_formed());
        assert_eq!(l.frame_count(), u32::MAX);
    }

    #[test]
    fn clear_tween_resets_settings() {
        let mut f = Frame::blank(0, 5);
        f.tween_type = TweenType::Classic;
        f.key_mode = KEY_MODE_CLASSIC_TWEEN;
        f.acceleration = 40;
        f.eases.push(TweenEase { target: EaseTarget::All, curve: EaseCurve::Named("cubicIn".into()) });
        f.label = Some("x".into());
        f.clear_tween();
        assert!(!f.is_tweened());
        assert_eq!(f.acceleration, 0);
        assert!(f.eases.is_empty());
        assert_eq!(f.key_mode, KEY_MODE_NORMAL);
        assert_eq!(f.label.as_deref(), Some("x"));
    }

    #[test]
    fn enum_names_round_trip() {
        for k in [LayerKind::Guide, LayerKind::Mask, LayerKind::Folder, LayerKind::Camera] {
            assert_eq!(k.xfl_name().and_then(LayerKind::from_xfl_name), Some(k));
        }
        assert_eq!(LayerKind::from_xfl_name("normal"), Some(LayerKind::Normal));
        for t in [TweenType::Classic, TweenType::Shape, TweenType::MotionObject] {
            assert_eq!(t.xfl_name().and_then(TweenType::from_xfl_name), Some(t));
        }
        for t in [EaseTarget::All, EaseTarget::Position, EaseTarget::Rotation, EaseTarget::Scale, EaseTarget::Color, EaseTarget::Filters] {
            assert_eq!(EaseTarget::from_xfl_name(t.xfl_name()), Some(t));
        }
        for s in [SoundSync::Event, SoundSync::Start, SoundSync::Stop, SoundSync::Stream] {
            assert_eq!(SoundSync::from_xfl_name(s.xfl_name()), Some(s));
        }
        for l in [LabelType::Name, LabelType::Comment, LabelType::Anchor] {
            assert_eq!(LabelType::from_xfl_name(l.xfl_name()), Some(l));
        }
        for r in [TweenRotate::None, TweenRotate::Clockwise, TweenRotate::CounterClockwise] {
            assert_eq!(r.xfl_name().and_then(TweenRotate::from_xfl_name), Some(r));
        }
    }
}
