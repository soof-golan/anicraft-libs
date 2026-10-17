//! Tests on Adobe-authored sample files. They read `ANICRAFT_SAMPLES` and pass trivially when it is unset.
//!
//! With `ANICRAFT_SCRATCH` set, they also render each sample to a PNG there for visual review.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]

use std::path::Path;

use anim_artwork::{ArtworkTree, Limits, import_path, to_display};
use anim_display::ImageStore;
use anim_render::{Background, RenderOptions, render};
use anim_testkit::sample;

fn load(rel: &str) -> Option<ArtworkTree> {
    let path = sample(rel)?;
    let tree = import_path(&path, &Limits::default()).unwrap();
    println!("{rel}\n{}", tree.outline());
    for w in &tree.warnings {
        println!("warning: {w}");
    }
    Some(tree)
}

/// Renders `tree` on white and writes `<scratch>/<name>.png` when `ANICRAFT_SCRATCH` is set.
fn render_to_scratch(tree: &ArtworkTree, name: &str) -> Vec<u8> {
    let mut store = ImageStore::new();
    let list = to_display(tree, &mut store);
    let mut options = RenderOptions::for_stage(tree.size);
    options.background = Background::Solid(anim_core::Rgba8::WHITE);
    let image = render(&list, &store, &options).unwrap();
    if let Some(dir) = std::env::var_os("ANICRAFT_SCRATCH") {
        write_png(&Path::new(&dir).join(format!("{name}.png")), image.width, image.height, &image.premultiplied_rgba);
    }
    image.premultiplied_rgba
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    anim_testkit::write_png(path, width, height, rgba).unwrap();
}

#[test]
fn boy_ai_layer_tree() {
    let Some(tree) = load("extracted/Boy/Ch Media/Gathered Media/A boy character.ai") else { return };
    assert!(tree.size.width > 100.0 && tree.size.height > 100.0, "{:?}", tree.size);
    for name in ["Ch", "Body", "Head", "Face", "mouth", "torso", "neck"] {
        assert!(tree.find(name).is_some(), "layer {name:?} missing from\n{}", tree.outline());
    }
    // Sublayers nest under their parents.
    assert!(tree.root.find_path(&["Ch", "Head", "mouth"]).is_some());
    assert!(tree.root.find_path(&["Ch", "Body", "torso"]).is_some());
    // The visemes are hidden sublayers of the mouth, the neutral mouth is shown.
    let mouth = tree.find("mouth").unwrap();
    assert!(mouth.children().iter().any(|c| !c.visible), "hidden viseme layers expected");
    assert!(mouth.children().iter().any(|c| c.visible));
    let pixels = render_to_scratch(&tree, "boy_ai");
    assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[0] != 255), "render is blank");
}

#[test]
fn girl_ai_layer_tree() {
    let Some(tree) = load("extracted/blood_cells/Ch Media/Gathered Media/A girl character (1).ai") else { return };
    for name in ["Head", "mouth", "Left eye", "Right eye", "Nose", "+hair", "Smile", "Surprised", "Aa", "neutral", "blink"] {
        assert!(tree.find(name).is_some(), "layer {name:?} missing from\n{}", tree.outline());
    }
    assert!(tree.find("Left eye").unwrap().children().len() >= 3);
    let pixels = render_to_scratch(&tree, "girl_ai");
    assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[0] != 255), "render is blank");
}

/// A grid of every mouth shape of the girl character, each rendered on its own, for visual review.
#[test]
fn mouth_shapes_contact_sheet() {
    let Some(tree) = load("extracted/blood_cells/Ch Media/Gathered Media/A girl character (1).ai") else { return };
    let mouth_path = ["Ch", "Head", "mouth"];
    let Some(mouth) = tree.root.find_path(&mouth_path) else { return };
    let bounds = mouth.bounds().unwrap();
    let names: Vec<String> = mouth.children().iter().map(|c| c.name.clone()).collect();
    println!("mouth shapes: {names:?} within {bounds:?}");
    assert!(names.len() >= 10);

    let (cw, ch, columns) = (240_usize, 160_usize, 4_usize);
    let (cell_w, cell_h) = (u32::try_from(cw).unwrap(), u32::try_from(ch).unwrap());
    let rows = names.len().div_ceil(columns);
    let (sheet_w, sheet_h) = (cw * columns, ch * rows);
    let mut sheet = vec![255_u8; sheet_w * sheet_h * 4];
    for (i, _) in names.iter().enumerate() {
        let mut variant = tree.clone();
        let m = variant.root.find_path_mut(&mouth_path).unwrap();
        let anim_artwork::ArtKind::Group(children) = &mut m.kind else { panic!("mouth is a group") };
        for (j, child) in children.iter_mut().enumerate() {
            child.visible = i == j;
        }
        // Only the head's mouth is drawn: hide everything else under the layer.
        let mut store = ImageStore::new();
        let mut list = to_display(&variant, &mut store);
        list.background = None;
        let size = anim_core::Size::new(bounds.width() + 20.0, bounds.height() + 20.0);
        let mut options = RenderOptions::fit_stage(size, cell_w, cell_h);
        options.view *= anim_core::Affine::translate((10.0 - bounds.x0, 10.0 - bounds.y0));
        options.background = Background::Solid(anim_core::Rgba8::WHITE);
        let cell = render(&list, &store, &options).unwrap();
        let (cx, cy) = ((i % columns) * cw, (i / columns) * ch);
        for y in 0..ch {
            let src = y * cw * 4;
            let dst = ((cy + y) * sheet_w + cx) * 4;
            sheet[dst..dst + cw * 4].copy_from_slice(&cell.premultiplied_rgba[src..src + cw * 4]);
        }
    }
    if let Some(dir) = std::env::var_os("ANICRAFT_SCRATCH") {
        write_png(&Path::new(&dir).join("girl_mouth_shapes.png"), u32::try_from(sheet_w).unwrap(), u32::try_from(sheet_h).unwrap(), &sheet);
    }
}

#[test]
fn imports_are_deterministic() {
    let Some(a) = load("extracted/Boy/Ch Media/Gathered Media/A boy character.ai") else { return };
    let b = import_path(&sample("extracted/Boy/Ch Media/Gathered Media/A boy character.ai").unwrap(), &Limits::default()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn puppet_psd_in_chproj_when_present() {
    // The news-anchors project keeps its gathered media as PSDs; import every one.
    let Some(dir) = sample("chproj_full/news anchors/Ch Media/Gathered Media") else { return };
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("psd")) {
            let tree = import_path(&path, &Limits::default()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            println!("{}\n{}", path.display(), tree.outline());
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let _ = render_to_scratch(&tree, &format!("psd_{stem}"));
            seen += 1;
        }
    }
    println!("{seen} PSDs imported");
}
