//! Conservative device-space bounds of display-list nodes, used to cull and to size offscreen layers.
//!
//! "Conservative" means the rectangle always contains every pixel the node can touch (anti-aliasing and
//! hairlines included); it may be larger. Nodes that draw nothing have no bounds.

use anim_core::{Affine, Point, Rect};
use anim_display::{Group, ImageStore, Mesh, Node};

use crate::{convert, filters, stroke};

/// Groups and meshes nested deeper than this are not drawn (hostile input guard; Animate documents nest ~10).
pub const MAX_DEPTH: usize = 64;

/// Whether a group or mesh in `list` sits at nesting depth [`MAX_DEPTH`] or deeper (given that `list` itself is
/// at `depth`): such nodes are not drawn and have no bounds.
pub fn too_deep(list: &[Node], depth: usize) -> bool {
    list.iter().any(|node| match node {
        Node::Group(group) => {
            depth >= MAX_DEPTH
                || too_deep(&group.children, depth.saturating_add(1))
                || group.mask.as_ref().is_some_and(|mask| too_deep(mask, depth.saturating_add(1)))
        }
        Node::Mesh(mesh) => depth >= MAX_DEPTH || too_deep(&mesh.content, depth.saturating_add(1)),
        Node::Fill(_) | Node::Stroke(_) | Node::Bitmap(_) => false,
    })
}

/// Bounds of a sibling list under `matrix`.
pub fn nodes(list: &[Node], matrix: Affine, images: &ImageStore, depth: usize) -> Option<Rect> {
    list.iter().fold(None, |acc, node| convert::union(acc, self::node(node, matrix, images, depth)))
}

/// Bounds of one node whose parent maps to device space with `matrix`.
pub fn node(node: &Node, matrix: Affine, images: &ImageStore, depth: usize) -> Option<Rect> {
    match node {
        Node::Fill(fill) => convert::path_bounds(&fill.path, matrix * fill.transform),
        Node::Stroke(outline) => {
            let full = matrix * outline.transform;
            let plan = stroke::plan(&outline.style, full)?;
            let rect = convert::path_bounds(&outline.path, full)?;
            Some(rect.inflate(plan.reach, plan.reach))
        }
        Node::Bitmap(bitmap) => {
            let image = images.get(bitmap.image)?;
            let size = Rect::new(0.0, 0.0, f64::from(image.width), f64::from(image.height));
            let rect = (matrix * bitmap.transform).transform_rect_bbox(size);
            finite(rect)
        }
        Node::Group(group) => self::group(group, matrix, images, depth),
        Node::Mesh(mesh) => self::mesh(mesh, matrix),
    }
}

/// Bounds of a group's visible result: children (clipped by the mask) grown by what its filters add.
pub fn group(group: &Group, matrix: Affine, images: &ImageStore, depth: usize) -> Option<Rect> {
    if depth >= MAX_DEPTH {
        return None;
    }
    if let Some(projection) = &group.projection {
        return projected_group(group, projection, matrix, images, depth);
    }
    let inner = matrix * group.transform;
    let mut rect = nodes(&group.children, inner, images, depth.saturating_add(1))?;
    if let Some(mask) = &group.mask {
        rect = intersect(rect, nodes(mask, inner, images, depth.saturating_add(1))?)?;
    }
    let pad = f64::from(filters::total_reach(&group.filters, inner));
    Some(rect.inflate(pad, pad))
}

/// Bounds of a group under a perspective projection: the content's rectangle in the plane, grown by the filters'
/// reach, projected with the part behind the viewer clipped away.
fn projected_group(group: &Group, projection: &anim_core::Homography, matrix: Affine, images: &ImageStore, depth: usize) -> Option<Rect> {
    let mut content = nodes(&group.children, group.transform, images, depth.saturating_add(1))?;
    if let Some(mask) = &group.mask {
        content = intersect(content, nodes(mask, group.transform, images, depth.saturating_add(1))?)?;
    }
    let pad = f64::from(filters::total_reach(&group.filters, Affine::IDENTITY));
    let content = content.inflate(pad, pad);
    let quad = [Point::new(content.x0, content.y0), Point::new(content.x1, content.y0), Point::new(content.x1, content.y1), Point::new(content.x0, content.y1)];
    crate::project::projected_bounds(&projection.before_affine(matrix), quad).and_then(finite)
}

