//! The property tracks of a motion tween (`tweenType="motion object"`).
//!
//! Model, reader and writer of `<motionObjectXML>`, and the editing functions behind Create Motion Tween, the Motion
//! Editor and motion presets.
//!
//! Animate stores `<Property id="Motion_X">` (also `Motion_Y`, `Rotation_Z`, `Scale_X`, `Scale_Y`) holding
//! `<Keyframe timevalue=".." value="..">` children under an `<AnimationCore duration="..">` root. Key times are
//! normalised to `0.0 ..= 1.0` over the tween span here, so stretching a span stretches its keys. Values are what
//! the file stores; the evaluator applies the *change* of every track relative to its first key to the object in
//! the span's keyframe, so the first key is the object itself and cannot be edited on its own.
//!
//! The writer adds two attributes to a `<Keyframe>`: `ease` (the classic acceleration -100 ..= 100 of the segment
//! that starts at the key) and `curve` (a custom bezier chain `x,y x,y ...` replacing it).

use std::fmt::Write;

use anim_core::Affine;

use crate::document::{Document, TimelineId};
use crate::edit::keyframe_mut;
use crate::element::Element;
use crate::error::SceneError;
use crate::timeline::{Frame, KEY_MODE_CLASSIC_TWEEN, TweenType};

/// Most `Property` and `Keyframe` elements read from one document.
const MAX_NODES: u32 = 50_000;
/// Time units per tween the writer uses (`TimeScale` 24000, one second at 24 fps).
const TIME_UNITS: f64 = 24_000.0;

/// What a track changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionProp {
    X,
    Y,
    /// Degrees, clockwise.
    Rotation,
    /// Percent.
    ScaleX,
    /// Percent.
    ScaleY,
    /// Alpha percent of the colour effect.
    Alpha,
}

impl MotionProp {
    /// Every property, in the order the Motion Editor lists them.
    pub const ALL: [Self; 6] = [Self::X, Self::Y, Self::Rotation, Self::ScaleX, Self::ScaleY, Self::Alpha];

    /// The `Property id` Animate uses.
    #[must_use]
    pub const fn xfl_id(self) -> &'static str {
        match self {
            Self::X => "Motion_X",
            Self::Y => "Motion_Y",
            Self::Rotation => "Rotation_Z",
            Self::ScaleX => "Scale_X",
            Self::ScaleY => "Scale_Y",
            Self::Alpha => "Color_Alpha",
        }
    }

    /// The name shown in the Motion Editor and used by commands.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Rotation => "rotation",
            Self::ScaleX => "scale_x",
            Self::ScaleY => "scale_y",
            Self::Alpha => "alpha",
        }
    }

    /// Parses a [`MotionProp::label`].
    #[must_use]
    pub fn from_label(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.label() == name)
    }

    /// Reads a `Property id` (close variants included).
    #[must_use]
    pub fn from_xfl_id(id: &str) -> Option<Self> {
        let id = id.to_ascii_lowercase();
        let axis = id.rsplit(['_', ' ', '.']).next().unwrap_or("");
        if id.contains("rotat") {
            return Some(Self::Rotation);
        }
        if id.contains("alpha") {
            return Some(Self::Alpha);
        }
        if id.contains("scale") {
            return match axis {
                "x" => Some(Self::ScaleX),
                "y" => Some(Self::ScaleY),
                _ => None,
            };
        }
        if id.contains("skew") {
            return None;
        }
        if id.starts_with("motion") || id.contains("position") || id.contains("translat") {
            return match axis {
                "x" => Some(Self::X),
                "y" => Some(Self::Y),
                _ => None,
            };
        }
        None
    }

    /// The value of this property on an element with `matrix` and the given alpha percent.
    #[must_use]
    pub fn of_matrix(self, matrix: Affine, alpha_percent: f64) -> f64 {
        let [a, b, c, d, tx, ty] = matrix.as_coeffs();
        match self {
            Self::X => tx,
            Self::Y => ty,
            Self::Rotation => b.atan2(a).to_degrees(),
            Self::ScaleX => a.hypot(b) * 100.0,
            Self::ScaleY => c.hypot(d) * 100.0,
            Self::Alpha => alpha_percent,
        }
    }
}

