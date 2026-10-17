//! Classic (motion) tweens: pairing of elements between two keyframes and interpolation of their properties.

use anim_core::{Affine, ColorTransform, Matrix3D};
use anim_display::Filter;
use anim_scene::{EaseTarget, Element, Frame, InstanceFilter, SymbolInstance};

use crate::ease;
use crate::guide::{Guide, follow};
use crate::xform::{self, Progress, TweenParams};
use anim_core::num::lerp;

/// One element as it is drawn in a frame: the source element with the properties a tween may have changed.
#[derive(Debug, Clone)]
pub struct Placed<'a> {
    pub element: &'a Element,
    pub matrix: Affine,
    /// Symbol instances only: the 3D placement that replaces `matrix` when the instance is drawn.
    pub matrix_3d: Option<Matrix3D>,
    /// Symbol instances only.
    pub color: ColorTransform,
    /// Symbol instances only.
    pub filters: Vec<Filter>,
}

impl<'a> Placed<'a> {
    /// The element exactly as stored.
    #[must_use]
    pub fn hold(element: &'a Element) -> Self {
        let (color, filters) = element.as_symbol_instance().map_or((ColorTransform::IDENTITY, Vec::new()), |i| (i.color, mapped_filters(i)));
        let matrix_3d = element.as_symbol_instance().and_then(|i| i.matrix_3d);
        Self { element, matrix: element.matrix().unwrap_or(Affine::IDENTITY), matrix_3d, color, filters }
    }
}

/// The renderable filters of an instance (unmapped, raw filters are skipped).
#[must_use]
pub fn mapped_filters(instance: &SymbolInstance) -> Vec<Filter> {
    instance.filters.iter().filter_map(InstanceFilter::to_display).cloned().collect()
}

/// The elements of `start` at progress `t` (0 ..= 1, not yet eased) towards `end`.
///
/// Elements are paired by depth (their position in the keyframe) and library item: symbol instances and bitmaps
/// must name the same item, text, groups and shapes pair with the same kind. An element with no partner stays as in
/// `start`. Paired elements interpolate their matrix (position, rotation and scale each with their own ease), and
/// symbol instances also their colour transform and filters.
#[must_use]
pub fn classic<'a>(start: &'a Frame, end: &'a Frame, t: f64) -> Vec<Placed<'a>> {
    classic_along(start, end, t, None)
}

/// Like [`classic`], for a layer guided by a motion guide: positions follow `guide` (see [`crate::guide`]).
#[must_use]
pub fn classic_along<'a>(start: &'a Frame, end: &'a Frame, t: f64, guide: Option<&Guide>) -> Vec<Placed<'a>> {
    let params = TweenParams { rotate: start.motion_tween_rotate, times: start.motion_tween_rotate_times.unwrap_or(1.0), scale: start.motion_tween_scale };
    let [position, rotation, scale, color_p, filter_p] =
        ease::for_targets(start, [EaseTarget::Position, EaseTarget::Rotation, EaseTarget::Scale, EaseTarget::Color, EaseTarget::Filters], t);
    let progress = Progress { position, rotation, scale };
    let mut used = vec![false; end.elements.len()];
    start
        .elements
        .iter()
        .enumerate()
        .map(|(i, from)| {
            let mut placed = Placed::hold(from);
            let Some(j) = partner(i, from, end, &used) else { return placed };
            if let Some(flag) = used.get_mut(j) {
                *flag = true;
            }
            let Some(to) = end.elements.get(j) else { return placed };
            if let (Some(a), Some(b)) = (from.matrix(), to.matrix()) {
                placed.matrix = xform::interpolate(a, b, progress, &params);
                if let Some(guide) = guide {
                    placed.matrix = follow(placed.matrix, a, b, guide, progress.position, start.motion_tween_snap, start.motion_tween_orient_to_path);
                }
            }
            if let (Element::SymbolInstance(a), Element::SymbolInstance(b)) = (from, to) {
                placed.color = a.color.lerp(&b.color, color_f32(color_p));
                placed.filters = lerp_filters(&mapped_filters(a), &mapped_filters(b), filter_p);
                if a.matrix_3d.is_some() || b.matrix_3d.is_some() {
                    // 3D tweens interpolate position, rotation and scale per axis; the 2D result above is replaced.
                    let m3 = a.placement_3d().lerp(&b.placement_3d(), progress.position);
                    placed.matrix = m3.plane_affine();
                    placed.matrix_3d = Some(m3);
                }
            }
            placed
        })
        .collect()
}

