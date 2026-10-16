//! Armature layers: the pose at a frame, tweened between pose keyframes, and the artwork moved by it.
//!
//! The artwork is the first keyframe's elements. A symbol armature moves each element by the motion of the bone that
//! names it; a shape armature deforms every point of the first keyframe's shapes with the bones (see
//! [`Armature::skin_point`]). A pose keyframe with a tween interpolates towards the next pose keyframe, eased like a
//! classic tween.

use anim_core::{Affine, Point};
use anim_scene::{Armature, ArmatureKind, EaseTarget, Element, Layer, Pose, TweenType};
use anim_shape::{EdgeShape, Segment};

use crate::ease;

/// Twips per pixel of [`EdgeShape`] coordinates.
const TWIPS: f64 = 20.0;

/// The pose a layer is in at `frame`, or `None` when it has no armature. Before the first pose keyframe the
/// armature is at rest.
#[must_use]
pub fn pose_at(layer: &Layer, frame: u32) -> Option<Pose> {
    let armature = layer.armature.as_ref()?;
    let rest = armature.rest_pose();
    let current = layer.frames.iter().rfind(|f| f.index <= frame && f.pose.is_some());
    let Some(current) = current else { return Some(rest) };
    let Some(from) = &current.pose else { return Some(rest) };
    if current.tween_type == TweenType::None || frame <= current.index {
        return Some(from.clone());
    }
    let next = layer.frames.iter().find(|f| f.index > current.index && f.pose.is_some());
    let Some((next, to)) = next.and_then(|n| n.pose.as_ref().map(|p| (n, p))) else { return Some(from.clone()) };
    let span = next.index.saturating_sub(current.index);
    if span == 0 || frame >= next.index {
        return Some(if frame >= next.index { to.clone() } else { from.clone() });
    }
    let t = f64::from(frame.saturating_sub(current.index)) / f64::from(span);
    let eased = ease::for_target(current, EaseTarget::All, t);
    Some(armature.clamped(&from.lerp(to, eased)))
}

/// One thing an armature layer draws: a stored element with a replaced matrix, or a deformed shape.
#[derive(Debug, Clone)]
pub enum Posed<'a> {
    /// A stored element drawn with `matrix`.
    Element { element: &'a Element, matrix: Affine },
    /// A shape deformed by the bones, in stage coordinates.
    Shape(EdgeShape),
}

/// The artwork of an armature layer at `frame`, one entry per element of the first keyframe (so entry `i` is
/// element `i`). Empty when the layer has no armature or no keyframe.
#[must_use]
pub fn posed_elements(layer: &Layer, frame: u32) -> Vec<Posed<'_>> {
    let (Some(armature), Some(rest), Some(pose)) = (layer.armature.as_ref(), layer.rest_keyframe(), pose_at(layer, frame)) else { return Vec::new() };
    let motions = armature.motions(&pose);
    rest.elements.iter().enumerate().map(|(i, element)| posed_element(armature, &motions, i, element)).collect()
}

fn posed_element<'a>(armature: &Armature, motions: &[Affine], index: usize, element: &'a Element) -> Posed<'a> {
    let own = element.matrix().unwrap_or(Affine::IDENTITY);
    if let (ArmatureKind::Shape, Element::Shape(s)) = (armature.kind, element) {
        let mut shape = s.shape.clone();
        shape.transform(own);
        deform(&mut shape, armature, motions);
        return Posed::Shape(shape);
    }
    let motion = armature.bones.iter().position(|b| b.element == Some(index)).and_then(|b| motions.get(b).copied()).unwrap_or(Affine::IDENTITY);
    Posed::Element { element, matrix: motion * own }
}

