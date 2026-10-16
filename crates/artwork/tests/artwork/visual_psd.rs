//! Visual verification of the PSD importer on synthetic documents written with `photocraft-psd`.
//!
//! Each test builds a document aimed at one way the importer could be wrong, checks the layer tree, renders the
//! tree on white, and probes pixels against the predictions in `tests/fixtures/visual/<name>.expect.md`. Renders
//! are written to `$ANICRAFT_VISUAL_OUT/artwork` when that variable is set; the pixel assertions run either way.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::identity_op,
    clippy::many_single_char_names,
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "test code"
)]

use crate::common;

use anim_artwork::{ArtKind, ArtMask, ArtworkTree, Limits, import_psd};
use anim_core::{BlendMode, Point};
use common::{assert_px, golden, render_tree, save};
use photocraft_psd::{
    BlendMode as PsdBlend, ChannelData, ColorMode, Compression, GroupSpec, LayerMask, LayerSpec, MaskData, MaskParameters, MaskSpec, PixelData, PsdBuilder,
    PsdFile, Rect as PsdRect, TaggedBlock, Version,
};

const OUT: &str = "artwork";

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> PixelData {
    let n = (w * h) as usize;
    PixelData::Rgba8(rgba.iter().copied().cycle().take(n * 4).collect())
}

fn layer(name: &str, x: i32, y: i32, w: u32, h: u32, rgba: [u8; 4]) -> LayerSpec {
    LayerSpec::new(name, x, y, w, h, solid(w, h, rgba))
}

fn import(file: &PsdFile) -> ArtworkTree {
    let tree = import_psd(&file.to_bytes().unwrap(), &Limits::default()).unwrap();
    println!("{}", tree.outline());
    for w in &tree.warnings {
        println!("warning: {w}");
    }
    tree
}

fn group_spec(name: &str) -> GroupSpec {
    GroupSpec::new(name)
}

/// Sets a pixel mask on the group record called `name` (the builder has no group masks).
fn set_group_mask(file: &mut PsdFile, name: &str, rect: PsdRect, data: &[u8], default: u8) {
    let version = file.header.version;
    let (w, h) = (rect.width() as usize, rect.height() as usize);
    let record = file.layers_mut().iter_mut().find(|r| r.name() == name && r.section_type() != photocraft_psd::SectionType::BoundingDivider).unwrap();
    record.mask = MaskData::Mask(LayerMask::new(rect, default, 0));
    record.channels.push(ChannelData::encode(-2, Compression::Raw, data, w, h, 8, version).unwrap());
}

// ---------------------------------------------------------------------------------------------------------------

fn nested_visibility_builder(version: Version) -> PsdBuilder {
    let mut b = PsdBuilder::new(32, 24).version(version);
    b.push_layer(layer("Paper", 0, 0, 32, 24, [255, 255, 255, 255]));
    b.begin_group(group_spec("Body"));
    b.push_layer(layer("torso", 2, 2, 10, 10, [200, 30, 30, 255]));
    b.begin_group(group_spec("Arm"));
    b.push_layer(layer("arm-fill", 12, 2, 6, 10, [30, 160, 30, 255]));
    let mut ghost = group_spec("Ghost");
    ghost.visible = false;
    b.begin_group(ghost);
    b.push_layer(layer("ghost-px", 0, 14, 8, 8, [0, 0, 255, 255]));
    b.end_group().unwrap();
    let mut hidden = layer("arm-hidden", 20, 2, 6, 6, [255, 0, 255, 255]);
    hidden.visible = false;
    b.push_layer(hidden);
    b.end_group().unwrap();
    b.push_layer(LayerSpec::new("empty", 0, 0, 0, 0, PixelData::Rgba8(Vec::new())));
    b.push_layer(layer("offcanvas", -20, -20, 10, 10, [255, 255, 0, 255]));
    b.push_layer(layer("straddle", 28, 20, 10, 10, [0, 200, 200, 255]));
    b.end_group().unwrap();
    b.begin_group(group_spec("Deep"));
    b.begin_group(group_spec("Deeper"));
    b.begin_group(group_spec("Deepest"));
    b.push_layer(layer("leaf", 14, 14, 4, 4, [0, 0, 0, 255]));
    b.push_layer(layer("dup", 20, 14, 4, 4, [255, 128, 0, 255]));
    b.end_group().unwrap();
    b.push_layer(layer("dup", 20, 18, 4, 4, [100, 0, 150, 255]));
    b.end_group().unwrap();
    b.end_group().unwrap();
    b.begin_group(group_spec("Names"));
    for name in
        ["\u{540D}\u{524D}", "0123456789012345678901234567890123456789012345678901234567 (60)", "\u{1F642} smile", "", "Left Eye \u{05E2}\u{05D9}\u{05DF}"]
    {
        let mut l = layer(name, 0, 0, 1, 1, [0, 0, 0, 255]);
        l.visible = false;
        b.push_layer(l);
    }
    b.end_group().unwrap();
    b
}

