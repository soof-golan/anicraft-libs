//! Artwork import benchmarks: the Adobe sample PSD/AI files when `ANICRAFT_SAMPLES` points at them, and synthetic
//! stress inputs (many layers, one huge masked layer, a wide group mask, an OCG-heavy PDF).
//!
//! With `BENCH_CHECKSUM=1` the benches print a checksum of every tree they build, to compare two runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::missing_const_for_fn,
    reason = "benchmark code over trusted local samples"
)]

use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::hint::black_box;
use std::path::PathBuf;

use anim_artwork::{ArtKind, ArtMask, ArtNode, ArtworkTree, Limits, import, import_ai, import_psd, to_display};
use anim_display::ImageStore;
use criterion::{Criterion, criterion_group, criterion_main};
use photocraft_psd::{ChannelData, Compression, GroupSpec, LayerMask, LayerSpec, MaskData, MaskSpec, PixelData, PsdBuilder, Rect as PsdRect};

/// Pseudo-random but smooth-ish RGBA so the RLE/ZIP paths of the writer have something to do.
fn noise(w: u32, h: u32, seed: u32) -> PixelData {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = (x / 4).wrapping_mul(31).wrapping_add((y / 4).wrapping_mul(17)).wrapping_add(seed.wrapping_mul(13));
            out.extend_from_slice(&[v as u8, (v >> 2) as u8, (v >> 4) as u8, if (x + y) % 97 < 90 { 255 } else { 128 }]);
        }
    }
    PixelData::Rgba8(out)
}

/// 600 layers of 128 x 128 in groups nested 4 deep, every third with a pixel mask, a clipping flag on every fifth.
fn many_layers_psd() -> Vec<u8> {
    let mut b = PsdBuilder::new(1024, 1024);
    for g in 0..30_u32 {
        b.begin_group(GroupSpec::new(format!("group {g}")));
        b.begin_group(GroupSpec::new(format!("inner {g}")));
        for l in 0..20_u32 {
            let i = g * 20 + l;
            let mut spec = LayerSpec::new(format!("layer {i}"), (i % 8 * 100) as i32, (i % 7 * 100) as i32, 128, 128, noise(128, 128, i));
            spec.opacity = 200;
            spec.clipping = i % 5 == 4;
            if i % 3 == 0 {
                let data: Vec<u8> = (0..128 * 128_u32).map(|p| (p % 251) as u8).collect();
                spec.mask = Some(MaskSpec { rect: PsdRect::from_xywh(0, 0, 128, 128), data, default_color: 0, disabled: false });
            }
            b.push_layer(spec);
        }
        b.end_group().unwrap();
        b.end_group().unwrap();
    }
    b.to_bytes().unwrap()
}

/// One 4096 x 3072 layer with a full-size pixel mask on a 4096 x 3072 canvas.
fn big_masked_layer_psd() -> Vec<u8> {
    let (w, h) = (4096_u32, 3072_u32);
    let mut b = PsdBuilder::new(w, h);
    let mut spec = LayerSpec::new("huge", 0, 0, w, h, noise(w, h, 7));
    let data: Vec<u8> = (0..w * h).map(|p| (p % 253) as u8).collect();
    spec.mask = Some(MaskSpec { rect: PsdRect::from_xywh(0, 0, w, h), data, default_color: 0, disabled: false });
    b.push_layer(spec);
    b.to_bytes().unwrap()
}

/// A group on a 4096 x 3072 canvas whose mask has a white background, forcing a whole-canvas mask image.
fn wide_group_mask_psd() -> Vec<u8> {
    let (w, h) = (4096_u32, 3072_u32);
    let mut b = PsdBuilder::new(w, h);
    b.begin_group(GroupSpec::new("G"));
    b.push_layer(LayerSpec::new("inside", 0, 0, 64, 64, noise(64, 64, 1)));
    b.end_group().unwrap();
    let mut file = b.build().unwrap();
    let version = file.header.version;
    let group = file.layers_mut().iter_mut().find(|r| r.name() == "G" && r.section_type() != photocraft_psd::SectionType::BoundingDivider).unwrap();
    group.mask = MaskData::Mask(LayerMask::new(PsdRect::from_xywh(100, 100, 64, 64), 255, 0));
    group.channels.push(ChannelData::encode(-2, Compression::Raw, &vec![128; 64 * 64], 64, 64, 8, version).unwrap());
    file.to_bytes().unwrap()
}

