//! Hit testing: which nodes of a display list cover a point, without rasterising.
//!
//! The answer follows the geometry the renderer draws: fills by their rule, strokes by their (scaled) width, bitmaps
//! and bitmap fills by pixel alpha, groups by their mask, meshes by mapping the point back into the content. Blend
//! modes, filters and colour transforms are ignored (a drop shadow is not pickable), except that a group made fully
//! transparent by its colour transform cannot be hit.

use anim_core::kurbo::{ParamCurveNearest, Shape};
use anim_core::{Affine, BezPath, Point};
use anim_display::{Bitmap, DisplayList, Fill, FillRule, Group, ImageId, ImageStore, Mesh, Node, Paint, Stroke, StrokeScale};

use crate::bounds::MAX_DEPTH;
use crate::convert::axis_scales;

/// Most hits [`pick_all`] returns.
const MAX_HITS: usize = 1024;
/// Strokes thinner than this many device pixels are still pickable as if they were this wide.
const MIN_PICK_WIDTH: f64 = 1.0;
/// Accuracy of nearest-point queries on curves, in device pixels.
const NEAREST_ACCURACY: f64 = 0.05;

/// One node under the point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Indices into the nested node lists leading to the node: `[3, 0, 2]` is node 2 of the children (or the mesh
    /// content) of node 0 of the children of node 3 of the display list.
    pub path: Vec<usize>,
    /// The [`Group::key`] of every keyed group around the node, outermost first.
    pub keys: Vec<u64>,
}

/// Every drawn node (fill, stroke, bitmap) under `point`, topmost first. `point` is in output pixels and `view`
/// maps stage coordinates to output pixels, as in [`crate::RenderOptions`].
#[must_use]
pub fn pick_all(list: &DisplayList, images: &ImageStore, view: Affine, point: Point) -> Vec<Hit> {
    let mut walk = Walk { images, hits: Vec::new(), path: Vec::new(), keys: Vec::new() };
    if point.x.is_finite() && point.y.is_finite() && view.is_finite() {
        walk.nodes(&list.nodes, view, point, 0, true);
    }
    walk.hits
}

/// The topmost drawn node under `point`, if any.
#[must_use]
pub fn pick(list: &DisplayList, images: &ImageStore, view: Affine, point: Point) -> Option<Hit> {
    let mut walk = Walk { images, hits: Vec::new(), path: Vec::new(), keys: Vec::new() };
    if point.x.is_finite() && point.y.is_finite() && view.is_finite() {
        walk.nodes(&list.nodes, view, point, 0, false);
    }
    walk.hits.into_iter().next()
}

struct Walk<'a> {
    images: &'a ImageStore,
    hits: Vec<Hit>,
    path: Vec<usize>,
    keys: Vec<u64>,
}

