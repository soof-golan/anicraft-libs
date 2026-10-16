//! PSD vector masks (`vmsk` / `vsms` blocks) as canvas masks.
//!
//! The block holds Bezier subpaths as fractions of the document size, each with a path operation. They are replayed
//! in order into one coverage shape: combine paints, subtract erases, intersect keeps only the overlap.

use anim_core::{Affine, BezPath, BlendMode, Point, Rgba8, Size};
use anim_display::{Fill, FillRule, Group, Node, Paint};
use photocraft_psd::LayerRecord;
use photocraft_psd::path::{PsdKnot, PsdSubpath, VectorMaskBlock};

use crate::tree::ArtMask;

/// Most subpaths replayed for one mask.
const MAX_SUBPATHS: usize = 4096;

/// The layer's vector mask on a canvas of `canvas` pixels, or `None` if it has none or it is disabled. Notes
/// approximations in `warnings`.
pub fn vector_mask(record: &LayerRecord, canvas: Size, warnings: &mut Vec<String>) -> Option<ArtMask> {
    let block = record.block(b"vmsk").or_else(|| record.block(b"vsms"))?;
    let parsed = VectorMaskBlock::from_bytes(&block.data).ok()?;
    if parsed.flags & VectorMaskBlock::FLAG_DISABLED != 0 {
        return None;
    }
    let mut shapes = Vec::new();
    for sub in parsed.path.subpaths().iter().filter(|s| s.closed).take(MAX_SUBPATHS) {
        let Some(path) = subpath(sub, canvas) else { continue };
        let node = fill(path);
        shapes.push(match sub.operation {
            2 => erasing(BlendMode::Erase, vec![node]),
            3 => erasing(BlendMode::Alpha, vec![node]),
            op => {
                if op == 0 {
                    warnings.push(format!("layer {:?}: vector mask 'exclude' is drawn as 'combine'", record.name()));
                }
                node
            }
        });
    }
    let mut nodes = if parsed.path.initial_fill() { vec![fill(rect_path(canvas))] } else { Vec::new() };
    nodes.append(&mut shapes);
    if parsed.flags & VectorMaskBlock::FLAG_INVERT != 0 {
        // Everything except the shape.
        nodes = vec![fill(rect_path(canvas)), erasing(BlendMode::Erase, nodes)];
    }
    Some(ArtMask::Vector { nodes })
}

const fn fill(path: BezPath) -> Node {
    Node::Fill(Fill { transform: Affine::IDENTITY, path, rule: FillRule::NonZero, paint: Paint::Solid(Rgba8::BLACK) })
}

fn erasing(blend: BlendMode, children: Vec<Node>) -> Node {
    Node::Group(Group { blend, children, ..Group::default() })
}

fn rect_path(canvas: Size) -> BezPath {
    let mut path = BezPath::new();
    path.move_to(Point::ORIGIN);
    path.line_to(Point::new(canvas.width, 0.0));
    path.line_to(Point::new(canvas.width, canvas.height));
    path.line_to(Point::new(0.0, canvas.height));
    path.close_path();
    path
}

fn subpath(sub: &PsdSubpath, canvas: Size) -> Option<BezPath> {
    let at = |p: (f64, f64)| Point::new(p.0 * canvas.width, p.1 * canvas.height);
    let first = sub.knots.first()?;
    let mut path = BezPath::new();
    path.move_to(at(first.anchor));
    let curve = |path: &mut BezPath, from: &PsdKnot, to: &PsdKnot| path.curve_to(at(from.post), at(to.pre), at(to.anchor));
    for (from, to) in sub.knots.iter().zip(sub.knots.iter().skip(1)) {
        curve(&mut path, from, to);
    }
    if let Some(last) = sub.knots.last() {
        curve(&mut path, last, first);
    }
    path.close_path();
    Some(path)
}
