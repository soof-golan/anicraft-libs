//! SVG exports: well-formedness, reference integrity and content checks.

// Integration tests may panic and use plain arithmetic; the no-panic rules are for shipped code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::unreachable,
    clippy::manual_is_multiple_of,
    clippy::chunks_exact_to_as_chunks,
    clippy::needless_range_loop
)]

mod common;

use std::collections::BTreeSet;
use std::io::Cursor;

use anim_core::{Affine, Point, Rgba8, Size};
use anim_display::{DisplayList, Fill, FillRule, Group, ImageStore, Mesh, Node, Paint};
use anim_export::{
    AnimatedSvgOptions, ExportError, SvgAnimation, SvgOptions, VecFrameSource, display_list_to_svg, export_animated_svg, export_animated_svg_file,
    export_svg_file, frame_to_svg, no_progress,
};
use base64::Engine as _;
use common::{BLUE, GREEN, RED, kitchen_sink, moving_square, rect_path, solid_rect};
use roxmltree::Document;

const XLINK: &str = "http://www.w3.org/1999/xlink";

fn parse(svg: &str) -> Document<'_> {
    Document::parse(svg).unwrap_or_else(|e| panic!("not well-formed: {e}\n{svg}"))
}

fn count(doc: &Document<'_>, tag: &str) -> usize {
    doc.descendants().filter(|n| n.has_tag_name(tag)).count()
}

/// Every `url(#id)` and `xlink:href="#id"` must name an element that exists.
fn assert_references_resolve(doc: &Document<'_>) {
    let ids: BTreeSet<&str> = doc.descendants().filter_map(|n| n.attribute("id")).collect();
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        for attr in node.attributes() {
            let v = attr.value();
            if let Some(rest) = v.strip_prefix("url(#") {
                let id = rest.trim_end_matches(')');
                assert!(ids.contains(id), "dangling {v} on <{}>", node.tag_name().name());
            }
            if let (true, Some(target)) = (attr.name() == "href", v.strip_prefix('#')) {
                assert!(ids.contains(target), "dangling href {v}");
            }
        }
    }
    // Ids are unique.
    assert_eq!(doc.descendants().filter_map(|n| n.attribute("id")).count(), ids.len(), "duplicate ids");
}

#[test]
fn static_svg_is_well_formed_and_covers_every_feature() {
    let (list, images) = kitchen_sink();
    let svg = display_list_to_svg(&list, &images, &SvgOptions::default()).unwrap();
    let doc = parse(&svg);
    assert_references_resolve(&doc);
    let root = doc.root_element();
    assert_eq!(root.tag_name().name(), "svg");
    assert_eq!(root.attribute("viewBox"), Some("0 0 200 120"));
    assert_eq!(root.attribute("width"), Some("200"));

    assert_eq!(count(&doc, "linearGradient"), 1);
    assert_eq!(count(&doc, "radialGradient"), 1);
    assert_eq!(count(&doc, "pattern"), 1);
    assert_eq!(count(&doc, "image"), 1, "one image definition even though it is used three times");
    assert_eq!(count(&doc, "mask"), 1);
    assert_eq!(count(&doc, "filter"), 1);
    assert_eq!(count(&doc, "clipPath"), 2, "one per mesh triangle");
    assert!(svg.contains("mix-blend-mode:multiply"));
    assert!(svg.contains("isolation:isolate"));
    assert!(svg.contains("opacity=\"0.5\""));
    assert!(svg.contains("fill-rule=\"evenodd\""));
    assert!(svg.contains("stroke-dasharray=\"4 2\""));
    assert!(svg.contains("vector-effect=\"non-scaling-stroke\""));
    assert!(svg.contains("stroke-linecap=\"square\""));
    assert!(svg.contains("stroke-miterlimit=\"4\""));
    assert!(svg.contains("spreadMethod=\"reflect\""));
    assert!(svg.contains("image-rendering:pixelated"));
    assert!(svg.contains("mask-type=\"alpha\""));
    assert!(!svg.contains("data-key"), "keys are opt-in");

    // Background first.
    let bg = doc.root_element().children().find(|n| n.has_tag_name("rect")).unwrap();
    assert_eq!(bg.attribute("fill"), Some("#0a141e"));
}