fn check_nested_visibility_tree(tree: &ArtworkTree) {
    let names = |n: &anim_artwork::ArtNode| n.children().iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&tree.root), ["Paper", "Body", "Deep", "Names"]);
    let body = tree.find("Body").unwrap();
    assert_eq!(names(body), ["torso", "Arm", "empty", "offcanvas", "straddle"]);
    let arm = tree.find("Arm").unwrap();
    assert_eq!(names(arm), ["arm-fill", "Ghost", "arm-hidden"]);
    let ghost = tree.find("Ghost").unwrap();
    assert!(!ghost.visible);
    assert!(ghost.children().first().unwrap().visible, "the child keeps its own flag");
    assert!(!tree.find("arm-hidden").unwrap().visible);
    assert!(tree.find("torso").unwrap().visible);
    assert!(tree.find("Body").unwrap().visible);
    // The empty layer exists, but holds no pixels.
    let empty = tree.find("empty").unwrap();
    assert!(!matches!(empty.kind, ArtKind::Raster { .. }), "{empty:?}");
    assert!(empty.bounds().is_none());
    let ArtKind::Raster { offset, .. } = &tree.find("offcanvas").unwrap().kind else { panic!("raster expected") };
    assert_eq!(*offset, Point::new(-20.0, -20.0));
    let ArtKind::Raster { offset, image } = &tree.find("straddle").unwrap().kind else { panic!("raster expected") };
    assert_eq!((*offset, image.width, image.height), (Point::new(28.0, 20.0), 10, 10));
    // Duplicate names are both kept, in the right places.
    assert!(tree.root.find_path(&["Deep", "Deeper", "dup"]).is_some());
    assert!(tree.root.find_path(&["Deep", "Deeper", "Deepest", "dup"]).is_some());
    let names_group = tree.find("Names").unwrap();
    assert_eq!(
        names(names_group),
        ["\u{540D}\u{524D}", "0123456789012345678901234567890123456789012345678901234567 (60)", "\u{1F642} smile", "", "Left Eye \u{05E2}\u{05D9}\u{05DF}"]
    );
}

#[test]
fn nested_visibility() {
    let file = nested_visibility_builder(Version::Psd).build().unwrap();
    let tree = import(&file);
    check_nested_visibility_tree(&tree);
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "psd_nested_visibility", w, h, &rgba, 12);
    assert_px("torso", &rgba, w, (5, 5), [200, 30, 30], 1);
    assert_px("arm-fill", &rgba, w, (14, 5), [30, 160, 30], 1);
    assert_px("ghost child hidden by its group", &rgba, w, (3, 16), [255, 255, 255], 1);
    assert_px("hidden layer", &rgba, w, (22, 4), [255, 255, 255], 1);
    assert_px("off canvas layer", &rgba, w, (0, 0), [255, 255, 255], 1);
    assert_px("straddle inside", &rgba, w, (30, 22), [0, 200, 200], 1);
    assert_px("straddle left edge", &rgba, w, (28, 20), [0, 200, 200], 1);
    assert_px("left of straddle", &rgba, w, (27, 22), [255, 255, 255], 1);
    assert_px("leaf", &rgba, w, (15, 15), [0, 0, 0], 1);
    assert_px("orange dup", &rgba, w, (21, 15), [255, 128, 0], 1);
    assert_px("purple dup", &rgba, w, (21, 19), [100, 0, 150], 1);
    assert_px("last pixel", &rgba, w, (31, 23), [0, 200, 200], 1);
    golden("psd_nested_visibility", w, h, &rgba, 2);
}

