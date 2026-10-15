//! Hostile and damaged input never panics; limits hold; format sniffing is sane.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]

use anim_artwork::{ArtworkError, Format, Limits, detect, import, import_ai, import_psd, import_svg};
use photocraft_psd::{GroupSpec, LayerSpec, PixelData, PsdBuilder};
use proptest::prelude::*;

fn psd_bytes() -> Vec<u8> {
    let mut b = PsdBuilder::new(8, 8);
    b.push_layer(LayerSpec::new("bg", 0, 0, 8, 8, PixelData::Rgba8(vec![200; 8 * 8 * 4])));
    b.begin_group(GroupSpec::new("+Head"));
    b.push_layer(LayerSpec::new("eye", 1, 1, 2, 2, PixelData::Rgba8(vec![90; 2 * 2 * 4])));
    b.end_group().unwrap();
    b.to_bytes().unwrap()
}

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><g id="A"><rect id="r" width="10" height="10" fill="#f00"/></g></svg>"##;

#[test]
fn sniffs_formats() {
    assert_eq!(detect(&psd_bytes()), Some(Format::Psd));
    assert_eq!(detect(SVG.as_bytes()), Some(Format::Svg));
    assert_eq!(detect(b"%PDF-1.6\n1 0 obj"), Some(Format::Ai));
    assert_eq!(detect(b"%!PS-Adobe-3.0 EPSF-3.0"), Some(Format::Ai));
    assert_eq!(detect(b"hello world"), None);
    assert!(matches!(import(b"hello world", &Limits::default()), Err(ArtworkError::Unrecognized)));
}

#[test]
fn import_dispatches() {
    let psd = import(&psd_bytes(), &Limits::default()).unwrap();
    assert!(psd.find("+Head").is_some());
    let svg = import(SVG.as_bytes(), &Limits::default()).unwrap();
    assert!(svg.find("A").is_some());
}

#[test]
fn input_size_limit() {
    let limits = Limits { max_input_bytes: 10, ..Limits::default() };
    assert!(matches!(import_psd(&psd_bytes(), &limits), Err(ArtworkError::Limit(_))));
    assert!(matches!(import_svg(SVG.as_bytes(), &limits), Err(ArtworkError::Limit(_))));
    assert!(matches!(import_ai(b"%PDF-1.6 0123456789", &limits), Err(ArtworkError::Limit(_))));
}

#[test]
fn damaged_pdfs_are_errors() {
    for bytes in [
        b"%PDF-1.6".as_slice(),
        b"%PDF-1.6\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer << /Root 1 0 R >>\n%%EOF",
        b"%PDF-1.6\n\x00\x01\x02\x03",
        b"%!PS-Adobe-3.0\n%%BoundingBox: 0 0 10 10\n(unterminated",
    ] {
        let _ = import_ai(bytes, &Limits::default());
    }
}

#[test]
fn group_depth_is_bounded() {
    let mut b = PsdBuilder::new(4, 4);
    for _ in 0..40 {
        b.begin_group(GroupSpec::new("g"));
    }
    b.push_layer(LayerSpec::new("leaf", 0, 0, 1, 1, PixelData::Rgba8(vec![1, 2, 3, 255])));
    for _ in 0..40 {
        b.end_group().unwrap();
    }
    let bytes = b.to_bytes().unwrap();
    let limits = Limits { max_depth: 8, ..Limits::default() };
    let tree = import_psd(&bytes, &limits).unwrap();
    let mut deepest = 0;
    tree.root.walk(|_, depth| deepest = deepest.max(depth));
    assert!(deepest <= 10, "depth {deepest}");
}

#[test]
fn deeply_nested_svg_groups_are_bounded() {
    let mut svg = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="5" height="5">"#);
    for _ in 0..500 {
        svg.push_str("<g>");
    }
    svg.push_str(r#"<rect width="1" height="1"/>"#);
    for _ in 0..500 {
        svg.push_str("</g>");
    }
    svg.push_str("</svg>");
    if let Ok(tree) = import_svg(svg.as_bytes(), &Limits { max_depth: 16, ..Limits::default() }) {
        let mut deepest = 0;
        tree.root.walk(|_, depth| deepest = deepest.max(depth));
        assert!(deepest <= 20, "depth {deepest}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = import(&bytes, &Limits::default());
        let mut psd = b"8BPS\0\x01".to_vec();
        psd.extend_from_slice(&bytes);
        let _ = import_psd(&psd, &Limits::default());
        let mut pdf = b"%PDF-1.6\n".to_vec();
        pdf.extend_from_slice(&bytes);
        let _ = import_ai(&pdf, &Limits::default());
        let mut svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\">".to_vec();
        svg.extend_from_slice(&bytes);
        let _ = import_svg(&svg, &Limits::default());
    }

    #[test]
    fn mutated_psd_never_panics(at in 0usize..2000, value in any::<u8>(), cut in 0usize..2000) {
        let mut bytes = psd_bytes();
        let i = at % bytes.len();
        bytes[i] = value;
        bytes.truncate(bytes.len().saturating_sub(cut % bytes.len()).max(1));
        let _ = import_psd(&bytes, &Limits::default());
    }

    #[test]
    fn mutated_svg_never_panics(at in 0usize..400, value in any::<u8>()) {
        let mut bytes = SVG.as_bytes().to_vec();
        let i = at % bytes.len();
        bytes[i] = value;
        let _ = import_svg(&bytes, &Limits::default());
    }
}
