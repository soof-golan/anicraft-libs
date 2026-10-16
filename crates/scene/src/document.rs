//! The document: stage settings, scenes and the library.

use anim_core::projective::DEFAULT_PERSPECTIVE_ANGLE;
use anim_core::{FrameRate, Perspective, Point, Rgba8, Tick};
use serde::{Deserialize, Serialize};

use crate::element::SymbolKind;
use crate::extra::Extra;
use crate::library::{Library, LibraryItem};
use crate::timeline::{Frame, Layer, Timeline};

/// Which timeline an edit or query addresses: a scene by position, or a symbol's timeline by library name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimelineId {
    Scene(usize),
    Symbol(String),
}

/// Layer outline colours Animate cycles through for new layers.
pub const LAYER_COLORS: [Rgba8; 8] = [
    Rgba8::opaque(0x4F, 0x80, 0xFF),
    Rgba8::opaque(0xFF, 0x4F, 0x4F),
    Rgba8::opaque(0x4F, 0xFF, 0x4F),
    Rgba8::opaque(0x99, 0x33, 0xCC),
    Rgba8::opaque(0xFF, 0x80, 0x00),
    Rgba8::opaque(0x00, 0xFF, 0xFF),
    Rgba8::opaque(0xFF, 0x00, 0xFF),
    Rgba8::opaque(0xFF, 0xFF, 0x00),
];

/// An Animate document (`DOMDocument`) minus the media bytes, which live in [`crate::MediaStore`].
///
/// `Clone` is cheap enough for undo snapshots: everything is plain data and no blob lives here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Stage width in pixels.
    pub width: f64,
    /// Stage height in pixels.
    pub height: f64,
    /// Frames per second as stored (`frameRate`, may be fractional such as 29.97).
    pub frame_rate: f64,
    pub background_color: Rgba8,
    /// Perspective angle (field of view) in degrees for 3D instances, 1 ..= 179; Animate's default is 55.
    pub perspective_angle: f64,
    /// The vanishing point of 3D instances in stage pixels; `None` is the centre of the stage.
    pub vanishing_point: Option<Point>,
    /// Index into `scenes` of the timeline that was open when the file was saved (XFL stores it 1-based; this is
    /// 0-based).
    pub current_timeline: usize,
    /// Scenes in playback order.
    pub scenes: Vec<Timeline>,
    pub library: Library,
    /// `PublishSettings.xml`, verbatim.
    pub publish_settings_xml: Option<String>,
    /// Unknown attributes (`xflVersion`, `creatorInfo`, GUIDs, ...) and unknown children (`swatchLists`,
    /// `PrinterSettings`, `publishHistory`, `scripts`, ...).
    pub extra: Extra,
}

impl Default for Document {
    fn default() -> Self {
        Self::new(550.0, 400.0, 24.0)
    }
}

impl Document {
    /// A new document with one scene ("Scene 1") that has one layer holding a blank keyframe.
    #[must_use]
    pub fn new(width: f64, height: f64, frame_rate: f64) -> Self {
        let mut scene = Timeline::new("Scene 1");
        let mut layer = Layer::new("Layer_1", LAYER_COLORS[0]);
        layer.autoname = true;
        layer.frames.push(Frame::blank(0, 1));
        scene.layers.push(layer);
        Self {
            width,
            height,
            frame_rate,
            background_color: Rgba8::WHITE,
            perspective_angle: DEFAULT_PERSPECTIVE_ANGLE,
            vanishing_point: None,
            current_timeline: 0,
            scenes: vec![scene],
            library: Library::new(),
            publish_settings_xml: None,
            extra: Extra::new(),
        }
    }

    /// The view 3D instances are projected with: the document's perspective angle and vanishing point over the stage.
    #[must_use]
    pub fn perspective(&self) -> Perspective {
        let mut perspective = Perspective::for_stage(self.width, self.height);
        perspective.angle = self.perspective_angle;
        if let Some(point) = self.vanishing_point {
            perspective.vanishing_point = point;
        }
        perspective
    }

    /// The frame rate as an exact rational ([`FrameRate::from_f64`]), falling back to 12 fps (Animate's classic
    /// default) when the stored value is not a usable number.
    #[must_use]
    pub fn frame_rate_exact(&self) -> FrameRate {
        if self.frame_rate.is_finite() && self.frame_rate >= 0.01 && self.frame_rate <= 1000.0 {
            FrameRate::from_f64(self.frame_rate)
        } else {
            FrameRate::new(12, 1)
        }
    }

    /// Start time of `frame` on the document's clock.
    #[must_use]
    pub fn frame_to_tick(&self, frame: u32) -> Tick {
        self.frame_rate_exact().tick_of(i64::from(frame))
    }