impl Walk<'_> {
    /// Tests `list` topmost first. Returns true when the walk should stop (a hit was found and `all` is off, or the
    /// hit limit was reached).
    fn nodes(&mut self, list: &[Node], matrix: Affine, point: Point, depth: usize, all: bool) -> bool {
        for (index, node) in list.iter().enumerate().rev() {
            self.path.push(index);
            let stop = self.node(node, matrix, point, depth, all);
            self.path.pop();
            if stop {
                return true;
            }
        }
        false
    }

    fn record(&mut self, all: bool) -> bool {
        self.hits.push(Hit { path: self.path.clone(), keys: self.keys.clone() });
        !all || self.hits.len() >= MAX_HITS
    }

    fn node(&mut self, node: &Node, matrix: Affine, point: Point, depth: usize, all: bool) -> bool {
        match node {
            Node::Fill(fill) => hits_fill(fill, matrix, point, self.images) && self.record(all),
            Node::Stroke(stroke) => hits_stroke(stroke, matrix, point) && self.record(all),
            Node::Bitmap(bitmap) => hits_bitmap(bitmap, matrix, point, self.images) && self.record(all),
            Node::Group(group) => self.group(group, matrix, point, depth, all),
            Node::Mesh(mesh) => self.mesh(mesh, matrix, point, depth, all),
        }
    }

    fn group(&mut self, group: &Group, matrix: Affine, point: Point, depth: usize, all: bool) -> bool {
        if depth >= MAX_DEPTH || invisible(group) {
            return false;
        }
        // Under a projection the point is carried back into the group's plane and the children are tested there.
        let (inner, point) = match &group.projection {
            Some(projection) => {
                let full = projection.before_affine(matrix);
                let Some(plane_point) = full.inverse().and_then(|inverse| inverse.apply_unchecked(point)) else { return false };
                if full.weight(plane_point) <= anim_core::projective::MIN_W {
                    return false;
                }
                (group.transform, plane_point)
            }
            None => (matrix * group.transform, point),
        };
        if let Some(mask) = &group.mask {
            let mut probe = Walk { images: self.images, hits: Vec::new(), path: Vec::new(), keys: Vec::new() };
            probe.nodes(mask, inner, point, depth.saturating_add(1), false);
            if probe.hits.is_empty() {
                return false;
            }
        }
        let keyed = group.key.is_some();
        if let Some(key) = group.key {
            self.keys.push(key);
        }
        let stop = self.nodes(&group.children, inner, point, depth.saturating_add(1), all);
        if keyed {
            self.keys.pop();
        }
        stop
    }

    fn mesh(&mut self, mesh: &Mesh, matrix: Affine, point: Point, depth: usize, all: bool) -> bool {
        if depth >= MAX_DEPTH {
            return false;
        }
        let full = matrix * mesh.transform;
        let Some(source) = source_point(mesh, full, point) else { return false };
        self.nodes(&mesh.content, Affine::IDENTITY, source, depth.saturating_add(1), all)
    }
}

/// A group whose colour transform leaves nothing visible.
fn invisible(group: &Group) -> bool {
    let ct = &group.color;
    ct.mul.get(3).is_some_and(|m| *m <= 0.0) && ct.add.get(3).is_some_and(|a| *a <= 0.0)
}

/// Maps `point` (device) back to content space through the mesh triangle that contains it (the topmost one when
/// triangles overlap).
fn source_point(mesh: &Mesh, full: Affine, point: Point) -> Option<Point> {
    const TOLERANCE: f64 = 1e-9;
    for corners in mesh.triangles.iter().rev() {
        let (Some(source), Some(target)) = (corner_points(&mesh.source, *corners), corner_points(&mesh.target, *corners)) else { continue };
        let to_device = triangle_basis(target.map(|p| full * p));
        if !invertible(to_device) {
            continue;
        }
        // Barycentric coordinates of the point: (u, v) with u, v >= 0 and u + v <= 1 inside the triangle.
        let barycentric = to_device.inverse() * point;
        if barycentric.x >= -TOLERANCE && barycentric.y >= -TOLERANCE && barycentric.x + barycentric.y <= 1.0 + TOLERANCE {
            return Some(triangle_basis(source) * barycentric);
        }
    }
    None
}

/// The affine map from the unit triangle `(0,0) (1,0) (0,1)` onto `tri`.
fn triangle_basis(tri: [Point; 3]) -> Affine {
    let [first, second, third] = tri;
    Affine::new([second.x - first.x, second.y - first.y, third.x - first.x, third.y - first.y, first.x, first.y])
}

fn corner_points(list: &[Point], corners: [u32; 3]) -> Option<[Point; 3]> {
    let get = |corner: u32| list.get(usize::try_from(corner).ok()?).copied();
    let [first, second, third] = corners;
    Some([get(first)?, get(second)?, get(third)?])
}

fn invertible(matrix: Affine) -> bool {
    matrix.is_finite() && matrix.determinant().abs() > 1e-18
}

fn hits_fill(shape: &Fill, matrix: Affine, point: Point, images: &ImageStore) -> bool {
    let to_device = matrix * shape.transform;
    if !invertible(to_device) {
        return false;
    }
    let local = to_device.inverse() * point;
    let winding = shape.path.winding(local);
    let inside = match shape.rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    };
    inside && paint_is_visible_at(&shape.paint, local, images)
}

/// Whether the paint has any opacity at `local` (path space).
fn paint_is_visible_at(paint: &Paint, local: Point, images: &ImageStore) -> bool {
    match paint {
        Paint::Solid(color) => color.a > 0,
        Paint::Linear(gradient) | Paint::Radial(gradient) => gradient.stops.iter().any(|stop| stop.color.a > 0),
        Paint::Bitmap { image, transform, repeat, .. } => invertible(*transform) && alpha_at(images, *image, transform.inverse() * local, *repeat) > 0,
    }
}