/// A plain PDF with 100 optional content groups, 100 filled rectangles each.
fn ocg_pdf() -> Vec<u8> {
    let layers = 100_usize;
    let mut content = String::new();
    let mut props = String::new();
    let mut all = String::new();
    for l in 0..layers {
        let _ = write!(props, "/L{l} {} 0 R ", 5 + l);
        let _ = write!(all, "{} 0 R ", 5 + l);
        let _ = writeln!(content, "/OC /L{l} BDC {} {} {} rg", (l % 3) as f32 / 2.0, (l % 5) as f32 / 4.0, (l % 7) as f32 / 6.0);
        for r in 0..100_usize {
            let _ = writeln!(content, "{} {} 8 8 re f", (r % 10) * 20 + l % 7, (r / 10) * 20 + l % 11);
        }
        content.push_str("EMC\n");
    }
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [{all}] /D << >> >> >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R /Resources << /Properties << {props} >> >> >>"),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    for l in 0..layers {
        objects.push(format!("<< /Type /OCG /Name (Layer {l}) >>"));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

fn hash_node(node: &ArtNode, h: &mut DefaultHasher) {
    node.name.hash(h);
    node.visible.hash(h);
    node.opacity.to_bits().hash(h);
    (node.blend as u8).hash(h);
    node.clipping.hash(h);
    if let Some(ArtMask::Raster { image, offset }) = &node.mask {
        image.premultiplied_rgba.hash(h);
        offset.x.to_bits().hash(h);
    }
    if let ArtKind::Raster { image, offset } = &node.kind {
        image.premultiplied_rgba.hash(h);
        offset.x.to_bits().hash(h);
        offset.y.to_bits().hash(h);
    }
    for c in node.children() {
        hash_node(c, h);
    }
}

fn checksum(name: &str, tree: &ArtworkTree) {
    if std::env::var_os("BENCH_CHECKSUM").is_some() {
        let mut h = DefaultHasher::new();
        hash_node(&tree.root, &mut h);
        tree.warnings.hash(&mut h);
        println!("checksum {name}: {:016x} ({} nodes)", h.finish(), tree.names().len());
    }
}

fn sample(rel: &str) -> Option<Vec<u8>> {
    let root = std::env::var_os("ANICRAFT_SAMPLES")?;
    std::fs::read(PathBuf::from(root).join(rel)).ok()
}

fn benches(c: &mut Criterion) {
    let limits = Limits::default();
    let mut group = c.benchmark_group("artwork");
    group.sample_size(10);

    let mut inputs: Vec<(&str, Vec<u8>)> =
        vec![("psd_600_layers", many_layers_psd()), ("psd_huge_masked_layer", big_masked_layer_psd()), ("psd_wide_group_mask", wide_group_mask_psd())];
    for (name, rel) in [
        ("sample_female_anchor_psd", "chproj_full/news anchors/Ch Media/Gathered Media/female_anchor.psd"),
        ("sample_male_anchor_psd", "chproj_full/news anchors/Ch Media/Gathered Media/male_anchor.psd"),
    ] {
        if let Some(bytes) = sample(rel) {
            inputs.push((name, bytes));
        }
    }
    for (name, bytes) in &inputs {
        checksum(name, &import_psd(bytes, &limits).unwrap());
        group.bench_function(format!("import_{name}"), |b| b.iter(|| black_box(import_psd(black_box(bytes), &limits).unwrap())));
    }
    if let Some((name, bytes)) = inputs.first() {
        let tree = import_psd(bytes, &limits).unwrap();
        group.bench_function(format!("to_display_{name}"), |b| b.iter(|| black_box(to_display(black_box(&tree), &mut ImageStore::new()))));
    }

    let pdf = ocg_pdf();
    checksum("pdf_100_ocg", &import_ai(&pdf, &limits).unwrap());
    group.bench_function("import_pdf_100_ocg", |b| b.iter(|| black_box(import_ai(black_box(&pdf), &limits).unwrap())));

    for (name, rel) in [
        ("sample_boy_ai", "extracted/Boy/Ch Media/Gathered Media/A boy character.ai"),
        ("sample_girl_ai", "extracted/blood_cells/Ch Media/Gathered Media/A girl character (1).ai"),
    ] {
        if let Some(bytes) = sample(rel) {
            checksum(name, &import(&bytes, &limits).unwrap());
            group.bench_function(format!("import_{name}"), |b| b.iter(|| black_box(import(black_box(&bytes), &limits).unwrap())));
            let tree = import(&bytes, &limits).unwrap();
            group.bench_function(format!("to_display_{name}"), |b| b.iter(|| black_box(to_display(black_box(&tree), &mut ImageStore::new()))));
        }
    }
    group.finish();
}

criterion_group!(artwork, benches);
criterion_main!(artwork);