#[test]
fn gradients_carry_their_transform_and_stops() {
    let (list, images) = kitchen_sink();
    let svg = display_list_to_svg(&list, &images, &SvgOptions::default()).unwrap();
    let doc = parse(&svg);
    let lin = doc.descendants().find(|n| n.has_tag_name("linearGradient")).unwrap();
    assert_eq!(lin.attribute("gradientUnits"), Some("userSpaceOnUse"));
    assert_eq!(lin.attribute("x1"), Some("-819.2"));
    assert_eq!(lin.attribute("x2"), Some("819.2"));
    assert_eq!(lin.attribute("gradientTransform"), Some("matrix(0.05 0 0 0.05 0 0)"));
    let stops: Vec<_> = lin.children().filter(|n| n.has_tag_name("stop")).collect();
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[1].attribute("stop-color"), Some("#00ff00"));
    assert_eq!(stops[2].attribute("stop-opacity"), Some("0.502"));
    assert!(stops[0].attribute("stop-opacity").is_none());
    let rad = doc.descendants().find(|n| n.has_tag_name("radialGradient")).unwrap();
    assert_eq!(rad.attribute("r"), Some("819.2"));
    assert_eq!(rad.attribute("fx"), Some("327.68"));
}

#[test]
fn embedded_image_is_a_straight_alpha_png() {
    let (list, images) = kitchen_sink();
    let svg = display_list_to_svg(&list, &images, &SvgOptions::default()).unwrap();
    let doc = parse(&svg);
    let image = doc.descendants().find(|n| n.has_tag_name("image")).unwrap();
    assert_eq!((image.attribute("width"), image.attribute("height")), (Some("2"), Some("2")));
    let href = image.attribute((XLINK, "href")).unwrap();
    let data = href.strip_prefix("data:image/png;base64,").unwrap();
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    assert_eq!(&buf[0..4], &[255, 0, 0, 255]);
    assert_eq!(&buf[12..16], &[255, 255, 255, 128], "premultiplied 128/128 un-premultiplies to white at half alpha");
}

#[test]
fn group_nesting_orders_filter_mask_and_blend() {
    let (list, images) = kitchen_sink();
    let svg = display_list_to_svg(&list, &images, &SvgOptions { emit_keys: true, ..SvgOptions::default() }).unwrap();
    let doc = parse(&svg);
    let outer = doc.descendants().find(|n| n.attribute("data-key") == Some("7")).unwrap();
    assert!(outer.attribute("transform").unwrap().starts_with("matrix(0.98"));
    assert!(outer.attribute("style").unwrap().contains("multiply"));
    let filtered = outer.children().find(roxmltree::Node::is_element).unwrap();
    assert!(filtered.attribute("filter").is_some());
    let masked = filtered.children().find(roxmltree::Node::is_element).unwrap();
    assert!(masked.attribute("mask").is_some());
    assert!(masked.children().any(|n| n.has_tag_name("path")));
    // The filter chain: blur, shadow pieces, glow, colour matrix and the tint.
    let filter = doc.descendants().find(|n| n.has_tag_name("filter")).unwrap();
    assert_eq!(filter.attribute("color-interpolation-filters"), Some("sRGB"));
    let prims: BTreeSet<&str> = filter.children().filter(roxmltree::Node::is_element).map(|n| n.tag_name().name()).collect();
    for want in ["feGaussianBlur", "feOffset", "feFlood", "feComposite", "feMerge", "feColorMatrix", "feComponentTransfer"] {
        assert!(prims.contains(want), "missing {want}: {prims:?}");
    }
}

#[test]
fn mirrored_group_keeps_its_negative_determinant() {
    // Right-to-left runs are laid out with mirrored matrices; they must reach the SVG unchanged.
    let (list, images) = kitchen_sink();
    let svg = display_list_to_svg(&list, &images, &SvgOptions::default()).unwrap();
    assert!(svg.contains("matrix(-1 0 0 1 190 40)"), "{svg}");
}

