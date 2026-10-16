//! Tests against the Adobe-authored sample files. They read `ANICRAFT_SAMPLES` and pass trivially when it is unset,
//! so CI stays green; the samples are never committed.

use anim_archive::{ArchiveWriter, Container, ContainerKind, ContainerWriter, IndexSource, WriteOptions, ZipWriter};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn samples() -> Option<PathBuf> {
    std::env::var_os("ANICRAFT_SAMPLES").map(PathBuf::from).filter(|p| p.is_dir())
}

#[cfg(test)]
mod helpers {
    use anim_archive::Container;
    use std::collections::BTreeMap;

    /// Reads every file entry (verifying size and CRC on the way), returning name -> (length, content hash).
    pub fn read_all(container: &Container) -> BTreeMap<String, (usize, u32)> {
        container
            .files()
            .map(|entry| {
                let bytes = container.read_entry(entry).unwrap_or_else(|e| panic!("reading {:?}: {e}", entry.name));
                assert_eq!(u64::try_from(bytes.len()).unwrap(), entry.size, "size of {}", entry.name);
                (entry.name.clone(), (bytes.len(), fnv1a(&bytes)))
            })
            .collect()
    }

    /// FNV-1a, independent of the crate's own CRC verification; only equality between two reads matters.
    fn fnv1a(bytes: &[u8]) -> u32 {
        let mut hash = 0x811c_9dc5_u32;
        for &b in bytes {
            hash ^= u32::from(b);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash
    }
}
use helpers::read_all;

#[test]
fn unicorn_fla_opens_despite_the_short_central_directory() {
    let Some(dir) = samples() else { return };
    let container = Container::open(dir.join("unicorn_4AE.fla")).unwrap();
    assert_eq!(container.kind(), ContainerKind::Zip);
    assert_eq!(container.len(), 61, "warnings: {:?}", container.warnings());
    eprintln!("unicorn index source: {:?}, warnings: {:?}", container.index_source(), container.warnings());

    let all = read_all(&container);
    assert_eq!(all.len(), container.files().count());
    assert_eq!(container.read("mimetype").unwrap(), b"application/vnd.adobe.xfl");
    assert!(container.read_text("DOMDocument.xml").unwrap().starts_with("<DOMDocument"));
    assert!(container.exists("LIBRARY"));
    assert!(container.exists("library/"));
    assert!(!container.exists("LIBRARY/nothing.xml"));

    // The Illustrator-imported symbol with Animate's `&#060` escapes, found by every spelling.
    let stored = "LIBRARY/flash0.ai_2/Isolation Mode/&#060Clip Group&#062.xml";
    let entry = container.find(stored).expect("escaped name");
    assert_eq!(entry.display_name(), "LIBRARY/flash0.ai_2/Isolation Mode/<Clip Group>.xml");
    assert_eq!(container.find("library/FLASH0.AI_2/isolation mode/<clip group>.xml").map(|e| &e.name), Some(&entry.name));

    // A name with a Hebrew point (U+05B9), stored as flagged UTF-8.
    let holam = container.find("LIBRARY/UNI\u{5b9}_FULL.xml").expect("hebrew-point name");
    assert_eq!(holam.raw_name, "LIBRARY/UNI\u{5b9}_FULL.xml".as_bytes());
    assert_eq!(container.find("library/uni\u{5b9}_full.XML").map(|e| &e.name), Some(&holam.name));
}

#[test]
fn derwood_fla_opens() {
    let Some(dir) = samples() else { return };
    let container = Container::open(dir.join("derwood_green_choreo_short.fla")).unwrap();
    assert_eq!(container.len(), 13, "warnings: {:?}", container.warnings());
    let all = read_all(&container);
    assert!(all.contains_key("DOMDocument.xml"));
    assert!(all.contains_key("LIBRARY/DerwoodGreen.mp3"));
    assert_eq!(container.read("mimetype").unwrap(), b"application/vnd.adobe.xfl");
}

#[test]
fn puppet_zips_read_and_match_their_extracted_folders() {
    let Some(dir) = samples() else { return };
    for (zip_name, folder_name) in [("Boy.puppet", "Boy"), ("blood_cells.puppet", "blood_cells")] {
        let zip = Container::open(dir.join(zip_name)).unwrap();
        assert_eq!(zip.index_source(), IndexSource::ZipCrate, "{zip_name} is a well-formed zip");
        assert_eq!(zip.len(), 25, "{zip_name}");
        let zipped = read_all(&zip);
        assert!(zipped.contains_key("_archive_info"));

        let folder_path = dir.join("extracted").join(folder_name);
        if folder_path.is_dir() {
            let folder = Container::open(&folder_path).unwrap();
            assert_eq!(folder.kind(), ContainerKind::Folder);
            let unpacked = read_all(&folder);
            // The extracted folder holds the same files (the zip also carries empty directory entries).
            for (name, value) in &zipped {
                if let Some(other) = unpacked.get(name) {
                    assert_eq!(other, value, "{name} differs between zip and folder");
                }
            }
            assert!(unpacked.len() >= zipped.len().saturating_sub(2), "folder has {} files, zip has {}", unpacked.len(), zipped.len());
        }
    }
}

#[test]
fn character_animator_project_zip_and_folder_agree() {
    let Some(dir) = samples() else { return };
    let zip_path = dir.join("chproj_zip").join("news_anchors.zip");
    let folder_path = dir.join("chproj_full").join("news anchors");
    if !zip_path.is_file() || !folder_path.is_dir() {
        return;
    }
    let zip = Container::open(&zip_path).unwrap();
    assert_eq!(zip.len(), 643);
    let folder = Container::open(&folder_path).unwrap();
    assert!(folder.exists("news anchors.chproj"));
    assert!(folder.exists("Ch Data/repo.noindex"));

    let zipped: BTreeMap<String, (usize, u32)> =
        read_all(&zip).into_iter().filter_map(|(name, v)| name.strip_prefix("news anchors/").map(|n| (n.to_owned(), v))).collect();
    let unpacked = read_all(&folder);
    let missing: Vec<_> = zipped.keys().filter(|name| !unpacked.contains_key(*name)).take(5).collect();
    assert!(missing.is_empty(), "in the zip but not in the folder: {missing:?}");
    for (name, value) in &zipped {
        assert_eq!(unpacked.get(name), Some(value), "{name}");
    }
}

#[test]
fn fla_survives_a_rewrite_byte_for_byte_and_is_deterministic() {
    let Some(dir) = samples() else { return };
    let source = Container::open(dir.join("derwood_green_choreo_short.fla")).unwrap();
    let before = read_all(&source);

    let write = || {
        let mut writer = ZipWriter::with_mimetype(Vec::new(), anim_archive::XFL_MIMETYPE, WriteOptions::default()).unwrap();
        for entry in source.entries().iter().filter(|e| e.name != "mimetype") {
            if entry.is_dir {
                writer.add_dir(&entry.name).unwrap();
            } else {
                writer.add_file(&entry.name, &source.read_entry(entry).unwrap()).unwrap();
            }
        }
        writer.finish().unwrap()
    };
    let first = write();
    assert_eq!(first, write(), "same input must give the same zip");

    let copy = Container::from_bytes(first).unwrap();
    assert_eq!(copy.index_source(), IndexSource::ZipCrate, "our own output is a strictly valid zip");
    assert_eq!(copy.entries().first().map(|e| e.name.as_str()), Some("mimetype"));
    assert_eq!(copy.entries().first().map(|e| e.method), Some(anim_archive::CompressionMethod::Stored));
    assert_eq!(read_all(&copy), before);
}

/// Writes archives for checking with other zip tools (`unzip -t`, Python's `zipfile`, 7-Zip, Finder, Explorer).
/// Set `ANIM_ARCHIVE_INTEROP_DIR` to an empty folder; without it the test does nothing.
#[test]
fn write_archives_for_interop_checks() {
    let Some(out) = std::env::var_os("ANIM_ARCHIVE_INTEROP_DIR").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();

    let mut writer = ContainerWriter::create(out.join("synthetic.fla"), ContainerKind::Zip).unwrap();
    writer.add_file("mimetype", anim_archive::XFL_MIMETYPE.as_bytes()).unwrap();
    writer.add_dir("LIBRARY").unwrap();
    writer.add_file("DOMDocument.xml", "<DOMDocument/>".repeat(500).as_bytes()).unwrap();
    writer.add_file("LIBRARY/\u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd}.xml", b"<DOMSymbolItem/>").unwrap();
    writer.add_file("LIBRARY/&#060Clip Group&#062.xml", b"<DOMSymbolItem/>").unwrap();
    writer.add_file("bin/empty.dat", b"").unwrap();
    writer.finish().unwrap();

    let options = WriteOptions { zip64: anim_archive::Zip64::Always, ..WriteOptions::default() };
    let mut zip64 = ZipWriter::with_options(Vec::new(), options);
    zip64.add_file("a.txt", "zip64 entry ".repeat(1000).as_bytes()).unwrap();
    zip64.add_dir("dir").unwrap();
    zip64.add_file("dir/b.bin", &[1, 2, 3, 4]).unwrap();
    std::fs::write(out.join("zip64.zip"), zip64.finish().unwrap()).unwrap();

    if let Some(dir) = samples() {
        let source = Container::open(dir.join("derwood_green_choreo_short.fla")).unwrap();
        let mut writer = ContainerWriter::create(out.join("derwood_rewritten.fla"), ContainerKind::Zip).unwrap();
        source.copy_to(&mut writer).unwrap();
        writer.finish().unwrap();
        let mut writer = ContainerWriter::create(out.join("derwood_rewritten.xfl"), ContainerKind::Folder).unwrap();
        source.copy_to(&mut writer).unwrap();
        writer.finish().unwrap();
    }
}

#[test]
fn fla_converts_to_an_xfl_folder_and_back() {
    let Some(dir) = samples() else { return };
    let source = Container::open(dir.join("derwood_green_choreo_short.fla")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let folder_path = scratch.path().join("derwood.xfl");

    let mut writer = ContainerWriter::create(&folder_path, ContainerKind::Folder).unwrap();
    source.copy_to(&mut writer).unwrap();
    writer.finish().unwrap();

    let folder = Container::open(&folder_path).unwrap();
    assert!(folder.exists("DOMDocument.xml"));
    assert_eq!(read_all(&folder), read_all(&source));

    let zip_path = scratch.path().join("again.fla");
    let mut writer = ContainerWriter::create(&zip_path, ContainerKind::Zip).unwrap();
    folder.copy_to(&mut writer).unwrap();
    writer.finish().unwrap();
    let again = Container::open(&zip_path).unwrap();
    assert_eq!(read_all(&again), read_all(&source));
}