#[test]
fn nested_visibility_as_psb() {
    let file = nested_visibility_builder(Version::Psb).build().unwrap();
    assert!(file.header.version.is_psb());
    let tree = import(&file);
    check_nested_visibility_tree(&tree);
    let (w, _, rgba) = render_tree(&tree);
    assert_px("psb leaf", &rgba, w, (15, 15), [0, 0, 0], 1);
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn opacity_and_fill_opacity() {
    let mut b = PsdBuilder::new(24, 16);
    let mut l50 = layer("L50", 0, 0, 8, 8, [0, 0, 0, 255]);
    l50.opacity = 128;
    b.push_layer(l50);
    let mut f50 = layer("F50", 8, 0, 8, 8, [0, 0, 0, 255]);
    f50.fill_opacity = Some(128);
    b.push_layer(f50);
    let mut both = layer("both", 16, 0, 8, 8, [0, 0, 0, 255]);
    both.opacity = 128;
    both.fill_opacity = Some(128);
    b.push_layer(both);
    let mut g = group_spec("G50");
    g.opacity = 128;
    b.begin_group(g);
    b.push_layer(layer("a", 0, 8, 8, 8, [255, 0, 0, 255]));
    b.push_layer(layer("b", 4, 8, 8, 8, [0, 0, 255, 255]));
    b.end_group().unwrap();
    let mut zero = layer("zero", 16, 8, 8, 8, [0, 0, 0, 255]);
    zero.opacity = 0;
    b.push_layer(zero);
    let tree = import(&b.build().unwrap());
    let near = |a: f32, b: f32| (a - b).abs() < 0.003;
    assert!(near(tree.find("L50").unwrap().opacity, 128.0 / 255.0));
    assert!(near(tree.find("F50").unwrap().opacity, 128.0 / 255.0));
    assert!(near(tree.find("both").unwrap().opacity, 0.25), "{}", tree.find("both").unwrap().opacity);
    assert!(near(tree.find("G50").unwrap().opacity, 128.0 / 255.0));
    assert!(near(tree.find("zero").unwrap().opacity, 0.0));
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "psd_opacity", w, h, &rgba, 16);
    assert_px("L50", &rgba, w, (3, 3), [127, 127, 127], 2);
    assert_px("F50", &rgba, w, (11, 3), [127, 127, 127], 2);
    assert_px("both", &rgba, w, (19, 3), [191, 191, 191], 3);
    assert_px("G50 a only", &rgba, w, (2, 12), [255, 127, 127], 2);
    assert_px("G50 overlap", &rgba, w, (6, 12), [127, 127, 255], 2);
    assert_px("G50 b only", &rgba, w, (10, 12), [127, 127, 255], 2);
    assert_px("zero", &rgba, w, (20, 12), [255, 255, 255], 1);
    assert_px("nothing", &rgba, w, (14, 12), [255, 255, 255], 1);
    golden("psd_opacity", w, h, &rgba, 2);
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn blend_modes_and_group_isolation() {
    let src = [100, 150, 200, 255];
    let mut b = PsdBuilder::new(32, 16);
    b.push_layer(layer("Base", 0, 0, 32, 16, [200, 100, 50, 255]));
    let modes = [
        PsdBlend::Multiply,
        PsdBlend::Screen,
        PsdBlend::Difference,
        PsdBlend::LinearDodge,
        PsdBlend::Overlay,
        PsdBlend::Darken,
        PsdBlend::Lighten,
        PsdBlend::Exclusion,
    ];
    for (i, mode) in modes.iter().enumerate() {
        let mut l = layer(&format!("mode{i}"), i32::try_from(i * 4).unwrap(), 0, 4, 8, src);
        l.blend_mode = *mode;
        b.push_layer(l);
    }
    // Normal group: isolated.
    let mut normal = group_spec("NormalGroup");
    normal.blend_mode = PsdBlend::Normal;
    b.begin_group(normal);
    let mut m = layer("mul-in-normal", 0, 8, 8, 8, src);
    m.blend_mode = PsdBlend::Multiply;
    b.push_layer(m);
    b.end_group().unwrap();
    // Pass through group.
    b.begin_group(group_spec("PassGroup"));
    let mut m = layer("mul-in-pass", 8, 8, 8, 8, src);
    m.blend_mode = PsdBlend::Multiply;
    b.push_layer(m);
    b.end_group().unwrap();
    // Multiply group.
    let mut mg = group_spec("MultiplyGroup");
    mg.blend_mode = PsdBlend::Multiply;
    b.begin_group(mg);
    b.push_layer(layer("plain-in-multiply", 16, 8, 8, 8, src));
    b.end_group().unwrap();
    // Pass through at half opacity.
    let mut half = group_spec("PassHalf");
    half.opacity = 128;
    b.begin_group(half);
    let mut m = layer("mul-in-half", 24, 8, 8, 8, src);
    m.blend_mode = PsdBlend::Multiply;
    b.push_layer(m);
    b.end_group().unwrap();

    let tree = import(&b.build().unwrap());
    // Photoshop's Normal group is isolated, and holds blend modes: it imports as an isolating `Layer` group.
    assert_eq!(tree.find("NormalGroup").unwrap().blend, BlendMode::Layer);
    assert_eq!(tree.find("PassGroup").unwrap().blend, BlendMode::PassThrough);
    assert_eq!(tree.find("MultiplyGroup").unwrap().blend, BlendMode::Multiply);
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "psd_blend_isolation", w, h, &rgba, 12);
    let want: [[u8; 3]; 8] = [[78, 59, 39], [222, 191, 211], [100, 50, 150], [255, 250, 250], [188, 118, 78], [100, 100, 50], [200, 150, 200], [143, 132, 172]];
    for (i, want) in want.iter().enumerate() {
        assert_px(&format!("mode {i} ({:?})", modes[i]), &rgba, w, (u32::try_from(i * 4 + 2).unwrap(), 4), *want, 3);
    }
    assert_px("multiply inside a Normal group is isolated", &rgba, w, (4, 12), [100, 150, 200], 3);
    assert_px("multiply inside pass through sees the base", &rgba, w, (12, 12), [78, 59, 39], 3);
    assert_px("multiply group", &rgba, w, (20, 12), [78, 59, 39], 3);
    // Known limitation (see the expect file): Photoshop gives lerp(base, multiply, 0.5) = (139,79,45); the display
    // model has no such group, so the group is drawn isolated: S at 50% over the base = (150,125,125). The importer says so.
    assert!(tree.warnings.iter().any(|w| w.contains("PassHalf") && w.contains("pass through")), "{:?}", tree.warnings);
    assert_px("pass through at half opacity (isolated approximation)", &rgba, w, (28, 12), [150, 125, 125], 4);
    golden("psd_blend_isolation", w, h, &rgba, 2);
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn clipping_masks() {
    let mut b = PsdBuilder::new(32, 16);
    // Left cluster: base is opaque only in its left half.
    let mut base_px = Vec::new();
    for _y in 0..12 {
        for x in 0..12 {
            base_px.extend_from_slice(&if x < 6 { [255, 0, 0, 255] } else { [255, 0, 0, 0] });
        }
    }
    b.push_layer(LayerSpec::new("base", 2, 2, 12, 12, PixelData::Rgba8(base_px)));
    let mut paint = layer("paint", 0, 0, 16, 16, [0, 0, 255, 255]);
    paint.clipping = true;
    b.push_layer(paint);
    let mut paint_b = layer("paint-b", 4, 4, 6, 6, [255, 255, 0, 255]);
    paint_b.clipping = true;
    paint_b.opacity = 128;
    b.push_layer(paint_b);
    b.push_layer(layer("free", 0, 13, 3, 3, [0, 0, 0, 255]));
    // Right cluster: translucent base.
    let mut base2 = layer("base2", 18, 2, 12, 12, [0, 160, 0, 255]);
    base2.opacity = 128;
    b.push_layer(base2);
    let mut paint2 = layer("paint2", 16, 0, 16, 16, [0, 0, 255, 255]);
    paint2.clipping = true;
    b.push_layer(paint2);

    let tree = import(&b.build().unwrap());
    for (name, clipped) in [("base", false), ("paint", true), ("paint-b", true), ("free", false), ("base2", false), ("paint2", true)] {
        assert_eq!(tree.find(name).unwrap().clipping, clipped, "{name}");
    }
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "psd_clipping", w, h, &rgba, 12);
    assert_px("inside the shape", &rgba, w, (3, 3), [0, 0, 255], 1);
    assert_px("yellow 50% over blue", &rgba, w, (5, 5), [127, 127, 127], 3);
    assert_px("transparent half of the base", &rgba, w, (8, 3), [255, 255, 255], 1);
    assert_px("paint-b cut at the shape edge", &rgba, w, (9, 5), [255, 255, 255], 1);
    assert_px("paint is cut to the base", &rgba, w, (1, 1), [255, 255, 255], 1);
    assert_px("free layer is not clipped", &rgba, w, (1, 14), [0, 0, 0], 1);
    assert_px("below the yellow square", &rgba, w, (5, 12), [0, 0, 255], 1);
    assert_px("base opacity applies to the whole clipping group", &rgba, w, (24, 8), [127, 127, 255], 3);
    assert_px("left of base2", &rgba, w, (17, 8), [255, 255, 255], 1);
    assert_px("right of base2", &rgba, w, (30, 8), [255, 255, 255], 1);
    golden("psd_clipping", w, h, &rgba, 2);
}

