//! Visual verification of the AI/PDF importer on hand-written PDFs with optional content groups (layers).
//!
//! The files are built as text here (nothing from Adobe). Predictions are in `tests/fixtures/visual/pdf_*.expect.md`.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::too_many_arguments,
    reason = "test code"
)]

mod common;

use std::fmt::Write as _;

use anim_artwork::{ArtworkTree, Limits, import_ai};
use common::{assemble_pdf, assert_px, golden, render_tree, save, stream_body, utf16_hex};

const OUT: &str = "artwork";

struct Spec<'a> {
    width: u32,
    height: u32,
    /// `(property key, PDF string literal)` per layer, in object order.
    layers: &'a [(&'a str, String)],
    /// Entries of the `/D` dictionary; `{key}` becomes the layer's reference.
    config: &'a str,
    content: &'a str,
    ext_gstate: &'a str,
    /// Extra objects numbered after the layers; `{first}` in the page resources is replaced by nothing, callers
    /// compute numbers with [`Spec::first_extra`].
    extra: Vec<String>,
    properties_extra: &'a str,
}

impl Spec<'_> {
    const fn first_extra(&self) -> usize {
        5 + self.layers.len()
    }

    fn build(&self) -> Vec<u8> {
        let refs = |text: &str| {
            let mut out = text.to_owned();
            for (i, (key, _)) in self.layers.iter().enumerate() {
                out = out.replace(&format!("{{{key}}}"), &format!("{} 0 R", 5 + i));
            }
            out
        };
        let mut all = String::new();
        let mut props = String::new();
        for (i, (key, _)) in self.layers.iter().enumerate() {
            let _ = write!(all, "{} 0 R ", 5 + i);
            let _ = write!(props, "/{key} {} 0 R ", 5 + i);
        }
        let mut objects = vec![
            format!("<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [{all}] /D << {} >> >> >>", refs(self.config)),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << /Properties << {props}{} >> /ExtGState << {} >> >> >>",
                self.width,
                self.height,
                refs(self.properties_extra),
                self.ext_gstate
            ),
            stream_body("", self.content),
        ];
        for (_, name) in self.layers {
            objects.push(format!("<< /Type /OCG /Name {name} >>"));
        }
        objects.extend(self.extra.iter().map(|e| refs(e)));
        assemble_pdf(&objects)
    }
}

fn import(bytes: &[u8]) -> ArtworkTree {
    let tree = import_ai(bytes, &Limits::default()).unwrap();
    println!("{}", tree.outline());
    for w in &tree.warnings {
        println!("warning: {w}");
    }
    tree
}

fn lit(name: &str) -> String {
    format!("({name})")
}

// ---------------------------------------------------------------------------------------------------------------

const MID_NAME: &str = "Mid \u{2713} \u{05E2}\u{05D1}\u{05E8}\u{05D9}\u{05EA}";

fn ocg_layers_pdf() -> Vec<u8> {
    let layers = [("L1", lit("Background")), ("L2", utf16_hex(MID_NAME)), ("L3", lit("Hidden")), ("L4", lit("Top")), ("L5", lit("Sub"))];
    let content = "\
/OC /L1 BDC 1 0.9 0.8 rg 0 0 100 100 re f EMC
/OC /L2 BDC 0 0 1 rg 10 10 40 40 re f EMC
/OC /L3 BDC 0 1 0 rg 30 30 40 40 re f EMC
/OC /L4 BDC 1 0 0 rg 60 60 30 30 re f
/OC /L5 BDC 1 0 1 rg 70 70 10 10 re f EMC
/Span << /MCID 0 >> BDC 0 0 0 rg 62 62 4 4 re f EMC
EMC
/OC /L2 BDC 1 1 0 rg 10 60 10 10 re f EMC
";
    Spec {
        width: 100,
        height: 100,
        layers: &layers,
        config: "/Order [{L1} {L2} {L3} {L4} [{L5}]] /OFF [{L3}]",
        content,
        ext_gstate: "",
        extra: vec![],
        properties_extra: "",
    }
    .build()
}

