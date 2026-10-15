//! AI/PDF import on files this test builds with `VectorCraft`'s own writers (nothing from Adobe): a legacy
//! PostScript `.ai` and a PDF with optional content groups as layers.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]
#![allow(clippy::disallowed_types, reason = "VectorCraft's document model shares nodes with Arc; the test builds one")]

use std::sync::Arc;

use anim_artwork::{ArtKind, ArtMask, ArtNode, Limits, import_ai, to_display};
use anim_core::{BlendMode, Rgba8};
use anim_display::{FillRule, ImageStore, Node as DisplayNode, Paint};
use vectorcraft_color::{Color, Gradient, GradientKind, GradientPaint, GradientStop, Paint as VcPaint};
use vectorcraft_doc::{Appearance, Document, ImageBlob, ImageObject, Node, NodeKind};
use vectorcraft_geom::{Affine, FillRule as VcFillRule, Rect, shapes};

fn shape(doc: &mut Document, path: vectorcraft_geom::PathData, fill: VcPaint) -> Node {
    Node::path(doc.alloc_id(), path, Appearance::basic(fill, VcPaint::None, 0.0))
}

fn solid(r: f32, g: f32, b: f32) -> VcPaint {
    VcPaint::solid(Color::rgb(r, g, b))
}

/// Layer `Back` (a group `Eyes` of two squares, a ring, a clipped group, a gradient square) and a hidden layer
/// `Front` (an ellipse and a 2x1 image), on a 200x100 artboard.
fn sample() -> Document {
    let mut doc = Document::new(200.0, 100.0);
    doc.layers.clear();
    let back = doc.add_layer(Some("Back"));
    let front = doc.add_layer(Some("Front"));

    let left = shape(&mut doc, shapes::rectangle(Rect::new(10.0, 10.0, 40.0, 40.0)), solid(1.0, 0.0, 0.0));
    let right = shape(&mut doc, shapes::rectangle(Rect::new(50.0, 10.0, 80.0, 40.0)), solid(0.0, 1.0, 0.0));
    let mut eyes = Node::group(doc.alloc_id(), vec![Arc::new(left), Arc::new(right)]);
    eyes.name = Some("Eyes \u{05E2}\u{05D9}\u{05E0}\u{05D9}\u{05D9}\u{05DD}".to_owned());
    doc.insert(Some(back), usize::MAX, eyes).unwrap();

    let outer = shape(&mut doc, shapes::rectangle(Rect::new(100.0, 10.0, 140.0, 50.0)), VcPaint::None);
    let inner = shape(&mut doc, shapes::rectangle(Rect::new(110.0, 20.0, 130.0, 40.0)), VcPaint::None);
    let mut ring = Node::new(doc.alloc_id(), NodeKind::Compound { children: vec![Arc::new(outer), Arc::new(inner)], rule: VcFillRule::EvenOdd });
    ring.appearance = Appearance::basic(solid(0.0, 0.0, 1.0), VcPaint::None, 0.0).into();
    doc.insert(Some(back), usize::MAX, ring).unwrap();

    let clip = Node::new(
        doc.alloc_id(),
        NodeKind::Path { path: shapes::rectangle(Rect::new(160.0, 20.0, 180.0, 40.0)), rule: VcFillRule::NonZero, live: None, clipping: true, guide: false },
    );
    let clipped = shape(&mut doc, shapes::rectangle(Rect::new(150.0, 10.0, 190.0, 50.0)), VcPaint::solid(Color::gray(0.25)));
    let window = Node::new(doc.alloc_id(), NodeKind::Group { children: vec![Arc::new(clip), Arc::new(clipped)], clip: true });
    doc.insert(Some(back), usize::MAX, window).unwrap();

    let gradient = Gradient {
        kind: GradientKind::Linear,
        stops: vec![GradientStop::new(0.0, Color::rgb(1.0, 1.0, 1.0)), GradientStop::new(1.0, Color::rgb(0.0, 0.0, 0.0))],
    };
    let shaded = shape(&mut doc, shapes::rectangle(Rect::new(10.0, 60.0, 50.0, 90.0)), VcPaint::Gradient(Box::new(GradientPaint::new(gradient))));
    doc.insert(Some(back), usize::MAX, shaded).unwrap();

    let ellipse = shape(&mut doc, shapes::ellipse(Rect::new(100.0, 60.0, 160.0, 90.0)), solid(1.0, 0.0, 0.0));
    doc.insert(Some(front), usize::MAX, ellipse).unwrap();
    let mut png = vec![];
    let img = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let blob = ImageBlob::new("image/png", png);
    let key = blob.content_key();
    doc.images.insert(key.clone(), blob);
    let object = ImageObject {
        key,
        width: 2,
        height: 1,
        xf: Affine::new([10.0, 0.0, 0.0, 10.0, 170.0, 70.0]),
        link: None,
        placement: vectorcraft_doc::PlacementOptions::default(),
    };
    let image = Node::new(doc.alloc_id(), NodeKind::Image(Box::new(object)));
    doc.insert(Some(front), usize::MAX, image).unwrap();
    if let Some(layer) = doc.layers.get_mut(1) {
        Arc::make_mut(layer).visible = false;
    }
    doc
}

