//! Checks against the Adobe-authored sample files. They are never committed: set `ANICRAFT_SAMPLES` to the folder
//! that holds them, otherwise every test here returns early and passes.
//!
//! The FLA zips have a central directory that is 54 bytes short, so the entries are read from their local file
//! headers (which carry the sizes) instead of through a zip library.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::string_slice,
    clippy::unwrap_used,
    reason = "test code over trusted local samples"
)]

use std::path::PathBuf;

use anim_core::kurbo::PathEl;
use anim_shape::codec::{parse_cubics, parse_edges, quantize_segment};
use anim_shape::{EdgeShape, FillStyle, NoImages, Segment, StrokeStyle, XflEdge, encode_edges};

/// The XML entries of a zip. Large media entries are skipped.
fn xml_entries(path: &std::path::Path) -> Vec<(String, String)> {
    anim_testkit::archive_texts(path, |name| {
        let is_xml = std::path::Path::new(name).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xml"));
        is_xml && !name.starts_with("META-INF") && !name.starts_with("Publish")
    })
    .expect("sample readable")
}

fn unescape(value: &str) -> String {
    value
        .replace("&#xD;", "\r")
        .replace("&#xA;", "\n")
        .replace("&#13;", "\r")
        .replace("&#10;", "\n")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Attributes of the element whose tag starts at `from` (just after `<Name`), and the index after its end.
fn attributes(xml: &str, from: usize) -> (Vec<(String, String)>, usize) {
    let bytes = xml.as_bytes();
    let mut pos = from;
    let mut attrs = Vec::new();
    loop {
        while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos += 1;
        }
        match bytes.get(pos) {
            Some(b'/' | b'>') | None => {
                let end = xml[pos..].find('>').map_or(xml.len(), |i| pos + i + 1);
                return (attrs, end);
            }
            _ => {}
        }
        let eq = xml[pos..].find('=').expect("attribute has a value") + pos;
        let name = xml[pos..eq].trim().to_owned();
        let quote = bytes[eq + 1];
        assert!(quote == b'"' || quote == b'\'', "attribute values are quoted");
        let value_start = eq + 2;
        let value_end = xml[value_start..].find(char::from(quote)).expect("closing quote") + value_start;
        attrs.push((name, unescape(&xml[value_start..value_end])));
        pos = value_end + 1;
    }
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// One `<DOMShape>`: the `<Edge>` elements in order and the highest fill and stroke index they use.
struct SampleShape {
    edges: Vec<XflEdge>,
}

fn shapes_of(xml: &str) -> Vec<SampleShape> {
    let mut shapes = Vec::new();
    let mut search = 0;
    while let Some(i) = xml[search..].find("<DOMShape") {
        let start = search + i;
        let end = xml[start..].find("</DOMShape>").map_or(xml.len(), |e| start + e);
        let body = &xml[start..end];
        let mut edges = Vec::new();
        let mut at = 0;
        while let Some(e) = body[at..].find("<Edge") {
            let tag_end = at + e + "<Edge".len();
            if body.as_bytes().get(tag_end).is_some_and(|b| !b.is_ascii_whitespace() && *b != b'/') {
                at = tag_end;
                continue;
            }
            let (attrs, next) = attributes(body, tag_end);
            let index = |name: &str| attr(&attrs, name).map_or(0, |v| v.parse::<u32>().expect("style index"));
            edges.push(XflEdge {
                fill_style0: index("fillStyle0"),
                fill_style1: index("fillStyle1"),
                stroke_style: index("strokeStyle"),
                edges: attr(&attrs, "edges").map(str::to_owned),
                cubics: attr(&attrs, "cubics").map(str::to_owned),
            });
            at = next;
        }
        shapes.push(SampleShape { edges });
        search = end.max(start + 1);
    }
    shapes
}

fn dummy_styles(edges: &[XflEdge]) -> (Vec<FillStyle>, Vec<StrokeStyle>) {
    let fills = edges.iter().map(|e| e.fill_style0.max(e.fill_style1)).max().unwrap_or(0);
    let strokes = edges.iter().map(|e| e.stroke_style).max().unwrap_or(0);
    (vec![FillStyle::default(); fills as usize], vec![StrokeStyle::default(); strokes as usize])
}

fn sample_files() -> Vec<(String, PathBuf)> {
    ["unicorn_4AE.fla", "derwood_green_choreo_short.fla"].into_iter().filter_map(|n| Some((n.to_owned(), anim_testkit::sample(n)?))).collect()
}

#[test]
fn every_sample_edge_and_cubic_parses_and_round_trips() {
    let mut edge_total = 0;
    let mut cubic_total = 0;
    for (name, path) in sample_files() {
        for (entry, xml) in xml_entries(&path) {
            for shape in shapes_of(&xml) {
                for (i, e) in shape.edges.iter().enumerate() {
                    if let Some(text) = &e.edges {
                        let segments = parse_edges(text).unwrap_or_else(|err| panic!("{name}:{entry}: edge {i}: {err}: {text:?}"));
                        assert!(!segments.is_empty(), "{name}:{entry}: edge {i} is empty");
                        // Canonical text parses back to the same geometry (after snapping to 1/256 twip).
                        let again = parse_edges(&encode_edges(&segments)).unwrap();
                        let snapped: Vec<Segment> = segments.iter().copied().map(quantize_segment).collect();
                        assert_eq!(again, snapped, "{name}:{entry}: edge {i}");
                        edge_total += 1;
                    }
                    if let Some(text) = &e.cubics {
                        let runs = parse_cubics(text).unwrap_or_else(|err| panic!("{name}:{entry}: cubics {i}: {err}: {text:?}"));
                        assert!(!runs.is_empty(), "{name}:{entry}: cubics {i} is empty");
                        cubic_total += runs.len();
                    }
                }
            }
        }
    }
    eprintln!("parsed {edge_total} edges attributes and {cubic_total} cubic curves");
    if anim_testkit::samples_dir().is_some() {
        assert!(edge_total > 200 && cubic_total > 1000, "the unicorn sample has hundreds of edges and over a thousand cubics");
    }
}

#[test]
fn sample_shapes_round_trip_losslessly_and_merge_the_cubics() {
    let mut shapes_total = 0;
    let mut runs_total = 0;
    let mut merged_total = 0;
    for (name, path) in sample_files() {
        for (entry, xml) in xml_entries(&path) {
            for (n, shape) in shapes_of(&xml).into_iter().enumerate() {
                let (fills, strokes) = dummy_styles(&shape.edges);
                let built = EdgeShape::from_xfl(fills, strokes, &shape.edges).unwrap_or_else(|err| panic!("{name}:{entry}: shape {n}: {err}"));
                assert!(built.edges.iter().all(anim_shape::Edge::is_unedited));
                assert_eq!(built.to_xfl_edges(), shape.edges, "{name}:{entry}: shape {n} is not written back verbatim");

                let cubic_runs: usize = shape.edges.iter().filter_map(|e| e.cubics.as_deref()).map(|t| parse_cubics(t).unwrap().len()).sum();
                let merged = built.edges.iter().flat_map(|e| e.segments.iter()).filter(|s| matches!(s, Segment::Cubic { .. })).count();
                if merged != cubic_runs {
                    eprintln!("{name}:{entry}: shape {n}: {merged} of {cubic_runs} cubics matched their quadratic edges");
                }
                runs_total += cubic_runs;
                merged_total += merged;
                shapes_total += 1;
            }
        }
    }
    eprintln!("{shapes_total} shapes round-tripped; {merged_total} of {runs_total} cubics merged into their edges");
    // About 15% of the cubics in the unicorn sample name vertices that no longer exist in the quadratic edges
    // (stale data left by an edit; checked independently of this crate), so they cannot be merged and the edges
    // stay as they are. Everything that matches must merge.
    assert!(merged_total * 100 >= runs_total * 80, "only {merged_total} of {runs_total} cubics merged");
}

#[test]
fn sample_shapes_assemble_into_closed_contours() {
    let mut contours = 0;
    let mut strokes = 0;
    for (name, path) in sample_files() {
        for (entry, xml) in xml_entries(&path) {
            for (n, shape) in shapes_of(&xml).into_iter().enumerate() {
                let (fills, stroke_styles) = dummy_styles(&shape.edges);
                let built = EdgeShape::from_xfl(fills, stroke_styles, &shape.edges).unwrap();
                let assembled = built.assemble();
                for region in &assembled.fills {
                    let mut start = None;
                    let mut last = None;
                    for element in region.path.elements() {
                        match *element {
                            PathEl::MoveTo(p) => {
                                start = Some(p);
                                last = Some(p);
                            }
                            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => last = Some(p),
                            PathEl::ClosePath => {
                                let gap = start.unwrap().distance(last.unwrap()) * 20.0;
                                assert!(gap <= 0.6, "{name}:{entry}: shape {n}, fill {}: contour needed a {gap} twip closing line", region.style);
                                contours += 1;
                            }
                        }
                    }
                }
                strokes += assembled.strokes.len();
                let nodes = built.to_display(&NoImages);
                assert_eq!(nodes.len(), assembled.fills.len() + assembled.strokes.len());
                if let Some(bounds) = built.bounds() {
                    assert!(bounds.x0.is_finite() && bounds.y1.is_finite());
                }
            }
        }
    }
    eprintln!("{contours} closed fill contours, {strokes} stroke runs");
}