#[test]
fn mesh_becomes_clipped_affine_copies() {
    let (list, images) = kitchen_sink();
    let svg = display_list_to_svg(&list, &images, &SvgOptions::default()).unwrap();
    let doc = parse(&svg);
    let clipped: Vec<_> = doc.descendants().filter(|n| n.attribute("clip-path").is_some()).collect();
    assert_eq!(clipped.len(), 2);
    for g in clipped {
        let used = g.children().find(|n| n.has_tag_name("use")).unwrap();
        assert!(used.attribute("transform").unwrap().starts_with("matrix("));
    }
}

#[test]
fn background_can_be_left_out_and_scale_resizes() {
    let (list, images) = kitchen_sink();
    let svg = display_list_to_svg(&list, &images, &SvgOptions { background: false, scale: 2.0, ..SvgOptions::default() }).unwrap();
    let doc = parse(&svg);
    assert!(!doc.root_element().children().any(|n| n.has_tag_name("rect")));
    assert_eq!(doc.root_element().attribute("width"), Some("400"));
    assert_eq!(doc.root_element().attribute("viewBox"), Some("0 0 200 120"));
    for bad in [0.0, -1.0, f64::NAN] {
        let err = display_list_to_svg(&list, &images, &SvgOptions { scale: bad, ..SvgOptions::default() }).unwrap_err();
        assert!(matches!(err, ExportError::InvalidSetting(_)));
    }
}

#[test]
fn identical_gradients_and_images_are_defined_once() {
    let mut list = DisplayList::new(Size::new(50.0, 50.0), None);
    let g = common::gradient(&[(0.0, RED), (1.0, BLUE)], anim_display::Spread::Pad);
    for i in 0..5 {
        list.nodes.push(Node::Fill(Fill {
            transform: Affine::translate((f64::from(i) * 5.0, 0.0)),
            path: rect_path(0.0, 0.0, 4.0, 4.0),
            rule: FillRule::NonZero,
            paint: Paint::Linear(g.clone()),
        }));
    }
    let svg = display_list_to_svg(&list, &ImageStore::new(), &SvgOptions::default()).unwrap();
    let doc = parse(&svg);
    assert_eq!(count(&doc, "linearGradient"), 1);
    assert_eq!(count(&doc, "path"), 5);
    assert_references_resolve(&doc);
}

#[test]
fn hostile_display_lists_still_produce_valid_svg() {
    let mut list = DisplayList::new(Size::new(100.0, 100.0), None);
    // NaN and infinite coordinates, a singular matrix, an empty mask, a missing image, a degenerate mesh.
    let mut bad = rect_path(0.0, 0.0, 10.0, 10.0);
    bad.line_to((f64::NAN, 1.0));
    list.nodes.push(Node::Fill(Fill { transform: Affine::IDENTITY, path: bad, rule: FillRule::NonZero, paint: Paint::Solid(RED) }));
    list.nodes.push(Node::Fill(Fill {
        transform: Affine::new([f64::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0]),
        path: rect_path(0.0, 0.0, 1.0, 1.0),
        rule: FillRule::NonZero,
        paint: Paint::Solid(RED),
    }));
    list.nodes.push(Node::Group(Group {
        transform: Affine::scale_non_uniform(0.0, 1.0),
        filters: vec![anim_display::Filter::Blur { x: 1e12, y: f64::NAN, quality: 255 }],
        children: vec![solid_rect(0.0, 0.0, 5.0, 5.0, GREEN)],
        ..Group::default()
    }));
    list.nodes.push(Node::Group(Group { mask: Some(Vec::new()), children: vec![solid_rect(0.0, 0.0, 5.0, 5.0, GREEN)], ..Group::default() }));
    list.nodes.push(Node::Bitmap(anim_display::Bitmap { transform: Affine::IDENTITY, image: anim_display::ImageId(99), smoothing: true }));
    list.nodes.push(Node::Mesh(Mesh {
        transform: Affine::IDENTITY,
        content: vec![solid_rect(0.0, 0.0, 5.0, 5.0, GREEN)],
        source: vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0), Point::new(2.0, 2.0)],
        target: vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0), Point::new(2.0, 2.0)],
        triangles: vec![[0, 1, 2], [0, 1, 99]],
    }));
    list.nodes.push(solid_rect(1e300, -1e300, 1e300, 1e300, BLUE));
    let svg = display_list_to_svg(&list, &ImageStore::new(), &SvgOptions::default()).unwrap();
    let doc = parse(&svg);
    assert_references_resolve(&doc);
    assert!(!svg.contains("NaN") && !svg.contains("inf"));
}

