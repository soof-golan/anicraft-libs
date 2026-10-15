//! Tests on Adobe-authored sample files. They read `ANICRAFT_SAMPLES` and pass trivially when it is unset.

use std::path::PathBuf;

use anim_artwork::{ArtworkTree, Limits, import_path};

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

#[test]
fn boy_ai_prints_layer_tree() {
    let Some(tree) = load("extracted/Boy/Ch Media/Gathered Media/A boy character.ai") else { return };
    assert!(tree.size.width > 100.0 && tree.size.height > 100.0, "{:?}", tree.size);
    assert!(tree.root.count() > 5);
}

#[test]
fn girl_ai_prints_layer_tree() {
    let Some(tree) = load("extracted/blood_cells/Ch Media/Gathered Media/A girl character (1).ai") else { return };
    assert!(tree.root.count() > 5);
}
