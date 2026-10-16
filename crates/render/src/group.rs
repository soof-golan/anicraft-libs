//! Groups: transforms, and the offscreen path for colour transforms, blend modes, masks and filters.
//!
//! A group whose only job is to move its children (normal or pass-through blend, identity colour transform, no
//! mask, no filters) draws them straight onto the parent: source-over is associative, so no offscreen layer is
//! needed. Anything else is rendered into a transparent layer covering just the group's visible bounds, then
//! processed in a fixed order and composited:
//!
//! 1. children are drawn;
//! 2. the mask's alpha multiplies them (the mask is drawn in the group's local space);
//! 3. filters run in list order on the masked result;
//! 4. the colour transform is applied;
//! 5. the layer is composited onto the parent with the group's blend mode.

use anim_core::{Affine, BlendMode};
use anim_display::Group;
use tiny_skia::{Mask, MaskType};

use crate::composite::{self, Composite};
use crate::draw::{self, Ctx, bump};
use crate::pool::Layer;
use crate::px::PxRect;
use crate::{bounds, filters, project};

/// Draws `group` (whose parent maps to device space with `matrix`) onto `layer`.
pub fn draw(cx: &mut Ctx<'_>, layer: &mut Layer, group: &Group, matrix: Affine, depth: usize) {
    if depth >= bounds::MAX_DEPTH {
        bump(&mut cx.stats.invalid);
        return;
    }
    if let Some(projection) = &group.projection {
        project::draw(cx, layer, group, projection, matrix, depth);
        return;
    }
    let inner = matrix * group.transform;
    let next = depth.saturating_add(1);
    let isolate = composite::needs_isolation(group.blend) || !group.color.is_identity() || group.mask.is_some() || !group.filters.is_empty();
    if !isolate {
        bump(&mut cx.stats.direct_groups);
        draw::nodes(cx, layer, &group.children, inner, next);
        return;
    }
    let Some(wanted) = offscreen_rect(cx, layer, group, inner, next) else {
        if bounds::too_deep(&group.children, next) || group.mask.as_ref().is_some_and(|mask| bounds::too_deep(mask, next)) {
            bump(&mut cx.stats.invalid);
        }
        // Nothing of the group is visible. Flash's alpha blend still wipes the parent where the source is empty.
        if group.blend == BlendMode::Alpha {
            layer.pm.data_mut().fill(0);
        }
        return;
    };
    let Some(mut offscreen) = cx.pool.take(wanted) else {
        bump(&mut cx.stats.degraded_groups);
        draw::nodes(cx, layer, &group.children, inner, next);
        return;
    };
    bump(&mut cx.stats.isolated_groups);
    finish_offscreen(cx, &mut offscreen, group, inner, wanted, next);
    composite::composite(layer, &offscreen, Composite::of(group.blend));
    cx.pool.recycle(offscreen);
}

/// Steps 1 to 4 on a layer that is already allocated: children, mask, filters and colour transform, all under
/// `inner` (the matrix from the group's local space to the layer's device space).
pub fn finish_offscreen(cx: &mut Ctx<'_>, offscreen: &mut Layer, group: &Group, inner: Affine, wanted: PxRect, next: usize) {
    draw::nodes(cx, offscreen, &group.children, inner, next);
    if let Some(mask_nodes) = &group.mask {
        if let Some(mut mask_layer) = cx.pool.take(wanted) {
            draw::nodes(cx, &mut mask_layer, mask_nodes, inner, next);
            let mask = Mask::from_pixmap(mask_layer.pm.as_ref(), MaskType::Alpha);
            offscreen.pm.apply_mask(&mask);
            cx.pool.recycle(mask_layer);
        } else {
            bump(&mut cx.stats.degraded_groups);
        }
    }
    for filter in &group.filters {
        if !filters::apply(offscreen, filter, inner) {
            bump(&mut cx.stats.degraded_groups);
        }
    }
    composite::apply_color_transform(offscreen, &group.color);
}

/// The device rectangle the group's offscreen layer must cover: its content (clipped by the mask) grown by what
/// the filters add, limited to what can still reach the parent layer.
fn offscreen_rect(cx: &Ctx<'_>, layer: &Layer, group: &Group, inner: Affine, depth: usize) -> Option<PxRect> {
    let content = bounds::nodes(&group.children, inner, cx.images, depth)?;
    let clipped = match &group.mask {
        Some(mask) => bounds::intersect(content, bounds::nodes(mask, inner, cx.images, depth)?)?,
        None => content,
    };
    let pad = filters::total_reach(&group.filters, inner);
    PxRect::enclosing(clipped)?.inflate(pad).intersect(&layer.rect.inflate(pad))
}