// ---------------------------------------------------------------------------------------------------------------

fn mask(rect: PsdRect, data: Vec<u8>, default_color: u8, disabled: bool) -> Option<MaskSpec> {
    Some(MaskSpec { rect, data, default_color, disabled })
}

#[test]
fn layer_masks() {
    let mut b = PsdBuilder::new(32, 24);
    let black = [0, 0, 0, 255];
    let mut ramp = layer("ramp", 0, 0, 16, 16, black);
    ramp.mask = mask(PsdRect::from_xywh(0, 0, 16, 16), (0..16_u32).flat_map(|_| (0..16_u32).map(|x| u8::try_from(x * 17).unwrap())).collect(), 0, false);
    b.push_layer(ramp);
    let mut partial = layer("partial", 16, 0, 16, 8, black);
    partial.mask = mask(PsdRect::from_xywh(20, 2, 4, 4), vec![255; 16], 0, false);
    b.push_layer(partial);
    let mut holey = layer("holey", 16, 8, 16, 8, black);
    holey.mask = mask(PsdRect::from_xywh(24, 10, 4, 4), vec![0; 16], 255, false);
    b.push_layer(holey);
    let mut disabled = layer("disabled", 0, 16, 8, 8, black);
    disabled.mask = mask(PsdRect::from_xywh(0, 16, 8, 8), vec![0; 64], 0, true);
    b.push_layer(disabled);
    let mut feather = layer("feather", 8, 16, 8, 8, black);
    feather.mask = mask(PsdRect::from_xywh(8, 16, 8, 8), (0..8_u32).flat_map(|_| (0..8_u32).map(|x| if x < 4 { 0 } else { 255 })).collect(), 0, false);
    b.push_layer(feather);
    let mut shifted = layer("shifted", 16, 16, 8, 8, black);
    shifted.mask = mask(PsdRect::from_xywh(14, 16, 8, 8), (0..8_u32).flat_map(|_| (0..8_u32).map(|x| if x < 4 { 0 } else { 255 })).collect(), 0, false);
    b.push_layer(shifted);
    let mut file = b.build().unwrap();
    // Feather and density parameters on the feather layer's mask.
    let record = file.layers_mut().iter_mut().find(|r| r.name() == "feather").unwrap();
    if let MaskData::Mask(m) = &mut record.mask {
        m.flags |= LayerMask::FLAG_PARAMETERS;
        m.parameters = Some(MaskParameters { flags: 0b11, user_density: Some(128), user_feather: Some(3.0), ..MaskParameters::default() });
    }
    let tree = import(&file);
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "psd_layer_mask", w, h, &rgba, 12);
    for (x, want) in [(0, 255), (3, 204), (8, 119), (15, 0)] {
        assert_px(&format!("ramp column {x}"), &rgba, w, (x, 5), [want; 3], 2);
    }
    assert_px("outside partial's mask", &rgba, w, (17, 1), [255; 3], 1);
    assert_px("inside partial's mask", &rgba, w, (21, 3), [0; 3], 1);
    assert_px("last mask pixel", &rgba, w, (23, 5), [0; 3], 1);
    assert_px("right of the mask rect", &rgba, w, (24, 3), [255; 3], 1);
    assert_px("above the mask rect", &rgba, w, (21, 1), [255; 3], 1);
    assert_px("holey default shows", &rgba, w, (16, 9), [0; 3], 1);
    assert_px("holey hole", &rgba, w, (25, 11), [255; 3], 1);
    assert_px("holey last hole pixel", &rgba, w, (27, 13), [255; 3], 1);
    assert_px("holey past the hole", &rgba, w, (28, 13), [0; 3], 1);
    assert_px("disabled mask ignored", &rgba, w, (3, 19), [0; 3], 1);
    for (x, want) in [(9, 255), (11, 255), (12, 0), (15, 0)] {
        assert_px(&format!("feather edge column {x}"), &rgba, w, (x, 19), [want; 3], 1);
    }
    for (x, want) in [(16, 255), (17, 255), (18, 0), (21, 0), (22, 255), (23, 255)] {
        assert_px(&format!("shifted mask column {x}"), &rgba, w, (x, 20), [want; 3], 1);
    }
    let ArtKind::Raster { image, .. } = &tree.find("partial").unwrap().kind else { panic!("raster expected") };
    assert_eq!((image.width, image.height), (16, 8));
    golden("psd_layer_mask", w, h, &rgba, 2);
}