/// A key as read: time, value, ease, custom curve.
type RawKey = (f64, f64, i32, Option<Vec<[f64; 2]>>);

/// One property keyframe.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionKey {
    /// Position over the span, 0.0 ..= 1.0.
    pub t: f64,
    pub value: f64,
    /// Classic acceleration of the segment that starts here.
    pub ease: i32,
    /// Custom bezier chain replacing `ease` (`1 + 3k` points `[time, value]`).
    pub curve: Option<Vec<[f64; 2]>>,
}

/// The keys of one property, sorted by time.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionTrack {
    pub prop: MotionProp,
    pub keys: Vec<MotionKey>,
}

/// All the tracks of a motion tween.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MotionObject {
    pub tracks: Vec<MotionTrack>,
}

impl MotionObject {
    /// Reads `xml`. `None` when it is not XML or holds no usable track.
    #[must_use]
    pub fn parse(xml: &str) -> Option<Self> {
        let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: MAX_NODES, ..roxmltree::ParsingOptions::default() };
        let doc = roxmltree::Document::parse_with_options(xml, options).ok()?;
        let declared = doc.descendants().find_map(|n| n.attribute("duration")).and_then(|d| d.trim().parse::<f64>().ok()).filter(|d| *d > 0.0);
        let mut raw: Vec<(MotionProp, Vec<RawKey>)> = Vec::new();
        for prop in doc.descendants().filter(|n| n.has_tag_name("Property")) {
            let Some(channel) = prop.attribute("id").and_then(MotionProp::from_xfl_id) else { continue };
            if raw.iter().any(|(c, _)| *c == channel) {
                continue;
            }
            let mut points: Vec<_> = prop
                .children()
                .filter(|k| k.has_tag_name("Keyframe"))
                .filter_map(|k| {
                    let time = k.attribute("timevalue").or_else(|| k.attribute("time"))?.trim().parse::<f64>().ok()?;
                    let value = keyframe_value(k)?;
                    let ease = k.attribute("ease").and_then(|e| e.trim().parse::<i32>().ok()).map_or(0, |e| e.clamp(-100, 100));
                    let curve = k.attribute("curve").and_then(parse_curve);
                    (time.is_finite() && value.is_finite()).then_some((time, value, ease, curve))
                })
                .collect();
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            if !points.is_empty() {
                raw.push((channel, points));
            }
        }
        if raw.is_empty() {
            return None;
        }
        let max_time = raw.iter().flat_map(|(_, p)| p.iter().map(|q| q.0)).fold(0.0f64, f64::max);
        let total = declared.unwrap_or(max_time).max(max_time);
        if total <= 0.0 {
            // All keys at time zero: nothing moves.
            return None;
        }
        let tracks = raw
            .into_iter()
            .map(|(prop, p)| MotionTrack { prop, keys: p.into_iter().map(|(t, value, ease, curve)| MotionKey { t: t / total, value, ease, curve }).collect() })
            .collect();
        Some(Self { tracks })
    }

    /// The `<AnimationCore>` document of these tracks.
    #[must_use]
    pub fn to_xml(&self) -> String {
        let mut out = format!("<AnimationCore TimeScale=\"{TIME_UNITS}\" duration=\"{TIME_UNITS}\"><PropertyContainer id=\"Basic_Motion\">");
        for track in &self.tracks {
            let _ = write!(out, "<Property id=\"{}\">", track.prop.xfl_id());
            for key in &track.keys {
                let _ = write!(out, "<Keyframe timevalue=\"{}\" value=\"{}\"", (key.t.clamp(0.0, 1.0) * TIME_UNITS).round(), key.value);
                if key.ease != 0 {
                    let _ = write!(out, " ease=\"{}\"", key.ease);
                }
                if let Some(curve) = &key.curve {
                    let text: Vec<String> = curve.iter().map(|p| format!("{},{}", p[0], p[1])).collect();
                    let _ = write!(out, " curve=\"{}\"", text.join(" "));
                }
                out.push_str("/>");
            }
            out.push_str("</Property>");
        }
        out.push_str("</PropertyContainer></AnimationCore>");
        out
    }

    /// The track of `prop`.
    #[must_use]
    pub fn track(&self, prop: MotionProp) -> Option<&MotionTrack> {
        self.tracks.iter().find(|t| t.prop == prop)
    }

    /// Sets the key of `prop` at `t` (replacing one within a hair of it). A missing track starts with its base key.
    fn set_key(&mut self, prop: MotionProp, base: f64, key: MotionKey) {
        let index = self.tracks.iter().position(|t| t.prop == prop).unwrap_or_else(|| {
            self.tracks.push(MotionTrack { prop, keys: vec![MotionKey { t: 0.0, value: base, ease: 0, curve: None }] });
            self.tracks.len().saturating_sub(1)
        });
        let Some(track) = self.tracks.get_mut(index) else { return };
        if let Some(existing) = track.keys.iter_mut().find(|k| (k.t - key.t).abs() < 1e-6) {
            *existing = key;
        } else {
            track.keys.push(key);
            track.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
    }
}