#[test]
fn deep_nesting_is_cut_off_not_overflowed() {
    let mut node = solid_rect(0.0, 0.0, 5.0, 5.0, RED);
    for _ in 0..500 {
        node = Node::Group(Group { transform: Affine::translate((0.1, 0.0)), children: vec![node], ..Group::default() });
    }
    let mut list = DisplayList::new(Size::new(10.0, 10.0), None);
    list.nodes.push(node);
    let svg = display_list_to_svg(&list, &ImageStore::new(), &SvgOptions::default()).unwrap();
    parse(&svg);
}

#[test]
fn byte_limit_is_enforced() {
    let (list, images) = kitchen_sink();
    let err = display_list_to_svg(&list, &images, &SvgOptions { max_bytes: 200, ..SvgOptions::default() }).unwrap_err();
    assert!(matches!(err, ExportError::TooLarge(_)));
}

#[test]
fn frame_and_file_helpers() {
    let src = moving_square(3);
    let svg = frame_to_svg(&src, 2, &SvgOptions::default()).unwrap();
    let doc = parse(&svg);
    assert_eq!(doc.root_element().attribute("width"), Some("64"));
    assert!(svg.contains("M16 16 L32 16"), "{svg}");
    assert!(matches!(frame_to_svg(&src, 3, &SvgOptions::default()), Err(ExportError::FrameOutOfRange { .. })));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x/frame.svg");
    export_svg_file(&src, 0, &path, &SvgOptions::default()).unwrap();
    parse(&std::fs::read_to_string(path).unwrap());
}

fn frame_groups<'a>(doc: &'a Document<'_>) -> Vec<roxmltree::Node<'a, 'a>> {
    doc.descendants().filter(|n| n.attribute("id").is_some_and(|i| i.starts_with("frame"))).collect()
}

fn track(g: roxmltree::Node<'_, '_>) -> (Vec<String>, Vec<f64>) {
    let animate = g.children().find(|n| n.has_tag_name("animate")).unwrap();
    let values = animate.attribute("values").unwrap().split(';').map(str::to_owned).collect();
    let times = animate.attribute("keyTimes").unwrap().split(';').map(|t| t.parse().unwrap()).collect();
    (values, times)
}

#[test]
fn animated_smil_has_one_group_per_frame_with_valid_tracks() {
    let src = moving_square(6);
    let svg = export_animated_svg(&src, &AnimatedSvgOptions::default(), &mut no_progress).unwrap();
    let doc = parse(&svg);
    assert_references_resolve(&doc);
    let groups = frame_groups(&doc);
    assert_eq!(groups.len(), 6);
    for (i, g) in groups.iter().enumerate() {
        let (values, times) = track(*g);
        assert_eq!(values.len(), times.len());
        assert_eq!(times[0], 0.0);
        assert_eq!(*times.last().unwrap(), 1.0);
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(values[0] == "visible", i == 0);
        let animate = g.children().find(|n| n.has_tag_name("animate")).unwrap();
        assert_eq!(animate.attribute("calcMode"), Some("discrete"));
        assert_eq!(animate.attribute("repeatCount"), Some("indefinite"));
        // 6 frames at 24 fps.
        assert_eq!(animate.attribute("dur"), Some("0.25s"));
        // The group is visible exactly during its frame: it switches on at i/6.
        if i > 0 {
            assert!((times[1] - i as f64 / 6.0).abs() < 1e-5, "frame {i}: {times:?}");
            assert_eq!(values[1], "visible");
        }
    }
    // The shared background is drawn once, outside the frame groups.
    assert_eq!(doc.root_element().children().filter(|n| n.has_tag_name("rect")).count(), 1);
}

