//! Property tests: hostile and damaged input never panics, and what the writer produces reads back unchanged.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]

use crate::support;

use anim_archive::encoding::decode_name;
use anim_archive::names::{escape_filename, is_safe_relative, lookup_key, normalize_name, unescape_filename, xml_escape, xml_unescape};
use anim_archive::{ArchiveWriter, Container, Limits, ZipWriter};
use proptest::prelude::*;
use support::{Descriptor, RawEntry, RawZip};

/// Limits small enough that a forged size cannot make a test allocate or inflate much.
fn limits() -> Limits {
    Limits { max_entries: 5000, max_entry_size: 8 * 1024 * 1024, max_total_size: 64 * 1024 * 1024, ..Limits::default() }
}

/// Opens `bytes` and, if that works, reads every entry. Errors are fine; panics and runaway allocations are not.
fn open_and_read_everything(bytes: Vec<u8>) {
    if let Ok(container) = Container::from_bytes_with(bytes, limits()) {
        for entry in container.entries() {
            if let Ok(data) = container.read_entry(entry) {
                assert_eq!(u64::try_from(data.len()).unwrap(), entry.size);
            }
            let _ = container.find(&entry.name);
            let _ = entry.display_name();
        }
        let _ = container.exists("LIBRARY");
        let _ = container.files_under("LIBRARY").count();
    }
}

fn valid_archives() -> Vec<Vec<u8>> {
    let text: Vec<u8> = b"<DOMTimeline name=\"Layer 1\"/>\r\n".iter().copied().cycle().take(3000).collect();
    let mut with_directory = RawZip::new(vec![
        RawEntry::directory("LIBRARY/"),
        RawEntry::deflated("DOMDocument.xml", &text),
        RawEntry::stored("bin/data.dat", &[9; 200]),
        RawEntry::deflated("LIBRARY/a.xml", &text).with_descriptor(Descriptor::Signed),
        RawEntry::stored("mimetype", b"application/vnd.adobe.xfl"),
    ]);
    let normal = with_directory.build();
    with_directory.central = false;
    let scanned = with_directory.build();
    let mut zip64 =
        ZipWriter::with_options(Vec::new(), anim_archive::WriteOptions { zip64: anim_archive::Zip64::Always, ..anim_archive::WriteOptions::default() });
    zip64.add_file("a.xml", &text).unwrap();
    zip64.add_file("b.bin", &[1, 2, 3]).unwrap();
    vec![normal, scanned, zip64.finish().unwrap()]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..3000)) {
        open_and_read_everything(bytes);
    }

    #[test]
    fn arbitrary_bytes_behind_a_zip_signature_never_panic(tail in proptest::collection::vec(any::<u8>(), 0..400)) {
        let mut bytes = b"PK\x03\x04".to_vec();
        bytes.extend(tail.iter().copied());
        open_and_read_everything(bytes);
        let mut ending = tail;
        ending.extend_from_slice(b"PK\x05\x06");
        ending.extend_from_slice(&[0; 18]);
        open_and_read_everything(ending);
    }

    #[test]
    fn mutated_archives_never_panic(
        which in 0_usize..3,
        edits in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..12),
        cut in proptest::option::of(any::<prop::sample::Index>()),
    ) {
        let archives = valid_archives();
        let mut bytes = archives.get(which).unwrap().clone();
        for (index, value) in edits {
            let at = index.index(bytes.len());
            *bytes.get_mut(at).unwrap() = value;
        }
        if let Some(cut) = cut {
            bytes.truncate(cut.index(bytes.len() + 1));
        }
        open_and_read_everything(bytes);
    }

    #[test]
    fn spliced_archives_never_panic(
        a in 0_usize..3,
        b in 0_usize..3,
        from in any::<prop::sample::Index>(),
        len in 0_usize..200,
        at in any::<prop::sample::Index>(),
    ) {
        let archives = valid_archives();
        let source = archives.get(a).unwrap();
        let mut target = archives.get(b).unwrap().clone();
        let start = from.index(source.len());
        let piece: Vec<u8> = source.iter().copied().skip(start).take(len).collect();
        let insert_at = at.index(target.len() + 1);
        target.splice(insert_at..insert_at, piece);
        open_and_read_everything(target);
    }

    #[test]
    fn name_decoding_is_total(raw in proptest::collection::vec(any::<u8>(), 0..64), flag in any::<bool>()) {
        let (text, encoding) = decode_name(&raw, flag);
        if let Ok(valid) = std::str::from_utf8(&raw) {
            prop_assert_eq!(text.as_str(), valid);
            prop_assert!(encoding.is_unicode());
        }
        let _ = lookup_key(&text);
    }

    #[test]
    fn normalised_names_are_stable_and_safe_means_no_parent_segments(name in ".{0,80}") {
        let once = normalize_name(&name);
        let twice = normalize_name(&once);
        prop_assert_eq!(twice.as_str(), once.as_str());
        prop_assert!(!once.contains("//"));
        prop_assert!(!once.starts_with('/'));
        if is_safe_relative(&once) {
            prop_assert!(once.split('/').all(|part| part != ".."));
            prop_assert!(!once.contains('\0'));
        }
        let _ = lookup_key(&name);
    }

    #[test]
    fn filename_escaping_round_trips_without_literal_escapes(name in "[^&]{0,60}") {
        let escaped = escape_filename(&name);
        let back = unescape_filename(&escaped);
        prop_assert_eq!(back.as_ref(), name.as_str());
    }

    #[test]
    fn xml_escaping_round_trips(text in ".{0,80}") {
        let escaped = xml_escape(&text);
        let back = xml_unescape(&escaped);
        prop_assert_eq!(back.as_ref(), text.as_str());
    }

    #[test]
    fn writer_output_reads_back_identically(
        files in proptest::collection::btree_map(
            "[A-Za-z0-9_][A-Za-z0-9_ .-]{0,10}(/[A-Za-z0-9_][A-Za-z0-9_ .-]{0,10}){0,3}",
            (proptest::collection::vec(any::<u8>(), 0..1500), any::<bool>()),
            1..12,
        ),
        keep_directory in any::<bool>(),
    ) {
        let mut writer = ZipWriter::in_memory();
        for (name, (data, repetitive)) in &files {
            let data: Vec<u8> = if *repetitive { data.iter().flat_map(|b| [*b; 8]).collect() } else { data.clone() };
            writer.add_file(name, &data).unwrap();
        }
        let mut bytes = writer.finish().unwrap();
        if !keep_directory {
            // Cut the central directory off so that the local-header scan has to find everything.
            let eocd = bytes.len() - 22;
            let directory_start = usize::try_from(u32::from_le_bytes(bytes.get(eocd + 16..eocd + 20).unwrap().try_into().unwrap())).unwrap();
            bytes.truncate(directory_start);
        }
        let container = Container::from_bytes(bytes).unwrap();
        prop_assert_eq!(container.len(), files.len());
        for (name, (data, repetitive)) in &files {
            let expected: Vec<u8> = if *repetitive { data.iter().flat_map(|b| [*b; 8]).collect() } else { data.clone() };
            prop_assert_eq!(container.read(name).unwrap(), expected);
        }
    }
}
