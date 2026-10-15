//! Tests on Adobe-authored sample files. They read `ANICRAFT_SAMPLES` and pass trivially when it is unset.
//!
//! With `ANICRAFT_SCRATCH` set, they also render each sample to a PNG there for visual review.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]

use std::path::{Path, PathBuf};

use anim_artwork::{ArtworkTree, Limits, import_path, to_display};
use anim_display::ImageStore;
use anim_render::{Background, RenderOptions, render};

fn sample(rel: &str) -> Option<PathBuf> {
    let root = std::env::var_os("ANICRAFT_SAMPLES")?;
    let path = PathBuf::from(root).join(rel);
    path.exists().then_some(path)
}

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
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(rgba).unwrap();
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