    /// The frame that contains time `t` (negative times give frame 0).
    #[must_use]
    pub fn tick_to_frame(&self, t: Tick) -> u32 {
        u32::try_from(self.frame_rate_exact().frame_at(t).max(0)).unwrap_or(u32::MAX)
    }

    /// The timeline `id` names.
    #[must_use]
    pub fn timeline(&self, id: &TimelineId) -> Option<&Timeline> {
        match id {
            TimelineId::Scene(i) => self.scenes.get(*i),
            TimelineId::Symbol(name) => self.library.find_symbol(name).map(|s| &s.timeline),
        }
    }

    /// Mutable access to the timeline `id` names.
    pub fn timeline_mut(&mut self, id: &TimelineId) -> Option<&mut Timeline> {
        match id {
            TimelineId::Scene(i) => self.scenes.get_mut(*i),
            TimelineId::Symbol(name) => self.library.find_symbol_mut(name).map(|s| &mut s.timeline),
        }
    }

    /// The timeline that was current when saved (falls back to the first scene).
    #[must_use]
    pub fn current_scene(&self) -> Option<&Timeline> {
        self.scenes.get(self.current_timeline).or_else(|| self.scenes.first())
    }

    /// Position of the scene called `name`.
    #[must_use]
    pub fn scene_index(&self, name: &str) -> Option<usize> {
        self.scenes.iter().position(|s| s.name == name)
    }

    /// Ids of every timeline: scenes first, then symbols in library order.
    #[must_use]
    pub fn timeline_ids(&self) -> Vec<TimelineId> {
        let scenes = (0..self.scenes.len()).map(TimelineId::Scene);
        let symbols = self.library.symbols().map(|s| TimelineId::Symbol(s.name.clone()));
        scenes.chain(symbols).collect()
    }

    /// Length in frames of the longest scene, summed over scenes (the movie's total length).
    #[must_use]
    pub fn total_frames(&self) -> u32 {
        self.scenes.iter().map(Timeline::frame_count).fold(0_u32, u32::saturating_add)
    }

    /// Adds a symbol to the library with a fresh id, creating its folders, and returns its name.
    ///
    /// # Errors
    /// Name errors as for [`Library::push`].
    pub fn add_symbol(&mut self, name: &str, kind: SymbolKind) -> Result<(), crate::SceneError> {
        crate::library::validate_item_name(name)?;
        if self.library.find_index(name).is_some() {
            return Err(crate::SceneError::NameTaken(name.to_owned()));
        }
        crate::edit::ensure_folders(&mut self.library, name);
        let id = self.library.fresh_item_id(name);
        let mut symbol = crate::library::Symbol::new(name, kind, id);
        let mut layer = Layer::new("Layer_1", LAYER_COLORS[0]);
        layer.autoname = true;
        layer.frames.push(Frame::blank(0, 1));
        symbol.timeline.layers.push(layer);
        self.library.items.push(LibraryItem::Symbol(Box::new(symbol)));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_has_one_scene_one_layer() {
        let d = Document::new(1920.0, 1080.0, 30.0);
        assert_eq!(d.scenes.len(), 1);
        assert_eq!(d.total_frames(), 1);
        assert_eq!(d.frame_rate_exact(), FrameRate::FPS_30);
        assert!(d.current_scene().is_some());
        assert_eq!(d.scene_index("Scene 1"), Some(0));
        assert_eq!(d.timeline_ids(), vec![TimelineId::Scene(0)]);
    }

    #[test]
    fn bad_frame_rates_fall_back() {
        for fps in [f64::NAN, 0.0, -3.0, f64::INFINITY, 1e9] {
            let mut d = Document::default();
            d.frame_rate = fps;
            assert_eq!(d.frame_rate_exact(), FrameRate::new(12, 1));
        }
        let mut d = Document::default();
        d.frame_rate = 29.97;
        assert!((d.frame_rate_exact().as_f64() - 29.97).abs() < 0.01);
    }

    #[test]
    fn add_symbol_creates_folders_and_timeline() {
        let mut d = Document::default();
        d.add_symbol("a/b/hat", SymbolKind::MovieClip).unwrap();
        assert!(d.library.find_symbol("a/b/hat").is_some());
        assert_eq!(d.library.folders().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["a", "a/b"]);
        assert_eq!(d.add_symbol("a/b/hat", SymbolKind::Graphic), Err(crate::SceneError::NameTaken("a/b/hat".into())));
        let id = TimelineId::Symbol("a/b/hat".into());
        assert_eq!(d.timeline(&id).map(Timeline::frame_count), Some(1));
        assert!(d.timeline_mut(&TimelineId::Symbol("nope".into())).is_none());
        assert_eq!(d.timeline_ids().len(), 2);
    }
}
