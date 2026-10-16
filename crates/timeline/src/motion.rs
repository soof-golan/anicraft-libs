//! Motion-object tweens (`tweenType="motion object"`): a best-effort reading of `<motionObjectXML>`.
//!
//! Animate stores property tracks as `<Property id="Motion_X">` (also `Motion_Y`, `Rotation_Z`, `Scale_X`,
//! `Scale_Y`) holding `<Keyframe timevalue=".." value="..">` children, under an `<AnimationCore duration="..">`
//! root. The reader accepts those names (and close variants) and applies the *change* of each track relative to its
//! first keyframe, so a different absolute unit or origin cannot displace the object. Tracks it cannot read are
//! ignored; when nothing is found the first keyframe is held.

use anim_core::Affine;

/// Most `Property` and `Keyframe` elements read from one document.
const MAX_NODES: u32 = 50_000;

/// What a track changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    X,
    Y,
    Rotation,
    ScaleX,
    ScaleY,
}

/// Keyframed property tracks, times normalised to 0 ..= 1 over the tween.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MotionTracks {
    tracks: Vec<(Channel, Vec<(f64, f64)>)>,
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
}

impl Default for MotionDelta {
    fn default() -> Self {
        Self { dx: 0.0, dy: 0.0, rotation: 0.0, scale_x: 1.0, scale_y: 1.0 }
    }
}

impl MotionTracks {
    /// Reads the tracks of `xml`. `None` when it is not XML or holds no usable track.
    #[must_use]
    pub fn parse(xml: &str) -> Option<Self> {
        let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: MAX_NODES, ..roxmltree::ParsingOptions::default() };
        let doc = roxmltree::Document::parse_with_options(xml, options).ok()?;
        let declared = doc.descendants().find_map(|n| n.attribute("duration")).and_then(|d| d.trim().parse::<f64>().ok()).filter(|d| *d > 0.0);
        let mut raw: Vec<(Channel, Vec<(f64, f64)>)> = Vec::new();
        for prop in doc.descendants().filter(|n| n.has_tag_name("Property")) {
            let Some(channel) = prop.attribute("id").and_then(classify) else { continue };
            let mut points: Vec<(f64, f64)> = prop
                .children()
                .filter(|k| k.has_tag_name("Keyframe"))
                .filter_map(|k| {
                    let time = k.attribute("timevalue").or_else(|| k.attribute("time"))?.trim().parse::<f64>().ok()?;
                    let value = keyframe_value(k)?;
                    (time.is_finite() && value.is_finite()).then_some((time, value))
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
        let tracks = raw.into_iter().map(|(c, p)| (c, p.into_iter().map(|(t, v)| (t / total, v)).collect())).collect();
        Some(Self { tracks })
    }

    /// The change at normalised time `t` (0 ..= 1) relative to the first keyframe of each track.
    #[must_use]
    pub fn delta_at(&self, t: f64) -> MotionDelta {
        let mut out = MotionDelta::default();
        for (channel, points) in &self.tracks {
            let Some(first) = points.first().map(|p| p.1) else { continue };
            let now = sample(points, t);
            match channel {
                Channel::X => out.dx = now - first,
                Channel::Y => out.dy = now - first,
                Channel::Rotation => out.rotation = now - first,
                Channel::ScaleX => out.scale_x = if first.abs() > 1e-9 { now / first } else { 1.0 },
                Channel::ScaleY => out.scale_y = if first.abs() > 1e-9 { now / first } else { 1.0 },
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

fn classify(id: &str) -> Option<Channel> {
    let id = id.to_ascii_lowercase();
    let axis = id.rsplit(['_', ' ', '.']).next().unwrap_or("");
    if id.contains("rotat") {
        return Some(Channel::Rotation);
    }
    if id.contains("scale") {
        return match axis {
            "x" => Some(Channel::ScaleX),
            "y" => Some(Channel::ScaleY),
            _ => None,
        };
    }
    if id.contains("skew") {
        return None;
    }
    if id.starts_with("motion") || id.contains("position") || id.contains("translat") {
        return match axis {
            "x" => Some(Channel::X),
            "y" => Some(Channel::Y),
            _ => None,
        };
    }
    None
}

fn keyframe_value(k: roxmltree::Node<'_, '_>) -> Option<f64> {
    let from_attr = |name: &str| k.attribute(name).and_then(|v| v.split(',').next()).and_then(|v| v.trim().parse::<f64>().ok());
    from_attr("value")
        .or_else(|| from_attr("anchor"))
        .or_else(|| k.children().find(|c| c.has_tag_name("Value")).and_then(|c| c.text()).and_then(|t| t.trim().parse::<f64>().ok()))
}

fn sample(points: &[(f64, f64)], t: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let mut prev: Option<(f64, f64)> = None;
    for &(time, value) in points {
        if t <= time {
            return prev.map_or(value, |(pt, pv)| {
                let span = time - pt;
                if span <= f64::EPSILON { value } else { (value - pv).mul_add((t - pt) / span, pv) }
            });
        }
        prev = Some((time, value));
    }
    prev.map_or(0.0, |p| p.1)
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
    fn apply_moves_and_rotates() {
        let d = MotionDelta { dx: 10.0, dy: 0.0, rotation: 90.0, scale_x: 1.0, scale_y: 1.0 };
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
