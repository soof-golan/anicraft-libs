//! Shape hints (Modify > Shape > Add Shape Hint) and custom ease curves of classic tweens: editing functions.

use anim_core::Point;

use crate::document::{Document, TimelineId};
use crate::edit::keyframe_mut;
use crate::element::Element;
use crate::error::SceneError;
use crate::timeline::{EaseCurve, EaseTarget, ShapeHint, TweenEase, TweenType};

/// Most hints a shape tween may carry (the letters a to z).
pub const MAX_HINTS: u32 = 26;

fn shape_tween_frame<'a>(doc: &'a mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<&'a mut crate::timeline::Frame, SceneError> {
    let k = keyframe_mut(doc, id, layer, frame)?;
    if k.tween_type == TweenType::Shape { Ok(k) } else { Err(SceneError::NotTweenable("the keyframe does not start a shape tween")) }
}

/// Add Shape Hint: adds the next free hint letter to the shape tween covering `frame`, at the centre of the start
/// shape and of the end shape. Returns the hint id (0 = `a`).
///
/// # Errors
/// Not a shape tween, no end keyframe, or all 26 letters used.
pub fn add_shape_hint(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32) -> Result<u32, SceneError> {
    let (start_centre, end_centre) = {
        let timeline = doc.timeline(id).ok_or_else(|| SceneError::NoSuchTimeline(id.clone()))?;
        let l = timeline.layers.get(layer).ok_or(SceneError::NoSuchLayer { layer, count: timeline.layers.len() })?;
        let k = l.keyframe_at(frame).ok_or(SceneError::NoFrame { layer, frame })?;
        if k.tween_type != TweenType::Shape {
            return Err(SceneError::NotTweenable("the keyframe does not start a shape tween"));
        }
        let end = l.next_keyframe_after(k.index).ok_or(SceneError::NoEndKeyframe { layer, frame: k.index })?;
        let centre = |elements: &[Element]| crate::bounds::elements_bounds(elements.iter().filter(|e| matches!(e, Element::Shape(_))), &doc.library).map(|r| r.center());
        (centre(&k.elements), centre(&end.elements))
    };
    let k = shape_tween_frame(doc, id, layer, frame)?;
    let free = (0..MAX_HINTS).find(|n| !k.shape_hints.iter().any(|h| h.id == *n)).ok_or(SceneError::LimitExceeded("shape hints"))?;
    // Stagger the hints so they do not sit on top of each other.
    let nudge = f64::from(free) * 12.0;
    let place = |c: Option<Point>| c.map_or_else(|| Point::new(nudge, nudge), |c| Point::new(c.x + nudge, c.y + nudge));
    k.shape_hints.push(ShapeHint { id: free, start: place(start_centre), end: place(end_centre) });
    k.shape_hints.sort_by_key(|h| h.id);
    k.morph_shape_xml = None;
    Ok(free)
}

/// Moves the start (`end == false`) or end point of hint `hint` to `to`.
///
/// # Errors
/// Not a shape tween, no such hint, or a non-finite point.
pub fn move_shape_hint(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, hint: u32, end: bool, to: Point) -> Result<(), SceneError> {
    if !to.x.is_finite() || !to.y.is_finite() {
        return Err(SceneError::InvalidRange("the hint position is not a number"));
    }
    let k = shape_tween_frame(doc, id, layer, frame)?;
    let h = k.shape_hints.iter_mut().find(|h| h.id == hint).ok_or(SceneError::InvalidRange("no such shape hint"))?;
    if end {
        h.end = to;
    } else {
        h.start = to;
    }
    k.morph_shape_xml = None;
    Ok(())
}

/// Removes hint `hint`, or every hint when `hint` is `None` (Remove All Hints).
///
/// # Errors
/// Not a shape tween, or no such hint.
pub fn remove_shape_hint(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, hint: Option<u32>) -> Result<(), SceneError> {
    let k = shape_tween_frame(doc, id, layer, frame)?;
    match hint {
        None => k.shape_hints.clear(),
        Some(n) => {
            let before = k.shape_hints.len();
            k.shape_hints.retain(|h| h.id != n);
            if k.shape_hints.len() == before {
                return Err(SceneError::InvalidRange("no such shape hint"));
            }
        }
    }
    k.morph_shape_xml = None;
    Ok(())
}