// ---------------------------------------------------------------------------------------------------------------

fn rgb16(v: [u16; 4]) -> [u16; 4] {
    v
}

#[test]
fn colour_modes() {
    // 16-bit RGB.
    let samples = [
        rgb16([65535, 0, 0, 65535]),
        rgb16([0, 32768, 0, 65535]),
        rgb16([257, 513, 65278, 65535]),
        rgb16([0, 0, 0, 32768]),
        rgb16([65535, 65535, 65535, 0]),
        rgb16([43690, 43690, 43690, 65535]),
    ];
    let mut b = PsdBuilder::new(6, 2).depth(16);
    b.push_layer(LayerSpec::new("Deep", 0, 0, 6, 1, PixelData::Rgba16(samples.iter().flatten().copied().collect())));
    let tree = import(&b.build().unwrap());
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "psd_rgb16", w, h, &rgba, 24);
    assert_px("red", &rgba, w, (0, 0), [255, 0, 0], 0);
    assert_px("half green", &rgba, w, (1, 0), [0, 128, 0], 1);
    assert_px("rounded low/high", &rgba, w, (2, 0), [1, 2, 254], 1);
    assert_px("half alpha black", &rgba, w, (3, 0), [127, 127, 127], 1);
    assert_px("zero alpha", &rgba, w, (4, 0), [255, 255, 255], 0);
    assert_px("170 grey", &rgba, w, (5, 0), [170, 170, 170], 1);
    assert_eq!(rgba[((w + 0) * 4) as usize..][..3], [255, 255, 255], "row below the layer stays white");

    // Greyscale, 8 and 16 bit.
    let mut b = PsdBuilder::new(4, 2).color_mode(ColorMode::Grayscale);
    b.push_layer(LayerSpec::new("Grey", 0, 0, 4, 1, PixelData::GrayA8(vec![0, 255, 64, 255, 200, 255, 128, 128])));
    let (w, h, rgba) = render_tree(&import(&b.build().unwrap()));
    save(OUT, "psd_gray8", w, h, &rgba, 24);
    assert_px("black", &rgba, w, (0, 0), [0; 3], 0);
    assert_px("64", &rgba, w, (1, 0), [64; 3], 0);
    assert_px("200", &rgba, w, (2, 0), [200; 3], 0);
    assert_px("grey 128 at alpha 128", &rgba, w, (3, 0), [191; 3], 2);
    let mut b = PsdBuilder::new(4, 2).color_mode(ColorMode::Grayscale).depth(16);
    b.push_layer(LayerSpec::new("Grey16", 0, 0, 2, 1, PixelData::GrayA16(vec![65535, 65535, 16448, 65535])));
    let (w, _, rgba) = render_tree(&import(&b.build().unwrap()));
    assert_px("white 16", &rgba, w, (0, 0), [255; 3], 0);
    assert_px("64 in 16 bit", &rgba, w, (1, 0), [64; 3], 1);

    // CMYK, 8 and 16 bit: C, M, Y, K ink amounts and alpha. Paper, K full, K half, C, M, Y.
    let inks: [[u8; 5]; 6] = [[0, 0, 0, 0, 255], [0, 0, 0, 255, 255], [0, 0, 0, 128, 255], [255, 0, 0, 0, 255], [0, 255, 0, 0, 255], [0, 0, 255, 0, 255]];
    for deep in [false, true] {
        let mut b = PsdBuilder::new(6, 2).color_mode(ColorMode::Cmyk);
        let pixels = if deep {
            b = b.depth(16);
            PixelData::Cmyka16(inks.iter().flatten().map(|v| u16::from(*v) * 257).collect())
        } else {
            PixelData::Cmyka8(inks.iter().flatten().copied().collect())
        };
        b.push_layer(LayerSpec::new("Ink", 0, 0, 6, 1, pixels));
        let (w, h, rgba) = render_tree(&import(&b.build().unwrap()));
        save(OUT, if deep { "psd_cmyk16" } else { "psd_cmyk8" }, w, h, &rgba, 24);
        let at = |x| common::px(&rgba, w, x, 0);
        assert_eq!(&at(0)[..3], &[255, 255, 255], "paper");
        assert!(at(1)[..3].iter().all(|c| *c < 60), "K full {:?}", at(1));
        let k = at(2);
        assert!(k[..3].iter().all(|c| (90..=170).contains(c)) && k[0].abs_diff(k[1]) <= 4 && k[1].abs_diff(k[2]) <= 4, "K half {k:?}");
        let c = at(3);
        assert!(c[0] < 90 && c[1] > 150 && c[2] > 150, "cyan {c:?}");
        let m = at(4);
        assert!(m[1] < 90 && m[0] > 150 && m[2] > 150, "magenta {m:?}");
        let y = at(5);
        assert!(y[2] < 90 && y[0] > 150 && y[1] > 150, "yellow {y:?}");
        let _ = h;
    }
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn group_masks() {
    let mut b = PsdBuilder::new(32, 32);
    b.begin_group(group_spec("G1"));
    b.push_layer(layer("a", 0, 0, 16, 16, [255, 0, 0, 255]));
    b.push_layer(layer("b", 4, 4, 12, 12, [0, 0, 255, 255]));
    b.end_group().unwrap();
    b.begin_group(group_spec("G2"));
    b.push_layer(layer("c", 16, 0, 16, 16, [0, 160, 0, 255]));
    b.end_group().unwrap();
    b.begin_group(group_spec("Outer"));
    b.begin_group(group_spec("Inner"));
    b.push_layer(layer("d", 0, 16, 16, 16, [0, 0, 0, 255]));
    b.end_group().unwrap();
    b.end_group().unwrap();
    b.begin_group(group_spec("EmptyMask"));
    b.push_layer(layer("e", 16, 16, 16, 16, [255, 128, 0, 255]));
    b.end_group().unwrap();
    b.begin_group(group_spec("FullMask"));
    b.push_layer(layer("f", 0, 0, 2, 2, [0, 128, 128, 255]));
    b.end_group().unwrap();
    let mut file = b.build().unwrap();
    let half_cols: Vec<u8> = (0..16_u32).flat_map(|_| (0..16_u32).map(|x| if x < 8 { 255 } else { 0 })).collect();
    let half_rows: Vec<u8> = (0..16_u32).flat_map(|y| (0..16_u32).map(move |_| if y < 8 { 255 } else { 0 })).collect();
    set_group_mask(&mut file, "G1", PsdRect::from_xywh(0, 0, 16, 16), &half_cols, 0);
    set_group_mask(&mut file, "G2", PsdRect::from_xywh(20, 4, 4, 4), &[0; 16], 255);
    set_group_mask(&mut file, "Outer", PsdRect::from_xywh(0, 16, 16, 16), &half_cols, 0);
    set_group_mask(&mut file, "Inner", PsdRect::from_xywh(0, 16, 16, 16), &half_rows, 0);
    set_group_mask(&mut file, "EmptyMask", PsdRect::from_xywh(0, 0, 0, 0), &[], 0);
    set_group_mask(&mut file, "FullMask", PsdRect::from_xywh(0, 0, 0, 0), &[], 255);
    let tree = import(&file);
    assert!(tree.find("Outer").unwrap().mask.is_some());
    assert!(tree.find("Inner").unwrap().mask.is_some());
    let (w, h, rgba) = render_tree(&tree);
    save(OUT, "psd_group_masks", w, h, &rgba, 12);
    let white = [255; 3];
    assert_px("a", &rgba, w, (2, 2), [255, 0, 0], 1);
    assert_px("b", &rgba, w, (6, 6), [0, 0, 255], 1);
    assert_px("G1 cut", &rgba, w, (9, 2), white, 1);
    assert_px("G1 cuts b too", &rgba, w, (12, 12), white, 1);
    assert_px("c", &rgba, w, (17, 1), [0, 160, 0], 1);
    assert_px("G2 hole", &rgba, w, (21, 5), white, 1);
    assert_px("after the hole", &rgba, w, (24, 5), [0, 160, 0], 1);
    assert_px("far right", &rgba, w, (27, 1), [0, 160, 0], 1);
    assert_px("nested masks both pass", &rgba, w, (3, 18), [0; 3], 1);
    assert_px("outer cuts columns", &rgba, w, (10, 18), white, 1);
    assert_px("inner cuts rows", &rgba, w, (3, 26), white, 1);
    assert_px("empty mask hides the group", &rgba, w, (20, 20), white, 1);
    assert_px("full mask shows the group", &rgba, w, (1, 1), [0, 128, 128], 1);
    golden("psd_group_masks", w, h, &rgba, 2);
}