#[test]
fn animated_once_freezes_and_css_mode_uses_keyframes() {
    let src = moving_square(4);
    let once = AnimatedSvgOptions { repeat: false, ..AnimatedSvgOptions::default() };
    let svg = export_animated_svg(&src, &once, &mut no_progress).unwrap();
    let doc = parse(&svg);
    let last = frame_groups(&doc).pop().unwrap();
    let animate = last.children().find(|n| n.has_tag_name("animate")).unwrap();
    assert_eq!(animate.attribute("fill"), Some("freeze"));
    assert_eq!(animate.attribute("repeatCount"), Some("1"));
    assert!(animate.attribute("values").unwrap().ends_with("visible"), "the last frame stays on screen");

    let css = AnimatedSvgOptions { animation: SvgAnimation::Css, ..AnimatedSvgOptions::default() };
    let svg = export_animated_svg(&src, &css, &mut no_progress).unwrap();
    let doc = parse(&svg);
    let style = doc.descendants().find(|n| n.has_tag_name("style")).unwrap().text().unwrap();
    assert_eq!(style.matches("@keyframes").count(), 4);
    assert!(style.contains("animation:k1 0.166667s step-end infinite"), "{style}");
    assert!(!svg.contains("<animate"));
}

#[test]
fn animated_merges_held_frames() {
    // Frames: A A A B B A -> A has two runs, B one; only two groups are written.
    let a = moving_square(1).frames.remove(0);
    let mut b = a.clone();
    b.nodes.push(solid_rect(40.0, 0.0, 4.0, 4.0, GREEN));
    let frames = vec![a.clone(), a.clone(), a.clone(), b.clone(), b, a];
    let src = VecFrameSource::new(Size::new(64.0, 48.0), 12.0, frames, ImageStore::new());
    let svg = export_animated_svg(&src, &AnimatedSvgOptions::default(), &mut no_progress).unwrap();
    let doc = parse(&svg);
    let groups = frame_groups(&doc);
    assert_eq!(groups.len(), 3, "A, B, A again (non-adjacent repeats are separate)");
    let (values, times) = track(groups[0]);
    assert_eq!(values, ["visible", "hidden", "hidden"]);
    assert!((times[1] - 0.5).abs() < 1e-6, "A shows for the first three of six frames: {times:?}");

    let unmerged = AnimatedSvgOptions { merge_holds: false, ..AnimatedSvgOptions::default() };
    let doc2 = export_animated_svg(&src, &unmerged, &mut no_progress).unwrap();
    assert_eq!(frame_groups(&parse(&doc2)).len(), 6);
}

#[test]
fn single_static_frame_has_no_animation_element() {
    let a = moving_square(1).frames.remove(0);
    let src = VecFrameSource::new(Size::new(64.0, 48.0), 24.0, vec![a.clone(), a], ImageStore::new());
    let svg = export_animated_svg(&src, &AnimatedSvgOptions::default(), &mut no_progress).unwrap();
    assert!(!svg.contains("<animate"));
    assert_eq!(frame_groups(&parse(&svg)).len(), 1);
}

#[test]
fn animated_range_progress_cancel_and_errors() {
    let src = moving_square(10);
    let opts = AnimatedSvgOptions { frames: Some(2..5), ..AnimatedSvgOptions::default() };
    let mut reports = Vec::new();
    let svg = export_animated_svg(&src, &opts, &mut |p| {
        reports.push((p.done, p.total));
        true
    })
    .unwrap();
    assert_eq!(frame_groups(&parse(&svg)).len(), 3);
    assert_eq!(reports, [(0, 3), (1, 3), (2, 3), (3, 3)]);

    let mut n = 0;
    let err = export_animated_svg(&src, &AnimatedSvgOptions::default(), &mut |_| {
        n += 1;
        n < 4
    })
    .unwrap_err();
    assert!(matches!(err, ExportError::Cancelled));

    let none = AnimatedSvgOptions { frames: Some(5..5), ..AnimatedSvgOptions::default() };
    assert!(matches!(export_animated_svg(&src, &none, &mut no_progress), Err(ExportError::NoFrames)));
    let mut zero_fps = moving_square(2);
    zero_fps.fps = 0.0;
    assert!(matches!(export_animated_svg(&zero_fps, &AnimatedSvgOptions::default(), &mut no_progress), Err(ExportError::InvalidSetting(_))));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("anim.svg");
    export_animated_svg_file(&src, &path, &AnimatedSvgOptions::default(), &mut no_progress).unwrap();
    parse(&std::fs::read_to_string(path).unwrap());
}