fn parse_curve(text: &str) -> Option<Vec<[f64; 2]>> {
    let points: Option<Vec<[f64; 2]>> = text
        .split_whitespace()
        .take(1000)
        .map(|pair| {
            let (x, y) = pair.split_once(',')?;
            let (x, y) = (x.parse::<f64>().ok()?, y.parse::<f64>().ok()?);
            (x.is_finite() && y.is_finite()).then_some([x, y])
        })
        .collect();
    points.filter(|p| p.len() >= 2)
}

fn keyframe_value(k: roxmltree::Node<'_, '_>) -> Option<f64> {
    let from_attr = |name: &str| k.attribute(name).and_then(|v| v.split(',').next()).and_then(|v| v.trim().parse::<f64>().ok());
    from_attr("value")
        .or_else(|| from_attr("anchor"))
        .or_else(|| k.children().find(|c| c.has_tag_name("Value")).and_then(|c| c.text()).and_then(|t| t.trim().parse::<f64>().ok()))
}

// ---------------------------------------------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------------------------------------------

/// The alpha percent of the single element of a keyframe (100 for anything but a symbol instance).
fn element_alpha(element: &Element) -> f64 {
    element.as_symbol_instance().map_or(100.0, |i| f64::from(i.color.mul[3]) * 100.0)
}

/// The base values (matrix properties and alpha) of the one object of a motion tween's keyframe.
fn base_value(frame: &Frame, prop: MotionProp) -> f64 {
    frame.elements.first().map_or(0.0, |e| prop.of_matrix(e.matrix().unwrap_or(Affine::IDENTITY), element_alpha(e)))
}

/// Number of steps of a motion tween span: the last frame is `steps` frames after the first.
const fn steps(frame: &Frame) -> u32 {
    frame.duration.saturating_sub(1)
}

fn motion_frame<'a>(doc: &'a mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<&'a mut Frame, SceneError> {
    let k = keyframe_mut(doc, id, layer, frame)?;
    if k.tween_type == TweenType::MotionObject { Ok(k) } else { Err(SceneError::NotTweenable("the keyframe does not start a motion tween")) }
}

/// The tracks of the motion tween starting at the keyframe covering `frame`.
#[must_use]
pub fn motion_object(keyframe: &Frame) -> Option<MotionObject> {
    (keyframe.tween_type == TweenType::MotionObject).then(|| keyframe.motion_object_xml.as_deref().and_then(MotionObject::parse)).flatten()
}