const fn color_f32(p: f64) -> f32 {
    anim_core::num::f64_to_f32(p)
}

fn partner(i: usize, from: &Element, end: &Frame, used: &[bool]) -> Option<usize> {
    let free = |j: usize| !used.get(j).copied().unwrap_or(true);
    if free(i) && end.elements.get(i).is_some_and(|to| compatible(from, to)) {
        return Some(i);
    }
    match from {
        Element::SymbolInstance(_) | Element::Bitmap(_) => end.elements.iter().enumerate().find(|(j, to)| free(*j) && compatible(from, to)).map(|(j, _)| j),
        _ => None,
    }
}

fn compatible(a: &Element, b: &Element) -> bool {
    match (a, b) {
        (Element::SymbolInstance(x), Element::SymbolInstance(y)) => x.library_item_name == y.library_item_name,
        (Element::Bitmap(x), Element::Bitmap(y)) => x.library_item_name == y.library_item_name,
        (Element::Text(_), Element::Text(_)) | (Element::Group(_), Element::Group(_)) | (Element::Shape(_), Element::Shape(_)) => true,
        _ => false,
    }
}

/// Interpolates filter lists index by index. Pairs of different kinds, and lists of different length, keep the
/// start filter.
#[must_use]
pub fn lerp_filters(a: &[Filter], b: &[Filter], t: f64) -> Vec<Filter> {
    a.iter().enumerate().map(|(i, fa)| b.get(i).map_or_else(|| fa.clone(), |fb| lerp_filter(fa, fb, t))).collect()
}

fn lerp_filter(a: &Filter, b: &Filter, t: f64) -> Filter {
    match (a, b) {
        (Filter::Blur { x: ax, y: ay, quality }, Filter::Blur { x: bx, y: by, .. }) => {
            Filter::Blur { x: lerp(*ax, *bx, t), y: lerp(*ay, *by, t), quality: *quality }
        }
        (
            Filter::DropShadow { color: ac, angle_deg: aa, distance: ad, blur_x: abx, blur_y: aby, strength: as_, inner, knockout, hide_object },
            Filter::DropShadow { color: bc, angle_deg: ba, distance: bd, blur_x: bbx, blur_y: bby, strength: bs, .. },
        ) => Filter::DropShadow {
            color: ac.lerp(*bc, t),
            angle_deg: lerp(*aa, *ba, t),
            distance: lerp(*ad, *bd, t),
            blur_x: lerp(*abx, *bbx, t),
            blur_y: lerp(*aby, *bby, t),
            strength: lerp(*as_, *bs, t),
            inner: *inner,
            knockout: *knockout,
            hide_object: *hide_object,
        },
        (
            Filter::Glow { color: ac, blur_x: abx, blur_y: aby, strength: as_, inner, knockout },
            Filter::Glow { color: bc, blur_x: bbx, blur_y: bby, strength: bs, .. },
        ) => Filter::Glow {
            color: ac.lerp(*bc, t),
            blur_x: lerp(*abx, *bbx, t),
            blur_y: lerp(*aby, *bby, t),
            strength: lerp(*as_, *bs, t),
            inner: *inner,
            knockout: *knockout,
        },
        (Filter::ColorMatrix(ma), Filter::ColorMatrix(mb)) => {
            let tf = color_f32(t);
            let mut out = [0.0f32; 20];
            for ((slot, x), y) in out.iter_mut().zip(ma).zip(mb) {
                *slot = (y - x).mul_add(tf, *x);
            }
            Filter::ColorMatrix(out)
        }
        _ => a.clone(),
    }
}

#[cfg(test)]
mod tests {
    use anim_core::Rgba8;
    use anim_scene::{BitmapInstance, Element, Frame, SymbolInstance, TweenRotate};

    use super::*;

    fn inst(name: &str, m: Affine) -> Element {
        Element::SymbolInstance(Box::new(SymbolInstance::new(name, m)))
    }

    #[test]
    fn pairs_by_depth_and_item() {
        let a = Frame::new(0, 10, vec![inst("a", Affine::translate((0.0, 0.0))), inst("b", Affine::translate((0.0, 0.0)))]);
        let b = Frame::new(10, 1, vec![inst("a", Affine::translate((100.0, 0.0))), inst("b", Affine::translate((0.0, 50.0)))]);
        let out = classic(&a, &b, 0.5);
        assert_eq!(out.len(), 2);
        assert!((out[0].matrix.translation().x - 50.0).abs() < 1e-9);
        assert!((out[1].matrix.translation().y - 25.0).abs() < 1e-9);
    }