/// Bounds of the deformed triangles of a mesh.
pub fn mesh(mesh: &Mesh, matrix: Affine) -> Option<Rect> {
    let full = matrix * mesh.transform;
    let mut rect: Option<Rect> = None;
    for corner in mesh.triangles.iter().flatten() {
        let point = mesh.target.get(usize::try_from(*corner).ok()?)?;
        let mapped: Point = full * *point;
        let single = Rect::new(mapped.x, mapped.y, mapped.x, mapped.y);
        rect = convert::union(rect, Some(single));
    }
    rect.and_then(finite)
}

fn finite(rect: Rect) -> Option<Rect> {
    (rect.x0.is_finite() && rect.y0.is_finite() && rect.x1.is_finite() && rect.y1.is_finite()).then_some(rect)
}

/// Intersection of two rectangles; `None` when they do not overlap in an area.
pub fn intersect(first: Rect, second: Rect) -> Option<Rect> {
    let rect = Rect::new(first.x0.max(second.x0), first.y0.max(second.y0), first.x1.min(second.x1), first.y1.min(second.y1));
    (rect.x0 < rect.x1 && rect.y0 < rect.y1).then_some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_core::{BezPath, Rgba8};
    use anim_display::{Fill, FillRule, Paint};

    fn rect_fill(x0: f64, y0: f64, x1: f64, y1: f64) -> Node {
        let mut path = BezPath::new();
        path.move_to((x0, y0));
        path.line_to((x1, y0));
        path.line_to((x1, y1));
        path.line_to((x0, y1));
        path.close_path();
        Node::Fill(Fill { transform: Affine::IDENTITY, path, rule: FillRule::NonZero, paint: Paint::Solid(Rgba8::BLACK) })
    }

    #[test]
    fn group_bounds_apply_transform_mask_and_filters() {
        let images = ImageStore::new();
        let group = Group {
            transform: Affine::translate((10.0, 0.0)),
            children: vec![rect_fill(0.0, 0.0, 10.0, 10.0)],
            mask: Some(vec![rect_fill(0.0, 0.0, 5.0, 100.0)]),
            ..Group::default()
        };
        let rect = self::group(&group, Affine::IDENTITY, &images, 0).unwrap();
        assert_eq!((rect.x0, rect.y0, rect.x1, rect.y1), (10.0, 0.0, 15.0, 10.0));
        let blurred = Group { filters: vec![anim_display::Filter::Blur { x: 8.0, y: 8.0, quality: 1 }], ..group };
        let padded = self::group(&blurred, Affine::IDENTITY, &images, 0).unwrap();
        assert!(padded.x0 < 10.0 && padded.x1 > 15.0);
    }

    #[test]
    fn disjoint_mask_means_no_bounds() {
        let images = ImageStore::new();
        let group = Group { children: vec![rect_fill(0.0, 0.0, 10.0, 10.0)], mask: Some(vec![rect_fill(50.0, 50.0, 60.0, 60.0)]), ..Group::default() };
        assert!(self::group(&group, Affine::IDENTITY, &images, 0).is_none());
    }

    #[test]
    fn too_deep_groups_have_no_bounds() {
        let images = ImageStore::new();
        let group = Group { children: vec![rect_fill(0.0, 0.0, 1.0, 1.0)], ..Group::default() };
        assert!(self::group(&group, Affine::IDENTITY, &images, MAX_DEPTH).is_none());
    }
}