/// Create Motion Tween: the keyframe covering `frame` becomes a motion tween over its whole span, which must be at
/// least two frames long. The content must be a single symbol instance, group or text field.
///
/// # Errors
/// [`SceneError::NotTweenable`] for unsuitable content or a one-frame span.
pub fn create_motion_tween(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<(), SceneError> {
    let k = keyframe_mut(doc, id, layer, frame)?;
    match k.elements.as_slice() {
        [Element::SymbolInstance(_) | Element::Group(_) | Element::Text(_)] => {}
        [Element::Shape(_)] => return Err(SceneError::NotTweenable("convert the shape to a symbol first")),
        [Element::Bitmap(_)] => return Err(SceneError::NotTweenable("convert the bitmap to a symbol first")),
        [] => return Err(SceneError::NotTweenable("the keyframe is empty")),
        [Element::Unknown(_)] => return Err(SceneError::NotTweenable("unknown content cannot be tweened")),
        _ => return Err(SceneError::NotTweenable("a motion tween needs exactly one object per layer")),
    }
    if k.duration < 2 {
        return Err(SceneError::NotTweenable("a motion tween needs a span of at least two frames; insert frames first"));
    }
    let base = |p| base_value(k, p);
    let tracks = [MotionProp::X, MotionProp::Y, MotionProp::Rotation, MotionProp::ScaleX, MotionProp::ScaleY]
        .into_iter()
        .map(|prop| MotionTrack { prop, keys: vec![MotionKey { t: 0.0, value: base(prop), ease: 0, curve: None }] })
        .collect();
    let xml = MotionObject { tracks }.to_xml();
    k.clear_tween();
    k.tween_type = TweenType::MotionObject;
    k.key_mode = KEY_MODE_CLASSIC_TWEEN;
    k.motion_object_xml = Some(xml);
    Ok(())
}

fn offset_to_t(k: &Frame, offset: u32) -> Result<f64, SceneError> {
    let steps = steps(k);
    if offset > steps {
        return Err(SceneError::InvalidRange("the key is past the end of the tween span"));
    }
    Ok(if steps == 0 { 0.0 } else { f64::from(offset) / f64::from(steps) })
}

fn edit_object(k: &mut Frame, f: impl FnOnce(&mut MotionObject, &Frame) -> Result<(), SceneError>) -> Result<(), SceneError> {
    let mut object = motion_object(k).ok_or(SceneError::NotTweenable("the motion tween has no readable tracks"))?;
    f(&mut object, k)?;
    k.motion_object_xml = Some(object.to_xml());
    Ok(())
}

/// Sets a property keyframe of the motion tween covering `frame`.
///
/// `offset` frames after the span start the object shows `value` (stage pixels, degrees or percent) for `prop`. Adds
/// the key or replaces the one there.
///
/// # Errors
/// Not a motion tween, `offset` past the span, a non-finite `value`, or `offset` 0 (the first key is the object).
pub fn set_motion_key(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, prop: MotionProp, offset: u32, value: f64) -> Result<(), SceneError> {
    if !value.is_finite() {
        return Err(SceneError::InvalidRange("the value is not a number"));
    }
    if offset == 0 {
        return Err(SceneError::InvalidRange("the first keyframe is the object itself; move the object instead"));
    }
    let k = motion_frame(doc, id, layer, frame)?;
    let t = offset_to_t(k, offset)?;
    edit_object(k, |object, k| {
        let base = base_value(k, prop);
        let first = object.track(prop).and_then(|tr| tr.keys.first()).map_or(base, |key| key.value);
        // Stored values are in the file's frame of reference: the first key stands for the object.
        let stored = first + (value - base);
        let (ease, curve) =
            object.track(prop).and_then(|tr| tr.keys.iter().find(|key| (key.t - t).abs() < 1e-6)).map_or((0, None), |key| (key.ease, key.curve.clone()));
        object.set_key(prop, base, MotionKey { t, value: stored, ease, curve });
        Ok(())
    })
}

/// Removes the property key of `prop` at `offset` frames into the span.
///
/// # Errors
/// Not a motion tween, no such key, or `offset` 0.
pub fn remove_motion_key(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, prop: MotionProp, offset: u32) -> Result<(), SceneError> {
    if offset == 0 {
        return Err(SceneError::InvalidRange("the first keyframe cannot be removed"));
    }
    let k = motion_frame(doc, id, layer, frame)?;
    let t = offset_to_t(k, offset)?;
    edit_object(k, |object, _| {
        let track = object.tracks.iter_mut().find(|tr| tr.prop == prop).ok_or(SceneError::InvalidRange("no keys for that property"))?;
        let before = track.keys.len();
        track.keys.retain(|key| (key.t - t).abs() >= 1e-6);
        if track.keys.len() == before {
            return Err(SceneError::InvalidRange("no key at that frame"));
        }
        Ok(())
    })
}

/// The ease of a motion key's segment: a classic acceleration, or a custom bezier chain which wins over it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyEase {
    pub acceleration: i32,
    pub curve: Option<Vec<[f64; 2]>>,
}

