//! Motion tweens (`tweenType="motion object"`): evaluating the property tracks of `<motionObjectXML>`.
//!
//! The tracks themselves (model, reader, writer, editing) live in [`anim_scene::motion`]. Here each track is sampled
//! at a moment of the span and the *change* relative to its first key is applied to the object, so a different
//! absolute origin cannot displace it. Segments ease with the key's classic acceleration or its custom bezier
//! chain. Tracks that cannot be read are ignored; when nothing is found the first keyframe is held.

use anim_core::Affine;
use anim_scene::motion::{MotionKey, MotionObject, MotionProp};

use crate::ease;

/// Keyframed property tracks, times normalised to 0 ..= 1 over the tween.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MotionTracks {
    object: MotionObject,
}

/// The change of every track at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionDelta {
    pub dx: f64,
    pub dy: f64,
    /// Degrees, clockwise.
    pub rotation: f64,
    /// Ratio to the start scale.
    pub scale_x: f64,
    pub scale_y: f64,
    /// Ratio to the start alpha.
    pub alpha: f64,
}

impl Default for MotionDelta {
    fn default() -> Self {
        Self { dx: 0.0, dy: 0.0, rotation: 0.0, scale_x: 1.0, scale_y: 1.0, alpha: 1.0 }
    }
}

impl MotionTracks {
    /// Reads the tracks of `xml`. `None` when it is not XML or holds no usable track.
    #[must_use]
    pub fn parse(xml: &str) -> Option<Self> {
        MotionObject::parse(xml).map(Self::from_object)
    }

    /// Evaluates `object`.
    #[must_use]
    pub const fn from_object(object: MotionObject) -> Self {
        Self { object }
    }

    /// The value of `prop` at normalised time `t` (0 ..= 1) as stored, `None` without a track for it.
    #[must_use]
    pub fn value_at(&self, prop: MotionProp, t: f64) -> Option<f64> {
        self.object.track(prop).map(|track| sample(&track.keys, t))
    }

    /// The change at normalised time `t` (0 ..= 1) relative to the first keyframe of each track.
    #[must_use]
    pub fn delta_at(&self, t: f64) -> MotionDelta {
        let mut out = MotionDelta::default();
        for track in &self.object.tracks {
            let Some(first) = track.keys.first().map(|k| k.value) else { continue };
            let now = sample(&track.keys, t);
            let ratio = |base: f64| if base.abs() > 1e-9 { now / base } else { 1.0 };
            match track.prop {
                MotionProp::X => out.dx = now - first,
                MotionProp::Y => out.dy = now - first,
                MotionProp::Rotation => out.rotation = now - first,
                MotionProp::ScaleX => out.scale_x = ratio(first),
                MotionProp::ScaleY => out.scale_y = ratio(first),
                MotionProp::Alpha => out.alpha = if first.abs() > 1e-9 { now / first } else { now / 100.0 },
            }
        }
        out
    }
}

impl MotionDelta {
    /// Applies the change to `base`: the linear part is rotated and scaled in stage axes, the translation moves.
    #[must_use]
    pub fn apply(&self, base: Affine) -> Affine {
        let [a, b, c, d, tx, ty] = base.as_coeffs();
        let linear = Affine::rotate(self.rotation.to_radians()) * Affine::scale_non_uniform(self.scale_x, self.scale_y) * Affine::new([a, b, c, d, 0.0, 0.0]);
        let [la, lb, lc, ld, _, _] = linear.as_coeffs();
        Affine::new([la, lb, lc, ld, tx + self.dx, ty + self.dy])
    }
}

/// Eased progress of the segment starting at `key`.
fn eased(key: &MotionKey, p: f64) -> f64 {
    key.curve.as_ref().map_or_else(|| ease::classic(key.ease, p), |points| ease::custom(points, p))
}

fn sample(points: &[MotionKey], t: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let mut prev: Option<&MotionKey> = None;
    for key in points {
        if t <= key.t {
            return prev.map_or(key.value, |p| {
                let span = key.t - p.t;
                if span <= f64::EPSILON { key.value } else { (key.value - p.value).mul_add(eased(p, (t - p.t) / span), p.value) }
            });
        }
        prev = Some(key);
    }
    prev.map_or(0.0, |p| p.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<AnimationCore TimeScale="24000" duration="48000">
        <PropertyContainer id="Basic_Motion">
          <Property id="Motion_X"><Keyframe timevalue="0" value="100"/><Keyframe timevalue="48000" value="300"/></Property>
          <Property id="Motion_Y"><Keyframe timevalue="0" anchor="50"/><Keyframe timevalue="24000" anchor="150"/></Property>
          <Property id="Rotation_Z"><Keyframe timevalue="0" value="0"/><Keyframe timevalue="48000" value="90"/></Property>
          <Property id="Scale_X"><Keyframe timevalue="0" value="100"/><Keyframe timevalue="48000" value="200"/></Property>
        </PropertyContainer></AnimationCore>"#;

    #[test]
    fn reads_tracks_relative_to_the_first_keyframe() {
        let tracks = MotionTracks::parse(XML).unwrap();
        let d = tracks.delta_at(0.5);
        assert!((d.dx - 100.0).abs() < 1e-9);
        assert!((d.dy - 100.0).abs() < 1e-9, "y track ends at half the duration");
        assert!((d.rotation - 45.0).abs() < 1e-9);
        assert!((d.scale_x - 1.5).abs() < 1e-9);
        assert!((d.scale_y - 1.0).abs() < 1e-9);
        let end = tracks.delta_at(1.0);
        assert!((end.dx - 200.0).abs() < 1e-9 && (end.dy - 100.0).abs() < 1e-9);
        let start = tracks.delta_at(0.0);
        assert_eq!(start, MotionDelta::default());
    }

    #[test]
    fn ease_and_alpha_apply() {
        let xml = r#"<AnimationCore duration="100"><Property id="Motion_X"><Keyframe timevalue="0" value="0" ease="-100"/><Keyframe timevalue="100" value="100"/></Property>
            <Property id="Color_Alpha"><Keyframe timevalue="0" value="100"/><Keyframe timevalue="100" value="0"/></Property></AnimationCore>"#;
        let tracks = MotionTracks::parse(xml).unwrap();
        let d = tracks.delta_at(0.5);
        assert!((d.dx - 25.0).abs() < 1e-9, "ease-in is quadratic: {}", d.dx);
        assert!((d.alpha - 0.5).abs() < 1e-9);
        assert_eq!(tracks.value_at(MotionProp::X, 1.0), Some(100.0));
        assert_eq!(tracks.value_at(MotionProp::Y, 1.0), None);
    }

    #[test]
    fn apply_moves_and_rotates() {
        let d = MotionDelta { dx: 10.0, dy: 0.0, rotation: 90.0, ..MotionDelta::default() };
        let m = d.apply(Affine::translate((5.0, 5.0)));
        let p = m * anim_core::Point::new(1.0, 0.0);
        assert!((p.x - 15.0).abs() < 1e-9 && (p.y - 6.0).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn junk_gives_none() {
        assert!(MotionTracks::parse("not xml").is_none());
        assert!(MotionTracks::parse("<a/>").is_none());
        assert!(MotionTracks::parse("<!DOCTYPE a [<!ENTITY b 'c'>]><a/>").is_none());
        assert!(MotionTracks::parse(r#"<a><Property id="Motion_X"><Keyframe timevalue="x" value="y"/></Property></a>"#).is_none());
        assert!(MotionTracks::parse(r#"<a><Property id="Motion_X"><Keyframe timevalue="0" value="1"/></Property></a>"#).is_none());
    }
}