/// Alpha of the image pixel containing `pixel` (image pixel coordinates), wrapping or clamping outside.
fn alpha_at(images: &ImageStore, id: ImageId, pixel: Point, repeat: bool) -> u8 {
    let Some(image) = images.get(id) else { return 0 };
    if image.width == 0 || image.height == 0 || !pixel.x.is_finite() || !pixel.y.is_finite() {
        return 0;
    }
    let (width, height) = (f64::from(image.width), f64::from(image.height));
    let (x, y) =
        if repeat { (pixel.x.rem_euclid(width), pixel.y.rem_euclid(height)) } else { (pixel.x.clamp(0.0, width - 1.0), pixel.y.clamp(0.0, height - 1.0)) };
    let column = u32::try_from(anim_core::num::floor_f64_to_i32(x).max(0)).unwrap_or(0).min(image.width.saturating_sub(1));
    let row = u32::try_from(anim_core::num::floor_f64_to_i32(y).max(0)).unwrap_or(0).min(image.height.saturating_sub(1));
    let index = u64::from(row).saturating_mul(u64::from(image.width)).saturating_add(u64::from(column)).saturating_mul(4).saturating_add(3);
    usize::try_from(index).ok().and_then(|i| image.premultiplied_rgba.get(i)).copied().unwrap_or(0)
}

fn hits_stroke(stroke: &Stroke, matrix: Affine, point: Point) -> bool {
    let full = matrix * stroke.transform;
    if !full.is_finite() || !stroke.style.width.is_finite() || stroke.style.width < 0.0 {
        return false;
    }
    let (sx, sy) = axis_scales(full);
    let scale = if stroke.style.width <= 0.0 {
        0.0
    } else {
        match stroke.style.scale {
            StrokeScale::Normal => full.determinant().abs().sqrt(),
            StrokeScale::Horizontal => sy,
            StrokeScale::Vertical => sx,
            StrokeScale::None => 0.0,
        }
    };
    let width = if stroke.style.scale == StrokeScale::None || stroke.style.width <= 0.0 { stroke.style.width } else { stroke.style.width * scale };
    let reach = (width.max(MIN_PICK_WIDTH)) / 2.0;
    let mut device: BezPath = stroke.path.clone();
    device.apply_affine(full);
    device.segments().any(|segment| segment.nearest(point, NEAREST_ACCURACY).distance_sq <= reach * reach)
}