/// Sets the ease of the segment that starts at the key of `prop` at `offset`.
///
/// # Errors
/// Not a motion tween, no such key, acceleration outside -100 ..= 100, or an unusable curve.
pub fn set_motion_key_ease(
    doc: &mut Document,
    id: &TimelineId,
    layer: usize,
    frame: u32,
    prop: MotionProp,
    offset: u32,
    key_ease: KeyEase,
) -> Result<(), SceneError> {
    let KeyEase { acceleration: ease, curve } = key_ease;
    if !(-100..=100).contains(&ease) {
        return Err(SceneError::InvalidRange("acceleration must be between -100 and 100"));
    }
    if let Some(c) = &curve
        && (c.len() < 2 || c.len() > 1000 || c.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()))
    {
        return Err(SceneError::InvalidRange("a custom ease needs at least two finite points"));
    }
    let k = motion_frame(doc, id, layer, frame)?;
    let t = offset_to_t(k, offset)?;
    edit_object(k, |object, _| {
        let key = object
            .tracks
            .iter_mut()
            .find(|tr| tr.prop == prop)
            .and_then(|tr| tr.keys.iter_mut().find(|key| (key.t - t).abs() < 1e-6))
            .ok_or(SceneError::InvalidRange("no key at that frame"))?;
        key.ease = ease;
        key.curve = curve;
        Ok(())
    })
}

/// A motion preset: relative changes over the span.
#[derive(Debug, Clone, Copy)]
pub struct MotionPreset {
    pub name: &'static str,
    pub description: &'static str,
    /// `(property, t, change, ease)`. The change is added for x, y, rotation and alpha (alpha percent points), and
    /// is a multiplier for the scales.
    pub keys: &'static [(MotionProp, f64, f64, i32)],
}

/// The built-in presets, applied with [`apply_motion_preset`].
pub const MOTION_PRESETS: &[MotionPreset] = &[
    MotionPreset { name: "fly-in-left", description: "Slides in from 300 px left", keys: &[(MotionProp::X, 0.0, -300.0, 0), (MotionProp::X, 1.0, 0.0, 60)] },
    MotionPreset { name: "fly-in-top", description: "Drops in from 200 px above", keys: &[(MotionProp::Y, 0.0, -200.0, 0), (MotionProp::Y, 1.0, 0.0, 60)] },
    MotionPreset { name: "fade-in", description: "Fades from transparent", keys: &[(MotionProp::Alpha, 0.0, -100.0, 0), (MotionProp::Alpha, 1.0, 0.0, 0)] },
    MotionPreset { name: "fade-out", description: "Fades to transparent", keys: &[(MotionProp::Alpha, 0.0, 0.0, 0), (MotionProp::Alpha, 1.0, -100.0, 0)] },
    MotionPreset { name: "spin", description: "One full turn", keys: &[(MotionProp::Rotation, 0.0, 0.0, 0), (MotionProp::Rotation, 1.0, 360.0, 0)] },
    MotionPreset {
        name: "pulse",
        description: "Grows and shrinks back",
        keys: &[
            (MotionProp::ScaleX, 0.0, 1.0, 0),
            (MotionProp::ScaleX, 0.5, 1.3, 0),
            (MotionProp::ScaleX, 1.0, 1.0, 0),
            (MotionProp::ScaleY, 0.0, 1.0, 0),
            (MotionProp::ScaleY, 0.5, 1.3, 0),
            (MotionProp::ScaleY, 1.0, 1.0, 0),
        ],
    },
    MotionPreset {
        name: "bounce",
        description: "Hops twice",
        keys: &[
            (MotionProp::Y, 0.0, 0.0, 0),
            (MotionProp::Y, 0.25, -80.0, 0),
            (MotionProp::Y, 0.5, 0.0, 0),
            (MotionProp::Y, 0.75, -30.0, 0),
            (MotionProp::Y, 1.0, 0.0, 0),
        ],
    },
    MotionPreset {
        name: "wiggle",
        description: "Rocks left and right",
        keys: &[
            (MotionProp::Rotation, 0.0, 0.0, 0),
            (MotionProp::Rotation, 0.25, -12.0, 0),
            (MotionProp::Rotation, 0.5, 12.0, 0),
            (MotionProp::Rotation, 0.75, -12.0, 0),
            (MotionProp::Rotation, 1.0, 0.0, 0),
        ],
    },
    MotionPreset {
        name: "zoom-in",
        description: "Grows from 20 percent",
        keys: &[(MotionProp::ScaleX, 0.0, 0.2, 0), (MotionProp::ScaleX, 1.0, 1.0, 40), (MotionProp::ScaleY, 0.0, 0.2, 0), (MotionProp::ScaleY, 1.0, 1.0, 40)],
    },
];