/// Sets the custom ease of a classic (or shape) tween for `target`: a cubic bezier chain of `1 + 3k` points
/// `[time, value]` from `(0,0)` to `(1,1)`; `None` removes the custom entry again.
///
/// # Errors
/// No keyframe at `frame`, or a curve that is not finite, not `1 + 3k` points, or not running from (0,0) to (1,1).
pub fn set_custom_ease(doc: &mut Document, id: &TimelineId, layer: usize, frame: u32, target: EaseTarget, curve: Option<Vec<[f64; 2]>>) -> Result<(), SceneError> {
    if let Some(points) = &curve {
        let n = points.len();
        let finite = points.iter().all(|p| p[0].is_finite() && p[1].is_finite());
        let ends = points.first().is_some_and(|p| p[0].abs() < 1e-9 && p[1].abs() < 1e-9) && points.last().is_some_and(|p| (p[0] - 1.0).abs() < 1e-9 && (p[1] - 1.0).abs() < 1e-9);
        if !(4..=301).contains(&n) || n.saturating_sub(1) % 3 != 0 || !finite || !ends {
            return Err(SceneError::InvalidRange("a custom ease is 1 + 3k finite points from (0,0) to (1,1)"));
        }
    }
    let k = keyframe_mut(doc, id, layer, frame)?;
    k.eases.retain(|e| e.target != target);
    if let Some(points) = curve {
        k.eases.push(TweenEase { target, curve: EaseCurve::Custom(points) });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tweened() -> Document {
        let mut doc = Document::default();
        let mut a = crate::timeline::Frame::new(0, 5, vec![]);
        a.tween_type = TweenType::Shape;
        let b = crate::timeline::Frame::new(5, 1, vec![]);
        if let Some(l) = doc.scenes.first_mut().and_then(|s| s.layers.first_mut()) {
            l.frames = vec![a, b];
        }
        doc
    }

    #[test]
    fn hints_add_move_remove() {
        let mut doc = tweened();
        let id = TimelineId::Scene(0);
        assert_eq!(add_shape_hint(&mut doc, &id, 0, 2).unwrap(), 0);
        assert_eq!(add_shape_hint(&mut doc, &id, 0, 0).unwrap(), 1);
        move_shape_hint(&mut doc, &id, 0, 0, 0, true, Point::new(5.0, 6.0)).unwrap();
        let hints = &doc.scenes[0].layers[0].frames[0].shape_hints;
        assert_eq!(hints[0].end, Point::new(5.0, 6.0));
        remove_shape_hint(&mut doc, &id, 0, 0, Some(0)).unwrap();
        assert_eq!(add_shape_hint(&mut doc, &id, 0, 0).unwrap(), 0, "the freed letter is reused");
        remove_shape_hint(&mut doc, &id, 0, 0, None).unwrap();
        assert!(doc.scenes[0].layers[0].frames[0].shape_hints.is_empty());
        assert!(remove_shape_hint(&mut doc, &id, 0, 0, Some(3)).is_err());
        assert!(add_shape_hint(&mut doc, &id, 0, 5).is_err(), "the end keyframe starts no tween");
    }

    #[test]
    fn custom_ease_validates() {
        let mut doc = tweened();
        let id = TimelineId::Scene(0);
        let good = vec![[0.0, 0.0], [0.3, 0.0], [0.7, 1.0], [1.0, 1.0]];
        set_custom_ease(&mut doc, &id, 0, 0, EaseTarget::All, Some(good.clone())).unwrap();
        assert_eq!(doc.scenes[0].layers[0].frames[0].eases.len(), 1);
        set_custom_ease(&mut doc, &id, 0, 0, EaseTarget::All, Some(good)).unwrap();
        assert_eq!(doc.scenes[0].layers[0].frames[0].eases.len(), 1, "replaced, not duplicated");
        assert!(set_custom_ease(&mut doc, &id, 0, 0, EaseTarget::All, Some(vec![[0.0, 0.0], [1.0, 1.0]])).is_err());
        set_custom_ease(&mut doc, &id, 0, 0, EaseTarget::All, None).unwrap();
        assert!(doc.scenes[0].layers[0].frames[0].eases.is_empty());
    }
}