#[test]
fn pdf_ocg_layers() {
    let bytes = ocg_layers_pdf();
    let tree = import(&bytes);
    assert_eq!((tree.size.width, tree.size.height), (100.0, 100.0));
    for name in ["Background", MID_NAME, "Hidden", "Top", "Sub"] {
        assert!(tree.find(name).is_some(), "layer {name:?} missing from\n{}", tree.outline());
    }
    assert!(!tree.find("Hidden").unwrap().visible, "{}", tree.outline());
    for name in ["Background", MID_NAME, "Top"] {
        assert!(tree.find(name).unwrap().visible, "{name}");
    }
    let mid_count = tree.root.names().iter().filter(|n| **n == MID_NAME).count();
    assert_eq!(mid_count, 1, "one layer per OCG\n{}", tree.outline());
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "pdf_ocg_layers", w, h, &rgba, 6);
    assert_px("background", &rgba, w, (5, 5), [255, 230, 204], 2);
    assert_px("blue", &rgba, w, (20, 80), [0, 0, 255], 1);
    assert_px("hidden green does not cover blue", &rgba, w, (40, 60), [0, 0, 255], 1);
    assert_px("hidden green does not cover the cream", &rgba, w, (60, 40), [255, 230, 204], 2);
    assert_px("red", &rgba, w, (85, 20), [255, 0, 0], 1);
    assert_px("sub layer", &rgba, w, (75, 25), [255, 0, 255], 1);
    assert_px("plain marked content", &rgba, w, (63, 36), [0, 0, 0], 1);
    assert_px("mid, second part", &rgba, w, (15, 35), [255, 255, 0], 1);
    assert_px("cream corner", &rgba, w, (95, 95), [255, 230, 204], 2);
    golden("pdf_ocg_layers", w, h, &rgba, 3);
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn pdf_shapes() {
    let layers = [("L1", lit("Shapes"))];
    let content = "\
/OC /L1 BDC
0 0 1 rg 10 10 60 60 re 25 25 30 30 re f*
1 0 0 rg 100 10 60 60 re 115 25 30 30 re f
q 170 10 20 30 re W n 0 0.5 0 rg 165 5 30 80 re f Q
q 0.7071 0.7071 -0.7071 0.7071 150 70 cm 0 0.6 0.6 rg 0 0 20 10 re f Q
0 0 0 RG 4 w [6 6] 0 d 10 90 m 90 90 l S
EMC
";
    let bytes = Spec { width: 200, height: 100, layers: &layers, config: "", content, ext_gstate: "", extra: vec![], properties_extra: "" }.build();
    let tree = import(&bytes);
    assert!(tree.find("Shapes").is_some(), "{}", tree.outline());
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "pdf_shapes", w, h, &rgba, 5);
    let white = [255; 3];
    assert_px("ring side", &rgba, w, (15, 60), [0, 0, 255], 1);
    assert_px("even-odd hole", &rgba, w, (40, 60), white, 1);
    assert_px("ring top band", &rgba, w, (40, 35), [0, 0, 255], 1);
    assert_px("non-zero keeps the inside", &rgba, w, (130, 60), [255, 0, 0], 1);
    assert_px("red band", &rgba, w, (105, 35), [255, 0, 0], 1);
    assert_px("clipped green", &rgba, w, (180, 75), [0, 128, 0], 2);
    assert_px("above the clip", &rgba, w, (180, 50), white, 1);
    assert_px("left of the clip", &rgba, w, (166, 75), white, 1);
    assert_px("right of the clip", &rgba, w, (192, 75), white, 1);
    assert_px("rotated bar", &rgba, w, (153, 20), [0, 153, 153], 2);
    assert_px("outside the rotated bar", &rgba, w, (145, 12), white, 1);
    assert_px("first dash", &rgba, w, (13, 10), [0; 3], 2);
    assert_px("gap", &rgba, w, (19, 10), white, 2);
    assert_px("second dash", &rgba, w, (25, 10), [0; 3], 2);
    assert_px("second gap", &rgba, w, (31, 10), white, 2);
    assert_px("outside the stroke width", &rgba, w, (13, 5), white, 1);
    golden("pdf_shapes", w, h, &rgba, 3);
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn pdf_transparency() {
    let layers = [("L1", lit("Back")), ("L2", lit("Alpha")), ("L3", lit("Mul"))];
    let content = "\
/OC /L1 BDC 1 0.9 0.8 rg 0 0 100 50 re f 0 1 1 rg 50 0 50 50 re f EMC
/OC /L2 BDC q /GS1 gs 1 0 0 rg 10 10 30 30 re f Q EMC
/OC /L3 BDC q /GS2 gs 1 1 0 rg 60 10 30 30 re f Q EMC
";
    let bytes = Spec {
        width: 100,
        height: 50,
        layers: &layers,
        config: "",
        content,
        ext_gstate: "/GS1 << /ca 0.5 >> /GS2 << /BM /Multiply >>",
        extra: vec![],
        properties_extra: "",
    }
    .build();
    let tree = import(&bytes);
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "pdf_transparency", w, h, &rgba, 8);
    assert_px("cream", &rgba, w, (5, 5), [255, 230, 204], 2);
    assert_px("50% red over cream", &rgba, w, (20, 20), [255, 115, 102], 3);
    assert_px("yellow multiply cyan", &rgba, w, (75, 25), [0, 255, 0], 2);
    assert_px("cyan", &rgba, w, (95, 45), [0, 255, 255], 1);
    assert_px("cyan left", &rgba, w, (55, 45), [0, 255, 255], 1);
    assert_px("cream right of red", &rgba, w, (45, 25), [255, 230, 204], 2);
    golden("pdf_transparency", w, h, &rgba, 3);
}