/// Changes one property of the object itself: added for position, rotation and alpha, multiplied for the scales.
fn offset_object(element: &mut Element, prop: MotionProp, change: f64) {
    let Some(matrix) = element.matrix() else { return };
    let [xx, xy, yx, yy, tx, ty] = matrix.as_coeffs();
    let moved = match prop {
        MotionProp::X => Affine::new([xx, xy, yx, yy, tx + change, ty]),
        MotionProp::Y => Affine::new([xx, xy, yx, yy, tx, ty + change]),
        MotionProp::Rotation => {
            let [ra, rb, rc, rd, _, _] = (Affine::rotate(change.to_radians()) * Affine::new([xx, xy, yx, yy, 0.0, 0.0])).as_coeffs();
            Affine::new([ra, rb, rc, rd, tx, ty])
        }
        MotionProp::ScaleX => Affine::new([xx * change, xy * change, yx, yy, tx, ty]),
        MotionProp::ScaleY => Affine::new([xx, xy, yx * change, yy * change, tx, ty]),
        MotionProp::Alpha => {
            if let Some(i) = element.as_symbol_instance_mut() {
                let alpha = (i.color.mul[3] + anim_core::num::f64_to_f32(change / 100.0)).clamp(0.0, 1.0);
                i.color_effect = crate::element::ColorEffect::Alpha(alpha);
                i.color = anim_core::ColorTransform::alpha(alpha);
            }
            return;
        }
    };
    element.set_matrix(moved);
}