fn check_hierarchy(tree: &anim_artwork::ArtworkTree) {
    assert_eq!((tree.size.width, tree.size.height), (200.0, 100.0), "{}", tree.outline());
    let back = tree.root.find_path(&["Back"]).unwrap_or_else(|| panic!("no Back layer in\n{}", tree.outline()));
    let front = tree.root.find_path(&["Front"]).unwrap();
    assert!(back.visible && back.is_group());
    assert!(!front.visible);
    // Back is below Front.
    assert_eq!(tree.root.children().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Back", "Front"]);
    assert_eq!(back.children().len(), 4, "{}", tree.outline());
}

#[test]
fn legacy_ai_layers_groups_and_paints() {
    let out = vectorcraft_eps::export_legacy_ai(&sample(), &vectorcraft_eps::LegacyAiOptions::default()).unwrap();
    let tree = import_ai(&out.bytes, &Limits::default()).unwrap();
    println!("{}", tree.outline());
    check_hierarchy(&tree);
    let back = tree.find("Back").unwrap();

    // The group of two squares: red then green, drawn at the artboard-relative positions.
    let group = &back.children()[0];
    assert!(group.is_group());
    let colours: Vec<Rgba8> = group
        .children()
        .iter()
        .map(|c| {
            let ArtKind::Vector { nodes, .. } = &c.kind else { panic!("vector expected") };
            let DisplayNode::Fill(fill) = &nodes[0] else { panic!("fill expected") };
            let Paint::Solid(c) = fill.paint else { panic!("solid expected") };
            c
        })
        .collect();
    assert_eq!(colours, [Rgba8::opaque(255, 0, 0), Rgba8::opaque(0, 255, 0)]);
    let bounds = group.bounds().unwrap();
    assert!((bounds.x0 - 10.0).abs() < 0.5 && (bounds.x1 - 80.0).abs() < 0.5, "{bounds:?}");

    // The compound ring keeps the even-odd rule and both contours.
    let ring = &back.children()[1];
    let ArtKind::Vector { nodes, .. } = &ring.kind else { panic!("vector expected") };
    let DisplayNode::Fill(ring_fill) = &nodes[0] else { panic!("fill expected") };
    assert_eq!(ring_fill.rule, FillRule::EvenOdd);
    assert_eq!(ring_fill.path.elements().iter().filter(|e| matches!(e, anim_core::kurbo::PathEl::MoveTo(_))).count(), 2);

    // The clipping group has its clip path as mask and keeps the clipped art.
    let window = &back.children()[2];
    assert!(matches!(window.mask, Some(ArtMask::Vector { ref nodes }) if !nodes.is_empty()), "{}", tree.outline());
    assert_eq!(window.children().len(), 1);

    // The gradient square has a two-stop linear gradient from white to black across its width.
    let shaded = &back.children()[3];
    let ArtKind::Vector { nodes, .. } = &shaded.kind else { panic!("vector expected") };
    let DisplayNode::Fill(shaded_fill) = &nodes[0] else { panic!("fill expected") };
    let Paint::Linear(g) = &shaded_fill.paint else { panic!("linear gradient expected, got {:?}", shaded_fill.paint) };
    assert!(g.stops.len() >= 2);
    assert_eq!(g.stops.first().unwrap().color, Rgba8::WHITE);
    assert_eq!(g.stops.last().unwrap().color, Rgba8::BLACK);

    // The hidden layer's image comes in as pixels.
    let front = tree.find("Front").unwrap();
    assert!(front.children().iter().any(|c| matches!(c.kind, ArtKind::Raster { .. })), "{}", tree.outline());
}

#[test]
fn legacy_ai_renders_through_display_list() {
    let out = vectorcraft_eps::export_legacy_ai(&sample(), &vectorcraft_eps::LegacyAiOptions::default()).unwrap();
    let tree = import_ai(&out.bytes, &Limits::default()).unwrap();
    let mut store = ImageStore::new();
    let list = to_display(&tree, &mut store);
    // The hidden Front layer is not in the display list: only Back's content is.
    assert_eq!(list.nodes.len(), 1);
    let mut fills = 0;
    list.visit(&mut |n| fills += usize::from(matches!(n, DisplayNode::Fill(_))));
    assert!(fills >= 5, "{fills}");
    assert_eq!(store.len(), 0);
}

#[test]
fn pdf_optional_content_groups_become_layers() {
    let settings: vectorcraft_pdf::PdfSettings = serde_json::from_value(serde_json::json!({"createLayers": true})).unwrap();
    let report = vectorcraft_pdf::export_with_report(&sample(), &vectorcraft_pdf::PdfOptions { settings, ..Default::default() }).unwrap();
    let tree = import_ai(&report.bytes, &Limits::default()).unwrap();
    println!("{}", tree.outline());
    let names: Vec<&str> = tree.root.children().iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Back"), "{names:?}");
    assert!(names.contains(&"Front"), "{names:?}");
    // Both layers hold art; the layer that was hidden comes in hidden.
    let back: &ArtNode = tree.find("Back").unwrap();
    assert!(back.visible);
    assert!(back.count() > 3);
    if let Some(front) = tree.find("Front") {
        assert!(!front.visible, "{}", tree.outline());
    }
    assert_eq!(back.blend, BlendMode::Normal);
}