// ---------------------------------------------------------------------------------------------------------------

fn states_pdf(config: &str) -> Vec<u8> {
    let layers = [("L1", lit("A")), ("L2", lit("B")), ("L3", lit("C"))];
    let content = "\
/OC /L1 BDC 1 0 0 rg 0 35 30 30 re f EMC
/OC /L2 BDC 0 1 0 rg 35 35 30 30 re f EMC
/OC /L3 BDC 0 0 1 rg 70 35 30 30 re f EMC
/OC /M1 BDC 1 0.75 0.8 rg 35 0 30 30 re f EMC
";
    let spec = Spec { width: 100, height: 100, layers: &layers, config, content, ext_gstate: "", extra: vec![], properties_extra: "/M1 8 0 R" };
    let mut spec = spec;
    spec.extra = vec!["<< /Type /OCMD /OCGs [{L2} {L3}] /P /AllOn >>".to_owned()];
    assert_eq!(spec.first_extra(), 8);
    spec.build()
}

#[test]
fn pdf_ocg_states() {
    let white = [255; 3];
    let cases: [(&str, &str, [[u8; 3]; 4]); 3] = [
        ("base_off", "/BaseState /OFF /ON [{L1}]", [[255, 0, 0], white, white, white]),
        ("two_off", "/OFF [{L1} {L2}]", [white, white, [0, 0, 255], white]),
        ("all_on", "", [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 192, 203]]),
    ];
    for (name, config, want) in cases {
        let tree = import(&states_pdf(config));
        let (w, h, rgba) = render_tree(&tree);
        save(OUT, &format!("pdf_ocg_states_{name}"), w, h, &rgba, 5);
        for (what, at, want) in [("A", (15, 50), want[0]), ("B", (50, 50), want[1]), ("C", (85, 50), want[2]), ("M1", (50, 85), want[3])] {
            assert_px(&format!("{name}: {what}"), &rgba, w, at, want, 1);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn hostile_layer_structures_do_not_panic() {
    let layers = [("L1", lit("A")), ("L2", lit("B"))];
    let deep: String = "/OC /L1 BDC ".repeat(5000) + "0 0 1 rg 0 0 10 10 re f" + &" EMC".repeat(5000);
    let cases: Vec<(&str, String, String)> = vec![
        ("order references a missing group", "/Order [{L1} 99 0 R [{L2} 98 0 R]]".to_owned(), "/OC /L1 BDC 1 0 0 rg 0 0 5 5 re f EMC".to_owned()),
        ("self-referencing order", "/Order 7 0 R".to_owned(), "/OC /L1 BDC 1 0 0 rg 0 0 5 5 re f EMC".to_owned()),
        ("unbalanced EMC", String::new(), "EMC EMC EMC /OC /L1 BDC 1 0 0 rg 0 0 5 5 re f".to_owned()),
        ("missing property", String::new(), "/OC /Nope BDC 1 0 0 rg 0 0 5 5 re f EMC".to_owned()),
        ("5000 nested marked content", String::new(), deep),
        (
            "all off with base state off and nothing on",
            "/BaseState /OFF".to_owned(),
            "/OC /L1 BDC 1 0 0 rg 0 0 5 5 re f EMC /OC /L2 BDC 0 1 0 rg 5 5 5 5 re f EMC".to_owned(),
        ),
    ];
    for (what, config, content) in cases {
        let mut spec = Spec { width: 20, height: 20, layers: &layers, config: &config, content: &content, ext_gstate: "", extra: vec![], properties_extra: "" };
        // Objects 7 and 8 are arrays that contain each other.
        spec.extra = vec!["[ 8 0 R {L1} ]".to_owned(), "[ 7 0 R {L2} ]".to_owned()];
        let bytes = spec.build();
        let result = import_ai(&bytes, &Limits::default());
        println!("{what}: {:?}", result.as_ref().map(anim_artwork::ArtworkTree::outline));
        if let Ok(tree) = result {
            let _ = render_tree(&tree);
        }
    }
}