// ---------------------------------------------------------------------------------------------------------------

fn rect_subpath(x0: f64, y0: f64, x1: f64, y1: f64, operation: i16) -> photocraft_psd::path::PsdSubpath {
    use photocraft_psd::path::{PsdKnot, PsdSubpath};
    let knot = |x: f64, y: f64| PsdKnot { linked: false, pre: (x, y), anchor: (x, y), post: (x, y) };
    PsdSubpath { closed: true, operation, knots: vec![knot(x0, y0), knot(x1, y0), knot(x1, y1), knot(x0, y1)] }
}

fn vector_masked(name: &str, subpaths: &[photocraft_psd::path::PsdSubpath], flags: u32) -> LayerSpec {
    use photocraft_psd::path::{PathData, VectorMaskBlock};
    let block = VectorMaskBlock { version: 3, flags, path: PathData::from_subpaths(subpaths, false) };
    let mut l = layer(name, 0, 0, 24, 24, [0, 0, 0, 255]);
    l.extra_blocks.push(TaggedBlock::new(*b"vmsk", block.to_bytes()));
    l
}

#[test]
fn vector_masks() {
    let render_one = |name: &str, spec: LayerSpec| {
        let mut b = PsdBuilder::new(24, 24);
        b.push_layer(spec);
        let tree = import(&b.build().unwrap());
        let (w, h, rgba) = render_tree(&tree);
        save(OUT, name, w, h, &rgba, 16);
        (w, h, rgba)
    };
    let white = [255; 3];
    let (w, h, rgba) = render_one("psd_vector_mask_square", vector_masked("square", &[rect_subpath(0.25, 0.25, 0.75, 0.75, 1)], 0));
    assert_px("inside", &rgba, w, (12, 12), [0; 3], 2);
    assert_px("outside", &rgba, w, (3, 3), white, 1);
    for (x, y, want) in [(5, 12, 255), (6, 12, 0), (17, 12, 0), (18, 12, 255), (12, 5, 255), (12, 6, 0), (12, 17, 0), (12, 18, 255)] {
        assert_px(&format!("edge ({x},{y})"), &rgba, w, (x, y), [want; 3], 2);
    }
    golden("psd_vector_mask_square", w, h, &rgba, 3);
    let (w, _, rgba) =
        render_one("psd_vector_mask_donut", vector_masked("donut", &[rect_subpath(0.1, 0.1, 0.9, 0.9, 1), rect_subpath(0.4, 0.4, 0.6, 0.6, 2)], 0));
    assert_px("frame", &rgba, w, (4, 12), [0; 3], 2);
    assert_px("hole", &rgba, w, (12, 12), white, 2);
    assert_px("outside", &rgba, w, (1, 12), white, 2);
    let (w, _, rgba) = render_one("psd_vector_mask_inverted", vector_masked("inverted", &[rect_subpath(0.25, 0.25, 0.75, 0.75, 1)], 1));
    assert_px("inverted inside", &rgba, w, (12, 12), white, 2);
    assert_px("inverted outside", &rgba, w, (2, 2), [0; 3], 2);
    let (w, _, rgba) = render_one("psd_vector_mask_disabled", vector_masked("disabled", &[rect_subpath(0.25, 0.25, 0.75, 0.75, 1)], 4));
    assert_px("disabled outside", &rgba, w, (2, 2), [0; 3], 1);
    assert_px("disabled inside", &rgba, w, (12, 12), [0; 3], 1);
}

#[test]
fn group_mask_is_carried_as_raster_mask() {
    // Smoke: the ArtMask type for a PSD group mask.
    let mut b = PsdBuilder::new(8, 8);
    b.begin_group(group_spec("G"));
    b.push_layer(layer("x", 0, 0, 8, 8, [1, 2, 3, 255]));
    b.end_group().unwrap();
    let mut file = b.build().unwrap();
    set_group_mask(&mut file, "G", PsdRect::from_xywh(1, 1, 2, 2), &[255; 4], 0);
    let tree = import(&file);
    assert!(matches!(tree.find("G").unwrap().mask, Some(ArtMask::Raster { .. })));
}