/// Moves every point of `shape` (stage pixels) by the skinning of `armature`.
fn deform(shape: &mut EdgeShape, armature: &Armature, motions: &[Affine]) {
    let map = |p: Point| {
        let px = Point::new(p.x / TWIPS, p.y / TWIPS);
        let q = armature.skin_point(px, motions);
        Point::new(q.x * TWIPS, q.y * TWIPS)
    };
    for edge in &mut shape.edges {
        for segment in &mut edge.segments {
            *segment = Segment::map_points(*segment, map);
        }
        edge.forget_original();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_core::Rgba8;
    use anim_scene::{ArmatureBone, Frame, SymbolInstance};

    fn layer() -> Layer {
        let mut l = Layer::new("rig", Rgba8::BLACK);
        let mut a = Armature::new("a", ArmatureKind::Symbol);
        let mut upper = ArmatureBone::between("upper", None, Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        upper.element = Some(0);
        a.push(upper);
        let mut lower = ArmatureBone::between("lower", Some(0), Point::new(100.0, 0.0), Point::new(200.0, 0.0));
        lower.element = Some(1);
        a.push(lower);
        l.armature = Some(a);
        let rest = Frame::new(
            0,
            10,
            vec![
                Element::SymbolInstance(Box::new(SymbolInstance::new("up", Affine::translate((50.0, 0.0))))),
                Element::SymbolInstance(Box::new(SymbolInstance::new("lo", Affine::translate((150.0, 0.0))))),
            ],
        );
        l.frames.push(rest);
        l
    }

    fn translation(m: Affine) -> Point {
        Point::new(m.as_coeffs()[4], m.as_coeffs()[5])
    }

    #[test]
    fn rest_pose_leaves_elements_alone() {
        let l = layer();
        let items = posed_elements(&l, 3);
        let Posed::Element { matrix, .. } = &items[1] else { panic!("element") };
        assert!(translation(*matrix).distance(Point::new(150.0, 0.0)) < 1.0e-9);
    }

    #[test]
    fn a_pose_moves_the_bound_elements() {
        let mut l = layer();
        let mut pose = Pose::rest(2);
        pose.set_angle(0, std::f64::consts::FRAC_PI_2);
        let mut key = Frame::new(5, 5, Vec::new());
        key.pose = Some(pose);
        l.frames.push(key);
        let before = posed_elements(&l, 4);
        let Posed::Element { matrix, .. } = &before[1] else { panic!("element") };
        assert!(translation(*matrix).distance(Point::new(150.0, 0.0)) < 1.0e-9);
        let after = posed_elements(&l, 5);
        let Posed::Element { matrix, .. } = &after[1] else { panic!("element") };
        // The lower element sat 150 px along the x axis; the root bone turned a quarter turn about the origin.
        assert!(translation(*matrix).distance(Point::new(0.0, 150.0)) < 1.0e-9, "{:?}", translation(*matrix));
    }

    #[test]
    fn poses_tween_with_the_ease_of_the_first_keyframe() {
        let mut l = layer();
        let mut a = Pose::rest(2);
        a.set_angle(0, 0.0);
        let mut b = Pose::rest(2);
        b.set_angle(0, 1.0);
        let mut k2 = Frame::new(10, 5, Vec::new());
        k2.pose = Some(b);
        l.frames[0].duration = 10;
        l.frames[0].pose = Some(a);
        l.frames[0].tween_type = TweenType::Classic;
        l.frames.push(k2);
        let mid = pose_at(&l, 5).expect("armature");
        assert!((mid.angle(0) - 0.5).abs() < 1.0e-9, "{}", mid.angle(0));
        assert!((pose_at(&l, 10).expect("armature").angle(0) - 1.0).abs() < 1.0e-9);
        l.frames[0].acceleration = 100;
        assert!(pose_at(&l, 5).expect("armature").angle(0) > 0.7);
    }

    #[test]
    fn a_tween_respects_the_limits() {
        let mut l = layer();
        l.armature.as_mut().expect("armature").bones[0].limits = Some((-0.25, 0.25));
        let mut b = Pose::rest(2);
        b.set_angle(0, 1.0);
        l.frames[0].pose = Some(Pose::rest(2));
        l.frames[0].tween_type = TweenType::Classic;
        let mut k2 = Frame::new(10, 5, Vec::new());
        k2.pose = Some(b);
        l.frames.push(k2);
        assert!((pose_at(&l, 9).expect("armature").angle(0) - 0.25).abs() < 1.0e-9);
    }

    #[test]
    fn no_armature_no_pose() {
        let l = Layer::new("plain", Rgba8::BLACK);
        assert!(pose_at(&l, 0).is_none());
        assert!(posed_elements(&l, 0).is_empty());
    }
}
