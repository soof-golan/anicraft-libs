//! Stage bounds of elements, used by selection, "convert to symbol" and zoom-to-fit.

use anim_core::{Affine, Rect};

use crate::element::Element;
use crate::library::Library;
use crate::timeline::Timeline;

/// How many symbol instances deep bounds are followed (guards against symbols that contain themselves).
pub const MAX_SYMBOL_DEPTH: usize = 16;

/// Bounds of an element in its parent's coordinates (its own matrix applied).
///
/// Symbol instances use the content of their `first_frame`; text uses its field rectangle; bitmaps use the library
/// item's known size.
#[must_use]
pub fn element_bounds(element: &Element, library: &Library) -> Option<Rect> {
    bounds_at(element, library, 0, 0)
}

/// Union of the bounds of the given elements.
#[must_use]
pub fn elements_bounds<'a>(elements: impl IntoIterator<Item = &'a Element>, library: &Library) -> Option<Rect> {
    elements.into_iter().filter_map(|e| element_bounds(e, library)).reduce(|a, b| a.union(b))
}

/// Bounds of what a timeline shows at `frame`, over all visible layers.
#[must_use]
pub fn timeline_bounds(timeline: &Timeline, frame: u32, library: &Library) -> Option<Rect> {
    timeline_bounds_at(timeline, frame, library, 0)
}

fn timeline_bounds_at(timeline: &Timeline, frame: u32, library: &Library, symbol_depth: usize) -> Option<Rect> {
    timeline
        .layers
        .iter()
        .filter(|l| l.visible && !l.kind.is_structural())
        .filter_map(|l| l.keyframe_at(frame))
        .flat_map(|f| f.elements.iter())
        .filter_map(|e| bounds_at(e, library, 0, symbol_depth))
        .reduce(|a, b| a.union(b))
}

fn bounds_at(element: &Element, library: &Library, group_depth: usize, symbol_depth: usize) -> Option<Rect> {
    match element {
        Element::Shape(s) => transform_rect(s.matrix, s.shape.bounds_with_strokes()?),
        Element::Text(t) => transform_rect(t.matrix, Rect::new(0.0, 0.0, t.width, t.height)),
        Element::Bitmap(b) => {
            let [w, h] = library.find_bitmap(&b.library_item_name)?.size?;
            transform_rect(b.matrix, Rect::new(0.0, 0.0, w, h))
        }
        Element::Group(g) => {
            if group_depth >= crate::MAX_GROUP_DEPTH {
                return None;
            }
            let inner = g.members.iter().filter_map(|m| bounds_at(m, library, group_depth.saturating_add(1), symbol_depth)).reduce(|a, b| a.union(b))?;
            transform_rect(g.matrix, inner)
        }
        Element::SymbolInstance(i) => {
            if symbol_depth >= MAX_SYMBOL_DEPTH {
                return None;
            }
            let symbol = library.find_symbol(&i.library_item_name)?;
            let inner = timeline_bounds_at(&symbol.timeline, i.first_frame, library, symbol_depth.saturating_add(1))?;
            transform_rect(i.matrix, inner)
        }
        Element::Unknown(_) => None,
    }
}

fn transform_rect(matrix: Affine, rect: Rect) -> Option<Rect> {
    let r = matrix.transform_rect_bbox(rect);
    (r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()).then_some(r)
}

#[cfg(test)]
mod tests {
    use anim_core::{Point, Rgba8};
    use anim_shape::{EdgeShape, FillStyle, PathInput};

    use super::*;
    use crate::element::{Group, ShapeElement, SymbolInstance, TextElement, TextKind};
    use crate::library::{BitmapItem, LibraryItem};

    fn square(x: f64, y: f64, s: f64) -> Element {
        let mut p = anim_core::BezPath::new();
        p.move_to(Point::new(x, y));
        p.line_to(Point::new(x + s, y));
        p.line_to(Point::new(x + s, y + s));
        p.line_to(Point::new(x, y + s));
        p.close_path();
        Element::Shape(ShapeElement::new(EdgeShape::from_paths(&[PathInput::new(p, Some(FillStyle::Solid(Rgba8::BLACK)), None)])))
    }

    #[test]
    fn shape_group_and_text_bounds() {
        let lib = Library::new();
        let b = element_bounds(&square(10.0, 20.0, 30.0), &lib).unwrap();
        assert!((b.x0 - 10.0).abs() < 0.1 && (b.y1 - 50.0).abs() < 0.1);
        let mut g = Group::new(vec![square(0.0, 0.0, 10.0), square(100.0, 0.0, 10.0)]);
        g.matrix = Affine::translate((5.0, 5.0));
        let gb = element_bounds(&Element::Group(g), &lib).unwrap();
        assert!((gb.x0 - 5.0).abs() < 0.1 && (gb.x1 - 115.0).abs() < 0.1);
        let t = Element::Text(Box::new(TextElement::new(TextKind::Static, Affine::translate((1.0, 2.0)), 50.0, 10.0, vec![])));
        assert_eq!(element_bounds(&t, &lib), Some(Rect::new(1.0, 2.0, 51.0, 12.0)));
        assert!(element_bounds(&Element::Unknown(crate::RawXml::default()), &lib).is_none());
    }

    #[test]
    fn symbol_instances_use_symbol_content_and_stop_at_cycles() {
        let mut doc = crate::Document::default();
        doc.add_symbol("box", crate::SymbolKind::Graphic).unwrap();
        doc.add_symbol("loop", crate::SymbolKind::Graphic).unwrap();
        if let Some(s) = doc.library.find_symbol_mut("box") {
            s.timeline.layers[0].frames[0].elements.push(square(0.0, 0.0, 10.0));
        }
        if let Some(s) = doc.library.find_symbol_mut("loop") {
            s.timeline.layers[0].frames[0].elements.push(Element::SymbolInstance(Box::new(SymbolInstance::new("loop", Affine::IDENTITY))));
        }
        let inst = Element::SymbolInstance(Box::new(SymbolInstance::new("box", Affine::scale(2.0))));
        let b = element_bounds(&inst, &doc.library).unwrap();
        assert!((b.x1 - 20.0).abs() < 0.1);
        let cyc = Element::SymbolInstance(Box::new(SymbolInstance::new("loop", Affine::IDENTITY)));
        assert!(element_bounds(&cyc, &doc.library).is_none());
        assert!(element_bounds(&Element::SymbolInstance(Box::new(SymbolInstance::new("missing", Affine::IDENTITY))), &doc.library).is_none());
    }

    #[test]
    fn bitmap_bounds_need_a_known_size() {
        let mut lib = Library::new();
        let mut item = BitmapItem::new("pic", "x");
        lib.push(LibraryItem::Bitmap(item.clone())).unwrap();
        let e = Element::Bitmap(crate::BitmapInstance::new("pic", Affine::IDENTITY));
        assert!(element_bounds(&e, &lib).is_none());
        lib.remove("pic");
        item.size = Some([64.0, 32.0]);
        lib.push(LibraryItem::Bitmap(item)).unwrap();
        assert_eq!(element_bounds(&e, &lib), Some(Rect::new(0.0, 0.0, 64.0, 32.0)));
        assert_eq!(elements_bounds([&e, &e], &lib), Some(Rect::new(0.0, 0.0, 64.0, 32.0)));
    }
}