#[test]
fn animated_frames_with_different_backgrounds_keep_their_own() {
    let mut a = DisplayList::new(Size::new(10.0, 10.0), Some(RED));
    a.nodes.push(solid_rect(0.0, 0.0, 2.0, 2.0, BLUE));
    let mut b = DisplayList::new(Size::new(10.0, 10.0), Some(GREEN));
    b.nodes.push(solid_rect(0.0, 0.0, 2.0, 2.0, BLUE));
    let src = VecFrameSource::new(Size::new(10.0, 10.0), 10.0, vec![a, b], ImageStore::new());
    let svg = export_animated_svg(&src, &AnimatedSvgOptions::default(), &mut no_progress).unwrap();
    let doc = parse(&svg);
    assert_eq!(doc.root_element().children().filter(|n| n.has_tag_name("rect")).count(), 0);
    assert!(svg.contains("#ff0000") && svg.contains("#00ff00"));
}

#[expect(clippy::disallowed_types, reason = "proptest's prop_oneof! keeps its strategies in an Arc")]
mod props {
    use super::*;
    use anim_display::{Filter, Stroke, StrokeStyle};
    use proptest::prelude::*;

    fn wild() -> impl Strategy<Value = f64> {
        prop_oneof![3 => -500.0..500.0f64, 1 => Just(f64::NAN), 1 => Just(f64::INFINITY), 1 => Just(f64::NEG_INFINITY), 1 => Just(1e300), 1 => Just(0.0)]
    }

    fn wild_affine() -> impl Strategy<Value = Affine> {
        prop::array::uniform6(wild()).prop_map(Affine::new)
    }

    fn node(depth: u32) -> BoxedStrategy<Node> {
        let leaf = prop_oneof![
            (wild_affine(), prop::collection::vec((wild(), wild()), 0..6)).prop_map(|(transform, pts)| {
                let mut path = anim_core::BezPath::new();
                for (i, (x, y)) in pts.into_iter().enumerate() {
                    if i == 0 {
                        path.move_to((x, y))
                    } else if i % 3 == 0 {
                        path.close_path()
                    } else {
                        path.line_to((x, y))
                    }
                }
                Node::Fill(Fill { transform, path, rule: FillRule::EvenOdd, paint: Paint::Solid(Rgba8::new(1, 2, 3, 200)) })
            }),
            (wild_affine(), wild(), prop::collection::vec(wild(), 0..4)).prop_map(|(transform, width, dashes)| Node::Stroke(Stroke {
                transform,
                path: rect_path(0.0, 0.0, 5.0, 5.0),
                style: StrokeStyle { width, dashes, ..StrokeStyle::default() },
                paint: Paint::Solid(Rgba8::BLACK),
            })),
        ];
        if depth == 0 {
            return leaf.boxed();
        }
        let inner = prop::collection::vec(node(depth - 1), 0..4);
        prop_oneof![
            2 => leaf,
            1 => (wild_affine(), inner, wild(), any::<bool>()).prop_map(|(transform, children, blur, masked)| Node::Group(Group {
                transform,
                filters: vec![Filter::Blur { x: blur, y: blur, quality: 3 }],
                mask: masked.then(|| vec![solid_rect(0.0, 0.0, 3.0, 3.0, RED)]),
                children,
                ..Group::default()
            })),
        ]
        .boxed()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn any_display_list_exports_to_wellformed_svg(nodes in prop::collection::vec(node(3), 0..6)) {
            let mut list = DisplayList::new(Size::new(100.0, 80.0), Some(Rgba8::WHITE));
            list.nodes = nodes;
            let svg = display_list_to_svg(&list, &ImageStore::new(), &SvgOptions::default()).unwrap();
            let doc = Document::parse(&svg).unwrap();
            assert_references_resolve(&doc);
            prop_assert!(!svg.contains("NaN"));
        }
    }
}