    #[test]
    fn swapped_order_still_finds_the_same_item() {
        let a = Frame::new(0, 10, vec![inst("a", Affine::IDENTITY), inst("b", Affine::IDENTITY)]);
        let b = Frame::new(10, 1, vec![inst("b", Affine::translate((0.0, 80.0))), inst("a", Affine::translate((40.0, 0.0)))]);
        let out = classic(&a, &b, 0.5);
        assert!((out[0].matrix.translation().x - 20.0).abs() < 1e-9);
        assert!((out[1].matrix.translation().y - 40.0).abs() < 1e-9);
    }

    #[test]
    fn unmatched_elements_hold() {
        let a = Frame::new(0, 10, vec![inst("a", Affine::IDENTITY), Element::Bitmap(BitmapInstance::new("img", Affine::IDENTITY))]);
        let b = Frame::new(10, 1, vec![inst("other", Affine::translate((100.0, 0.0)))]);
        let out = classic(&a, &b, 0.5);
        assert_eq!(out.len(), 2);
        assert!(out[0].matrix.translation().x.abs() < 1e-9, "different library item does not tween");
    }

    #[test]
    fn two_instances_of_one_item_pair_in_order() {
        let a = Frame::new(0, 10, vec![inst("a", Affine::IDENTITY), inst("a", Affine::translate((0.0, 10.0)))]);
        let b = Frame::new(10, 1, vec![inst("a", Affine::translate((10.0, 0.0))), inst("a", Affine::translate((0.0, 30.0)))]);
        let out = classic(&a, &b, 1.0);
        assert!((out[0].matrix.translation().x - 10.0).abs() < 1e-9);
        assert!((out[1].matrix.translation().y - 30.0).abs() < 1e-9);
    }

    #[test]
    fn colour_and_filters_interpolate() {
        let mut ia = SymbolInstance::new("a", Affine::IDENTITY);
        ia.set_color_effect(anim_scene::ColorEffect::Alpha(1.0));
        ia.filters.push(InstanceFilter::mapped(Filter::Blur { x: 0.0, y: 10.0, quality: 2 }));
        let mut ib = SymbolInstance::new("a", Affine::IDENTITY);
        ib.set_color_effect(anim_scene::ColorEffect::Alpha(0.0));
        ib.filters.push(InstanceFilter::mapped(Filter::Blur { x: 20.0, y: 10.0, quality: 3 }));
        let a = Frame::new(0, 10, vec![Element::SymbolInstance(Box::new(ia))]);
        let b = Frame::new(10, 1, vec![Element::SymbolInstance(Box::new(ib))]);
        let out = classic(&a, &b, 0.25);
        assert!((out[0].color.mul[3] - 0.75).abs() < 1e-6);
        assert_eq!(out[0].filters, vec![Filter::Blur { x: 5.0, y: 10.0, quality: 2 }]);
    }

    #[test]
    fn tween_settings_are_respected() {
        let mut a = Frame::new(0, 10, vec![inst("a", Affine::IDENTITY)]);
        a.motion_tween_rotate = TweenRotate::Clockwise;
        a.motion_tween_rotate_times = Some(1.0);
        let b = Frame::new(10, 1, vec![inst("a", Affine::rotate(90f64.to_radians()))]);
        let quarter = xform::decompose(classic(&a, &b, 0.25)[0].matrix);
        // (90 degrees + one turn) * 0.25 = 112.5 degrees.
        assert!((quarter.rotation.to_degrees() - 112.5).abs() < 1e-6);
        a.motion_tween_scale = false;
        let b = Frame::new(10, 1, vec![inst("a", Affine::scale(5.0))]);
        a.motion_tween_rotate = TweenRotate::Auto;
        assert!((classic(&a, &b, 0.5)[0].matrix.as_coeffs()[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn filter_lists_with_mismatched_kinds_hold() {
        let a = [Filter::Blur { x: 1.0, y: 1.0, quality: 1 }];
        let b = [Filter::Glow { color: Rgba8::BLACK, blur_x: 1.0, blur_y: 1.0, strength: 1.0, inner: false, knockout: false }];
        assert_eq!(lerp_filters(&a, &b, 0.5), a.to_vec());
        assert_eq!(lerp_filters(&a, &[], 0.5), a.to_vec());
    }
}