/// Applies the preset called `name` to the motion tween covering `frame`, replacing all its tracks.
///
/// # Errors
/// Not a motion tween, or no such preset.
pub fn apply_motion_preset(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, name: &str) -> Result<(), SceneError> {
    let preset = MOTION_PRESETS.iter().find(|p| p.name == name).ok_or(SceneError::InvalidRange("no such motion preset"))?;
    let k = motion_frame(doc, id, layer, frame)?;
    let original = |frame: &Frame, prop| base_value(frame, prop);
    // Changes at the very start belong to the object itself: apply them to its keyframe.
    for &(prop, t, change, _) in preset.keys {
        if t > 0.0 {
            continue;
        }
        let Some(element) = k.elements.first_mut() else { continue };
        offset_object(element, prop, change);
    }
    let mut object = MotionObject::default();
    let wants_alpha = preset.keys.iter().any(|k| k.0 == MotionProp::Alpha);
    for prop in MotionProp::ALL {
        if prop == MotionProp::Alpha && !wants_alpha {
            continue;
        }
        object.tracks.push(MotionTrack { prop, keys: vec![MotionKey { t: 0.0, value: original(k, prop), ease: 0, curve: None }] });
    }
    for &(prop, t, change, ease) in preset.keys {
        if t <= 0.0 {
            if let Some(first) = object.tracks.iter_mut().find(|tr| tr.prop == prop).and_then(|tr| tr.keys.first_mut()) {
                first.ease = ease;
            }
            continue;
        }
        // The key is where the object ends up; the start offset above was relative to the same original value.
        let start_change = preset.keys.iter().find(|q| q.0 == prop && q.1 <= 0.0).map_or(0.0, |q| q.2);
        let neutral = if matches!(prop, MotionProp::ScaleX | MotionProp::ScaleY) { 1.0 } else { 0.0 };
        let now = original(k, prop);
        let value = match prop {
            // The object already carries the start change; undo it to reach `orig + change`.
            MotionProp::ScaleX | MotionProp::ScaleY => now / if (start_change - neutral).abs() < 1e-12 { 1.0 } else { start_change } * change,
            _ => now - start_change + change,
        };
        let base = original(k, prop);
        object.set_key(prop, base, MotionKey { t, value, ease, curve: None });
    }
    k.motion_object_xml = Some(object.to_xml());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::timeline::Frame;

    fn doc_with_tween(len: u32) -> Document {
        let mut doc = Document::default();
        let inst = crate::element::SymbolInstance::new("S", Affine::translate((100.0, 50.0)));
        let mut k = Frame::new(0, len, vec![Element::SymbolInstance(Box::new(inst))]);
        k.tween_type = TweenType::Classic;
        if let Some(l) = doc.scenes.first_mut().and_then(|s| s.layers.first_mut()) {
            l.frames = vec![k];
        }
        doc
    }

    fn frame(doc: &Document) -> &Frame {
        &doc.scenes[0].layers[0].frames[0]
    }

    #[test]
    fn xml_round_trips() {
        let object = MotionObject {
            tracks: vec![MotionTrack {
                prop: MotionProp::X,
                keys: vec![
                    MotionKey { t: 0.0, value: 10.0, ease: 0, curve: None },
                    MotionKey { t: 0.5, value: 20.5, ease: -40, curve: Some(vec![[0.0, 0.0], [0.25, 0.5], [0.75, 0.5], [1.0, 1.0]]) },
                ],
            }],
        };
        assert_eq!(MotionObject::parse(&object.to_xml()), Some(object));
    }

    #[test]
    fn create_set_remove() {
        let mut doc = doc_with_tween(11);
        let id = TimelineId::Scene(0);
        create_motion_tween(&mut doc, &id, 0, 3).unwrap();
        let obj = motion_object(frame(&doc)).unwrap();
        assert_eq!(obj.track(MotionProp::X).unwrap().keys[0].value, 100.0);
        set_motion_key(&mut doc, &id, 0, 0, MotionProp::X, 10, 400.0).unwrap();
        set_motion_key(&mut doc, &id, 0, 0, MotionProp::Alpha, 5, 40.0).unwrap();
        let obj = motion_object(frame(&doc)).unwrap();
        assert_eq!(obj.track(MotionProp::X).unwrap().keys.len(), 2);
        assert_eq!(obj.track(MotionProp::Alpha).unwrap().keys.len(), 2);
        assert!(set_motion_key(&mut doc, &id, 0, 0, MotionProp::X, 0, 1.0).is_err());
        assert!(set_motion_key(&mut doc, &id, 0, 0, MotionProp::X, 11, 1.0).is_err());
        set_motion_key_ease(&mut doc, &id, 0, 0, MotionProp::X, 10, KeyEase { acceleration: 50, curve: None }).unwrap();
        remove_motion_key(&mut doc, &id, 0, 0, MotionProp::X, 10).unwrap();
        assert_eq!(motion_object(frame(&doc)).unwrap().track(MotionProp::X).unwrap().keys.len(), 1);
        assert!(remove_motion_key(&mut doc, &id, 0, 0, MotionProp::X, 10).is_err());
    }

    #[test]
    fn short_span_and_empty_are_refused() {
        let mut doc = doc_with_tween(1);
        assert!(create_motion_tween(&mut doc, &TimelineId::Scene(0), 0, 0).is_err());
        let mut doc = doc_with_tween(5);
        doc.scenes[0].layers[0].frames[0].elements.clear();
        assert!(create_motion_tween(&mut doc, &TimelineId::Scene(0), 0, 0).is_err());
    }

    #[test]
    fn presets_apply() {
        let mut doc = doc_with_tween(24);
        let id = TimelineId::Scene(0);
        create_motion_tween(&mut doc, &id, 0, 0).unwrap();
        for p in MOTION_PRESETS {
            apply_motion_preset(&mut doc, &id, 0, 0, p.name).unwrap();
            assert!(motion_object(frame(&doc)).is_some(), "{}", p.name);
        }
        assert!(apply_motion_preset(&mut doc, &id, 0, 0, "nope").is_err());
    }
}