fn hits_bitmap(bitmap: &Bitmap, matrix: Affine, point: Point, images: &ImageStore) -> bool {
    let full = matrix * bitmap.transform;
    if !invertible(full) {
        return false;
    }
    let pixel = full.inverse() * point;
    let Some(image) = images.get(bitmap.image) else { return false };
    if pixel.x < 0.0 || pixel.y < 0.0 || pixel.x >= f64::from(image.width) || pixel.y >= f64::from(image.height) {
        return false;
    }
    alpha_at(images, bitmap.image, pixel, false) > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_core::{ColorTransform, Rgba8};
    use anim_display::{Cap, Image, StrokeStyle};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        anim_core::Rect::new(x0, y0, x1, y1).to_path(0.05)
    }

    fn fill(path: BezPath, color: Rgba8) -> Node {
        Node::Fill(Fill { transform: Affine::IDENTITY, path, rule: FillRule::NonZero, paint: Paint::Solid(color) })
    }

    fn list(nodes: Vec<Node>) -> DisplayList {
        DisplayList { nodes, ..DisplayList::default() }
    }

    #[test]
    fn topmost_fill_wins_and_order_is_reported_top_down() {
        let l = list(vec![fill(rect(0.0, 0.0, 10.0, 10.0), Rgba8::BLACK), fill(rect(5.0, 5.0, 15.0, 15.0), Rgba8::WHITE)]);
        let images = ImageStore::new();
        assert_eq!(pick(&l, &images, Affine::IDENTITY, Point::new(7.0, 7.0)).unwrap().path, vec![1]);
        assert_eq!(pick(&l, &images, Affine::IDENTITY, Point::new(2.0, 2.0)).unwrap().path, vec![0]);
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(20.0, 2.0)).is_none());
        let all = pick_all(&l, &images, Affine::IDENTITY, Point::new(7.0, 7.0));
        assert_eq!(all.iter().map(|h| h.path.clone()).collect::<Vec<_>>(), vec![vec![1], vec![0]]);
    }

    #[test]
    fn fill_rules_and_transparent_paints() {
        let mut ring = rect(0.0, 0.0, 20.0, 20.0);
        ring.extend(rect(5.0, 5.0, 15.0, 15.0));
        let even_odd = Node::Fill(Fill { transform: Affine::IDENTITY, path: ring.clone(), rule: FillRule::EvenOdd, paint: Paint::Solid(Rgba8::BLACK) });
        let non_zero = Node::Fill(Fill { transform: Affine::IDENTITY, path: ring, rule: FillRule::NonZero, paint: Paint::Solid(Rgba8::BLACK) });
        let images = ImageStore::new();
        assert!(pick(&list(vec![even_odd]), &images, Affine::IDENTITY, Point::new(10.0, 10.0)).is_none(), "the even-odd hole");
        assert!(pick(&list(vec![non_zero]), &images, Affine::IDENTITY, Point::new(10.0, 10.0)).is_some());
        assert!(pick(&list(vec![fill(rect(0.0, 0.0, 9.0, 9.0), Rgba8::new(1, 2, 3, 0))]), &images, Affine::IDENTITY, Point::new(4.0, 4.0)).is_none());
    }

    #[test]
    fn the_view_and_node_transforms_are_applied() {
        let group = Node::Group(Group {
            transform: Affine::translate((10.0, 0.0)),
            children: vec![fill(rect(0.0, 0.0, 10.0, 10.0), Rgba8::BLACK)],
            ..Group::default()
        });
        let l = list(vec![group]);
        let images = ImageStore::new();
        let view = Affine::scale(2.0);
        assert!(pick(&l, &images, view, Point::new(30.0, 10.0)).is_some());
        assert!(pick(&l, &images, view, Point::new(10.0, 10.0)).is_none());
        assert!(pick(&l, &images, Affine::new([0.0; 6]), Point::new(1.0, 1.0)).is_none(), "a singular view picks nothing");
        assert!(pick(&l, &images, view, Point::new(f64::NAN, 0.0)).is_none());
    }

    #[test]
    fn group_keys_come_outermost_first_and_masks_clip() {
        let inner = Node::Group(Group { key: Some(2), children: vec![fill(rect(0.0, 0.0, 10.0, 10.0), Rgba8::BLACK)], ..Group::default() });
        let outer =
            Node::Group(Group { key: Some(1), mask: Some(vec![fill(rect(0.0, 0.0, 5.0, 10.0), Rgba8::BLACK)]), children: vec![inner], ..Group::default() });
        let l = list(vec![outer]);
        let images = ImageStore::new();
        let hit = pick(&l, &images, Affine::IDENTITY, Point::new(2.0, 5.0)).unwrap();
        assert_eq!((hit.keys, hit.path), (vec![1, 2], vec![0, 0, 0]));
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(8.0, 5.0)).is_none(), "outside the mask");
    }

    #[test]
    fn fully_transparent_groups_cannot_be_hit() {
        let hidden =
            Node::Group(Group { color: ColorTransform::alpha(0.0), children: vec![fill(rect(0.0, 0.0, 10.0, 10.0), Rgba8::BLACK)], ..Group::default() });
        assert!(pick(&list(vec![hidden]), &ImageStore::new(), Affine::IDENTITY, Point::new(5.0, 5.0)).is_none());
    }

    #[test]
    fn strokes_are_hit_within_their_scaled_width() {
        let mut line = BezPath::new();
        line.move_to((0.0, 10.0));
        line.line_to((40.0, 10.0));
        let style = |scale| StrokeStyle { width: 4.0, cap: Cap::None, scale, ..StrokeStyle::default() };
        let stroke = |scale| Node::Stroke(Stroke { transform: Affine::IDENTITY, path: line.clone(), style: style(scale), paint: Paint::Solid(Rgba8::BLACK) });
        let images = ImageStore::new();
        let at = |node: Node, view: Affine, y: f64| pick(&list(vec![node]), &images, view, Point::new(20.0, y)).is_some();
        assert!(at(stroke(StrokeScale::Normal), Affine::IDENTITY, 11.5));
        assert!(!at(stroke(StrokeScale::Normal), Affine::IDENTITY, 13.0));
        // At 3x zoom a normal stroke is 12 px wide, a non-scaling one stays 4.
        assert!(at(stroke(StrokeScale::Normal), Affine::scale(3.0), 30.0 + 5.0));
        assert!(!at(stroke(StrokeScale::None), Affine::scale(3.0), 30.0 + 5.0));
        assert!(at(stroke(StrokeScale::None), Affine::scale(3.0), 30.0 + 1.5));
        // Thin lines stay pickable.
        let hairline = Node::Stroke(Stroke {
            transform: Affine::IDENTITY,
            path: line.clone(),
            style: StrokeStyle { width: 0.0, ..StrokeStyle::default() },
            paint: Paint::Solid(Rgba8::BLACK),
        });
        assert!(at(hairline, Affine::IDENTITY, 10.4));
    }

    #[test]
    fn bitmaps_are_hit_where_their_pixels_are_not_transparent() {
        let mut images = ImageStore::new();
        // Left pixel opaque, right pixel transparent.
        let id = images.insert(Image::new(2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0]).unwrap());
        let bitmap = Node::Bitmap(Bitmap { transform: Affine::scale(10.0), image: id, smoothing: false });
        let l = list(vec![bitmap]);
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(5.0, 5.0)).is_some());
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(15.0, 5.0)).is_none(), "transparent pixel");
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(25.0, 5.0)).is_none(), "outside the image");
        let fill = Node::Fill(Fill {
            transform: Affine::IDENTITY,
            path: rect(0.0, 0.0, 100.0, 10.0),
            rule: FillRule::NonZero,
            paint: Paint::Bitmap { image: id, transform: Affine::scale(10.0), repeat: true, smoothing: false },
        });
        let l = list(vec![fill]);
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(45.0, 5.0)).is_some(), "the repeat has an opaque pixel here");
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(55.0, 5.0)).is_none());
    }

    #[test]
    fn meshes_map_the_point_back_into_their_content() {
        // Content: a small square at the source origin; the mesh moves it 100 px right and stretches it 2x.
        let content = vec![fill(rect(0.0, 0.0, 10.0, 10.0), Rgba8::BLACK)];
        let source = vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0), Point::new(0.0, 20.0), Point::new(20.0, 20.0)];
        let target: Vec<Point> = source.iter().map(|p| Point::new(p.x.mul_add(2.0, 100.0), p.y * 2.0)).collect();
        let mesh = Node::Mesh(Mesh { transform: Affine::IDENTITY, content, source, target, triangles: vec![[0, 1, 2], [1, 3, 2]] });
        let l = list(vec![mesh]);
        let images = ImageStore::new();
        let hit = pick(&l, &images, Affine::IDENTITY, Point::new(110.0, 10.0)).unwrap();
        assert_eq!(hit.path, vec![0, 0]);
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(150.0, 10.0)).is_none(), "inside the mesh but beyond the content");
        assert!(pick(&l, &images, Affine::IDENTITY, Point::new(10.0, 10.0)).is_none(), "outside the mesh");
    }

    #[test]
    fn deep_nesting_and_hit_limits_are_bounded() {
        let mut node = fill(rect(0.0, 0.0, 10.0, 10.0), Rgba8::BLACK);
        for _ in 0..(MAX_DEPTH + 20) {
            node = Node::Group(Group { children: vec![node], ..Group::default() });
        }
        assert!(pick(&list(vec![node]), &ImageStore::new(), Affine::IDENTITY, Point::new(5.0, 5.0)).is_none());
        let many = list((0..(MAX_HITS + 50)).map(|_| fill(rect(0.0, 0.0, 10.0, 10.0), Rgba8::BLACK)).collect());
        assert_eq!(pick_all(&many, &ImageStore::new(), Affine::IDENTITY, Point::new(5.0, 5.0)).len(), MAX_HITS);
    }
}
