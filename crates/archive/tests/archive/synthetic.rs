//! Behaviour tests on archives and folders built by the tests themselves (nothing from Adobe).

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]

use crate::support;

use anim_archive::{
    ArchiveError, ArchiveWriter, Compression, CompressionMethod, Container, ContainerKind, ContainerWriter, DirectoryWriter, DosDateTime, IndexSource, Limits,
    NameEncoding, NewEntry, WriteOptions, Zip64, ZipWriter,
};
use support::{Descriptor, RawEntry, RawZip};

fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect()
}

fn xml(len: usize) -> Vec<u8> {
    "<DOMTimeline name=\"Layer 1\"><frames/></DOMTimeline>\r\n".bytes().cycle().take(len).collect()
}

fn names(container: &Container) -> Vec<String> {
    container.entries().iter().map(|e| e.name.clone()).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Writer

#[test]
fn writer_output_round_trips_files_dirs_and_empty_entries() {
    let random = noise(5000, 7);
    let text = xml(20_000);
    let mut writer = ZipWriter::in_memory();
    writer.add_dir("LIBRARY").unwrap();
    writer.add_file("DOMDocument.xml", &text).unwrap();
    writer.add_file("LIBRARY/a b.xml", b"<a/>").unwrap();
    writer.add_file("empty.txt", b"").unwrap();
    writer.add_file("bin/random.dat", &random).unwrap();
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();

    assert_eq!(container.index_source(), IndexSource::ZipCrate);
    assert_eq!(container.warnings().len(), 0);
    assert_eq!(names(&container), ["LIBRARY/", "DOMDocument.xml", "LIBRARY/a b.xml", "empty.txt", "bin/random.dat"]);
    assert_eq!(container.read("DOMDocument.xml").unwrap(), text);
    assert_eq!(container.read("LIBRARY/a b.xml").unwrap(), b"<a/>");
    assert_eq!(container.read("empty.txt").unwrap(), b"");
    assert_eq!(container.read("bin/random.dat").unwrap(), random);
    assert_eq!(container.read("LIBRARY/").unwrap(), b"");

    let method = |name: &str| container.find(name).unwrap().method;
    assert_eq!(method("DOMDocument.xml"), CompressionMethod::Deflate, "compressible text is deflated");
    assert_eq!(method("bin/random.dat"), CompressionMethod::Stored, "incompressible data is stored");
    assert_eq!(method("empty.txt"), CompressionMethod::Stored);
    assert!(container.find("LIBRARY/").unwrap().is_dir);
    assert_eq!(container.find("DOMDocument.xml").unwrap().modified, Some(DosDateTime::EPOCH));
    assert_eq!(container.find("DOMDocument.xml").unwrap().name_encoding, NameEncoding::Utf8Flagged);
    assert_eq!(container.files().count(), 4);
}

#[test]
fn writer_output_is_deterministic_and_mimetype_comes_first_stored() {
    let build = || {
        let mut writer = ZipWriter::with_mimetype(Vec::new(), "application/vnd.adobe.xfl", WriteOptions::default()).unwrap();
        writer.add_file("DOMDocument.xml", &xml(4000)).unwrap();
        writer.add_file("LIBRARY/x.xml", &xml(900)).unwrap();
        writer.finish().unwrap()
    };
    let bytes = build();
    assert_eq!(bytes, build());

    // First local header: signature, version, flags (UTF-8 bit), method 0, then the name and the raw content.
    assert_eq!(bytes.get(..4), Some(&b"PK\x03\x04"[..]));
    assert_eq!(bytes.get(6..8), Some(&0x0800_u16.to_le_bytes()[..]));
    assert_eq!(bytes.get(8..10), Some(&0_u16.to_le_bytes()[..]));
    assert_eq!(bytes.get(30..38), Some(&b"mimetype"[..]));
    assert_eq!(bytes.get(38..63), Some(&b"application/vnd.adobe.xfl"[..]));

    let container = Container::from_bytes(bytes).unwrap();
    assert_eq!(names(&container), ["mimetype", "DOMDocument.xml", "LIBRARY/x.xml"]);
}

#[test]
fn mimetype_is_stored_even_when_added_later_or_asked_to_deflate() {
    let mut writer = ZipWriter::in_memory();
    writer.add_file("a.xml", &xml(2000)).unwrap();
    writer.add_entry(NewEntry::file("mimetype", b"application/vnd.adobe.xfl").with_compression(Compression::Deflate)).unwrap();
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();
    assert_eq!(container.find("mimetype").unwrap().method, CompressionMethod::Stored);
}

#[test]
fn large_entries_are_judged_by_a_sample_and_still_round_trip() {
    let random = noise(2 * 1024 * 1024 + 123, 99);
    let text = xml(3 * 1024 * 1024);
    // Random head, repetitive tail: sampling says "incompressible", which only costs space, never correctness.
    let mixed: Vec<u8> = random.iter().take(200_000).copied().chain(text.iter().copied()).collect();
    let mut writer = ZipWriter::in_memory();
    writer.add_file("random.mp3", &random).unwrap();
    writer.add_file("text.xml", &text).unwrap();
    writer.add_file("mixed.bin", &mixed).unwrap();
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();
    assert_eq!(container.find("random.mp3").unwrap().method, CompressionMethod::Stored);
    assert_eq!(container.find("text.xml").unwrap().method, CompressionMethod::Deflate);
    assert_eq!(container.read("random.mp3").unwrap(), random);
    assert_eq!(container.read("text.xml").unwrap(), text);
    assert_eq!(container.read("mixed.bin").unwrap(), mixed);
}

#[test]
fn writer_honours_timestamps_levels_and_compression_choices() {
    let stamp = DosDateTime { year: 2024, month: 5, day: 17, hour: 13, minute: 45, second: 20 };
    let options = WriteOptions { compression: Compression::Deflate, level: 9, modified: stamp, zip64: Zip64::Auto };
    let mut writer = ZipWriter::with_options(Vec::new(), options);
    writer.add_file("a.txt", &noise(100, 3)).unwrap();
    writer.add_entry(NewEntry::file("b.txt", &xml(3000)).with_compression(Compression::Stored)).unwrap();
    let other = DosDateTime { year: 1999, month: 12, day: 31, hour: 23, minute: 59, second: 58 };
    writer.add_entry(NewEntry::file("c.txt", b"c").with_modified(Some(other))).unwrap();
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();
    assert_eq!(container.find("a.txt").unwrap().method, CompressionMethod::Deflate, "Deflate always deflates");
    assert_eq!(container.find("b.txt").unwrap().method, CompressionMethod::Stored);
    assert_eq!(container.find("a.txt").unwrap().modified, Some(stamp));
    assert_eq!(container.find("c.txt").unwrap().modified, Some(other));
    assert_eq!(container.read("b.txt").unwrap(), xml(3000));
}

#[test]
fn writer_rejects_unsafe_and_duplicate_names() {
    let mut writer = ZipWriter::in_memory();
    for bad in ["../evil.txt", "a/../../b", "", "/", "C:/x", "./", "a\0b/../.."] {
        assert!(matches!(writer.add_file(bad, b"x"), Err(ArchiveError::InvalidName(_))), "{bad:?}");
    }
    let too_long = "n".repeat(70_000);
    assert!(matches!(writer.add_file(&too_long, b"x"), Err(ArchiveError::InvalidName(_))));
    writer.add_file("same.txt", b"1").unwrap();
    assert!(matches!(writer.add_file("same.txt", b"2"), Err(ArchiveError::DuplicateName(_))));
    assert!(matches!(writer.add_file("./same.txt", b"2"), Err(ArchiveError::DuplicateName(_))));
    // A leading slash is dropped rather than honoured.
    writer.add_file("/abs/file.txt", b"ok").unwrap();
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();
    assert_eq!(names(&container), ["same.txt", "abs/file.txt"]);
}

#[test]
fn zip64_output_is_valid_for_the_reader() {
    let options = WriteOptions { zip64: Zip64::Always, ..WriteOptions::default() };
    let mut writer = ZipWriter::with_options(Vec::new(), options);
    writer.add_file("big.xml", &xml(10_000)).unwrap();
    writer.add_dir("d").unwrap();
    writer.add_file("d/e.bin", &noise(300, 11)).unwrap();
    let bytes = writer.finish().unwrap();
    assert!(bytes.windows(4).any(|w| w == b"PK\x06\x06"), "zip64 end record present");
    let container = Container::from_bytes(bytes).unwrap();
    assert_eq!(container.read("big.xml").unwrap(), xml(10_000));
    assert_eq!(container.read("d/e.bin").unwrap(), noise(300, 11));
}

#[test]
fn many_entries_use_the_zip64_end_record() {
    let mut writer = ZipWriter::in_memory();
    for i in 0..70_000_u32 {
        writer.add_entry(NewEntry::file(&format!("f/{i}"), b"").with_compression(Compression::Stored)).unwrap();
    }
    let container = Container::from_bytes_with(writer.finish().unwrap(), Limits { max_entries: 100_000, ..Limits::default() }).unwrap();
    assert_eq!(container.len(), 70_000);
    assert!(container.exists("f/69999"));
}

// ---------------------------------------------------------------------------------------------------------------
// Names

#[test]
fn raw_legacy_names_are_decoded_and_survive_a_rewrite() {
    // "ספר.xml" as Windows-1255 bytes, without the UTF-8 flag, as an old Hebrew tool would write it.
    let raw: &[u8] = &[0xF1, 0xF4, 0xF8, b'.', b'x', b'm', b'l'];
    let zip = RawZip::new(vec![RawEntry::stored("ignored", b"<x/>").with_name_bytes(raw)]).build();
    let container = Container::from_bytes(zip).unwrap();
    let entry = container.entries().first().unwrap();
    assert_eq!(entry.raw_name, raw);
    assert_eq!(entry.name, "ספר.xml");
    assert_eq!(entry.name_encoding, NameEncoding::Windows1255);
    assert_eq!(container.read("ספר.xml").unwrap(), b"<x/>");

    let mut writer = ZipWriter::in_memory();
    container.copy_to(&mut writer).unwrap();
    let copy = Container::from_bytes(writer.finish().unwrap()).unwrap();
    let again = copy.entries().first().unwrap();
    assert_eq!(again.raw_name, raw, "the legacy bytes are written back unchanged");
    assert_eq!(again.name, "ספר.xml");
    assert_eq!(again.name_encoding, NameEncoding::Windows1255);
}

#[test]
fn utf8_names_without_the_flag_are_still_utf8() {
    let zip = RawZip::new(vec![RawEntry::stored("x", b"1").with_name_bytes("LIBRARY/שלום.xml".as_bytes())]).build();
    let container = Container::from_bytes(zip).unwrap();
    assert_eq!(container.entries().first().unwrap().name, "LIBRARY/שלום.xml");
    assert_eq!(container.entries().first().unwrap().name_encoding, NameEncoding::Utf8);
}

#[test]
fn lookups_ignore_case_escapes_separators_and_unicode_form() {
    let mut writer = ZipWriter::in_memory();
    for name in ["LIBRARY/M_body.xml", "LIBRARY/&#060Clip Group&#062.xml", "LIBRARY/caf\u{e9}.xml", "bin/x.dat"] {
        writer.add_file(name, name.as_bytes()).unwrap();
    }
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();
    for spelling in ["LIBRARY/M_body.xml", "library/m_BODY.XML", "./LIBRARY\\M_body.xml", "/LIBRARY//M_body.xml"] {
        assert_eq!(container.find(spelling).map(|e| e.name.as_str()), Some("LIBRARY/M_body.xml"), "{spelling:?}");
    }
    assert!(container.find("LIBRARY/<Clip Group>.xml").is_some(), "unescaped spelling");
    assert!(container.find("library/&#060clip group&#062.XML").is_some(), "escaped spelling, other case");
    assert!(container.find("LIBRARY/cafe\u{301}.xml").is_some(), "decomposed Unicode");
    assert!(container.find("LIBRARY/nothing.xml").is_none());
    assert!(matches!(container.read("LIBRARY/nothing.xml"), Err(ArchiveError::NotFound(_))));
    assert_eq!(container.read("Bin/X.DAT").unwrap(), b"bin/x.dat");
}

#[test]
fn hrefs_resolve_to_library_entries() {
    let mut writer = ZipWriter::in_memory();
    for name in ["LIBRARY/Tween 1.xml", "LIBRARY/flash0.ai_2/Isolation Mode/&#060Clip Group&#062.xml", "bin/M 1 1418138545.dat"] {
        writer.add_file(name, name.as_bytes()).unwrap();
    }
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();
    // The href as an XML parser hands it over: the file name still carries Animate's own `&#060` escapes.
    let nested = container.find_href("flash0.ai_2/Isolation Mode/&#060Clip Group&#062.xml", "LIBRARY").unwrap();
    assert_eq!(nested.name, "LIBRARY/flash0.ai_2/Isolation Mode/&#060Clip Group&#062.xml");
    assert_eq!(nested.display_name(), "LIBRARY/flash0.ai_2/Isolation Mode/<Clip Group>.xml");
    assert!(container.find_href("tween 1.XML", "LIBRARY").is_some());
    assert!(container.find_href("M 1 1418138545.dat", "bin").is_some());
    assert!(container.find_href("missing.xml", "LIBRARY").is_none());
}

#[test]
fn exists_knows_implied_directories() {
    let mut writer = ZipWriter::in_memory();
    writer.add_file("bin/deep/x.dat", b"1").unwrap();
    writer.add_file("top.txt", b"2").unwrap();
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();
    assert!(container.exists("bin"));
    assert!(container.exists("BIN/"));
    assert!(container.exists("bin/deep"));
    assert!(container.exists("top.txt"));
    assert!(!container.exists("bi"));
    assert!(!container.exists("top.txt/more"));
    assert_eq!(container.files_under("bin").count(), 1);
    assert_eq!(container.files_under("BIN/deep/").count(), 1);
    assert_eq!(container.files_under("").count(), 2);
    assert_eq!(container.files_under("b").count(), 0, "a prefix must end at a path separator");
}

#[test]
fn text_reading_handles_byte_order_marks() {
    let mut writer = ZipWriter::in_memory();
    writer.add_file("plain.txt", "héllo".as_bytes()).unwrap();
    writer.add_file("bom8.txt", b"\xEF\xBB\xBFhello").unwrap();
    let little_endian: Vec<u8> = [0xFF, 0xFE].into_iter().chain("שלום".encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let big_endian: Vec<u8> = [0xFE, 0xFF].into_iter().chain("שלום".encode_utf16().flat_map(u16::to_be_bytes)).collect();
    writer.add_file("le.txt", &little_endian).unwrap();
    writer.add_file("be.txt", &big_endian).unwrap();
    writer.add_file("bad.txt", &[0xFF, 0xC3, 0x28]).unwrap();
    let container = Container::from_bytes(writer.finish().unwrap()).unwrap();
    assert_eq!(container.read_text("plain.txt").unwrap(), "héllo");
    assert_eq!(container.read_text("bom8.txt").unwrap(), "hello");
    assert_eq!(container.read_text("le.txt").unwrap(), "שלום");
    assert_eq!(container.read_text("be.txt").unwrap(), "שלום");
    assert!(matches!(container.read_text("bad.txt"), Err(ArchiveError::Text { .. })));
}

// ---------------------------------------------------------------------------------------------------------------
// Damaged and unusual zips

fn sample_entries() -> Vec<RawEntry> {
    vec![
        RawEntry::directory("LIBRARY/"),
        RawEntry::deflated("DOMDocument.xml", &xml(30_000)),
        RawEntry::stored("bin/data.dat", &noise(4000, 5)),
        RawEntry::deflated("LIBRARY/a.xml", &xml(500)),
        RawEntry::stored("mimetype", b"application/vnd.adobe.xfl"),
    ]
}

fn assert_sample_contents(container: &Container) {
    assert_eq!(names(container), ["LIBRARY/", "DOMDocument.xml", "bin/data.dat", "LIBRARY/a.xml", "mimetype"]);
    assert_eq!(container.read("DOMDocument.xml").unwrap(), xml(30_000));
    assert_eq!(container.read("bin/data.dat").unwrap(), noise(4000, 5));
    assert_eq!(container.read("LIBRARY/a.xml").unwrap(), xml(500));
    assert_eq!(container.read("mimetype").unwrap(), b"application/vnd.adobe.xfl");
}

#[test]
fn a_central_directory_size_that_is_too_long_still_opens() {
    // This is how Animate writes FLA files: the end record claims a directory 54 bytes longer than it is.
    let mut zip = RawZip::new(sample_entries());
    zip.cd_size_delta = 54;
    let container = Container::from_bytes(zip.build()).unwrap();
    assert_sample_contents(&container);
    assert_ne!(container.index_source(), IndexSource::Folder);
}

#[test]
fn a_central_directory_size_that_is_too_short_still_opens() {
    let mut zip = RawZip::new(sample_entries());
    zip.cd_size_delta = -30;
    assert_sample_contents(&Container::from_bytes(zip.build()).unwrap());
}

#[test]
fn a_missing_central_directory_is_recovered_from_local_headers() {
    let mut zip = RawZip::new(sample_entries());
    zip.central = false;
    let container = Container::from_bytes(zip.build()).unwrap();
    assert_eq!(container.index_source(), IndexSource::LocalHeaders);
    assert!(container.warnings().iter().any(|w| w.contains("scanning local file headers")));
    assert_sample_contents(&container);
    // Directory-ness comes from the trailing slash when there is no directory to say so.
    assert!(container.find("LIBRARY/").unwrap().is_dir);
}

#[test]
fn a_truncated_archive_keeps_every_complete_entry() {
    let mut zip = RawZip::new(sample_entries());
    zip.central = false;
    let bytes = zip.build();
    let cut = bytes.len() - 20;
    let container = Container::from_bytes(bytes.get(..cut).unwrap().to_vec()).unwrap();
    assert_eq!(names(&container), ["LIBRARY/", "DOMDocument.xml", "bin/data.dat", "LIBRARY/a.xml"]);
    assert_eq!(container.read("DOMDocument.xml").unwrap(), xml(30_000));
    assert!(!container.exists("mimetype"));
}

#[test]
fn truncating_a_complete_archive_never_panics_and_recovers_a_prefix() {
    let bytes = RawZip::new(sample_entries()).build();
    for cut in 0..bytes.len() {
        let Some(head) = bytes.get(..cut) else { continue };
        if let Ok(container) = Container::from_bytes(head.to_vec()) {
            for entry in container.entries() {
                let _ = container.read_entry(entry);
            }
        }
    }
}

#[test]
fn entries_with_data_descriptors_are_recovered_by_scanning() {
    let entries = vec![
        RawEntry::deflated("signed.xml", &xml(9000)).with_descriptor(Descriptor::Signed),
        RawEntry::deflated("unsigned.xml", &xml(7000)).with_descriptor(Descriptor::Unsigned),
        RawEntry::deflated("zip64.xml", &xml(5000)).with_descriptor(Descriptor::Signed64),
        RawEntry::stored("stored.bin", &noise(3000, 9)).with_descriptor(Descriptor::Signed),
        RawEntry::deflated("empty.xml", b"").with_descriptor(Descriptor::Signed),
        RawEntry::stored("last.txt", b"after the descriptors"),
    ];
    let mut zip = RawZip::new(entries);
    zip.central = false;
    let container = Container::from_bytes(zip.build()).unwrap();
    assert_eq!(container.index_source(), IndexSource::LocalHeaders);
    assert_eq!(container.read("signed.xml").unwrap(), xml(9000));
    assert_eq!(container.read("unsigned.xml").unwrap(), xml(7000));
    assert_eq!(container.read("zip64.xml").unwrap(), xml(5000));
    assert_eq!(container.read("stored.bin").unwrap(), noise(3000, 9));
    assert_eq!(container.read("empty.xml").unwrap(), b"");
    assert_eq!(container.read("last.txt").unwrap(), b"after the descriptors");
}

#[test]
fn entries_with_data_descriptors_read_normally_with_a_directory() {
    let entries = vec![
        RawEntry::deflated("a.xml", &xml(9000)).with_descriptor(Descriptor::Signed),
        RawEntry::stored("b.bin", &noise(100, 1)).with_descriptor(Descriptor::Unsigned),
    ];
    let container = Container::from_bytes(RawZip::new(entries).build()).unwrap();
    assert_eq!(container.index_source(), IndexSource::ZipCrate);
    assert_eq!(container.read("a.xml").unwrap(), xml(9000));
    assert_eq!(container.read("b.bin").unwrap(), noise(100, 1));
}

#[test]
fn a_self_extracting_prefix_and_an_archive_comment_are_tolerated() {
    let mut zip = RawZip::new(sample_entries());
    zip.prefix = b"MZ\x90\x00 pretend this is an executable stub ".repeat(40);
    zip.comment = b"created by a test".to_vec();
    assert_sample_contents(&Container::from_bytes(zip.build()).unwrap());
}

#[test]
fn an_end_record_count_that_is_too_small_does_not_hide_entries() {
    // Writers unaware of Zip64 wrap the 16-bit entry count; readers that trust it silently lose entries.
    let mut bytes = RawZip::new(sample_entries()).build();
    let end = bytes.len() - 22;
    bytes.get_mut(end + 8..end + 12).unwrap().copy_from_slice(&[2, 0, 2, 0]);
    let container = Container::from_bytes(bytes).unwrap();
    assert_sample_contents(&container);
    assert!(container.warnings().iter().any(|w| w.contains("central directory lists 5")), "{:?}", container.warnings());
}

#[test]
fn containers_are_shareable_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Container>();
    assert_send_sync::<anim_archive::Entry>();
    assert_send_sync::<ArchiveError>();
}

#[test]
fn legacy_binary_documents_get_a_helpful_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.fla");
    let mut header = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    header.resize(2048, 0);
    std::fs::write(&path, header).unwrap();
    let error = Container::open(&path).unwrap_err();
    assert!(matches!(error, ArchiveError::NotAnArchive(_)));
    assert!(error.to_string().contains("legacy binary document"), "{error}");
}

#[test]
fn an_empty_zip_is_an_empty_container() {
    let bytes = ZipWriter::in_memory().finish().unwrap();
    let container = Container::from_bytes(bytes).unwrap();
    assert!(container.is_empty());
    assert_eq!(container.kind(), ContainerKind::Zip);
}

#[test]
fn non_zip_data_is_reported_as_such() {
    for junk in [&b""[..], &b"hello, this is not a zip file"[..], &[0_u8; 4096][..]] {
        assert!(matches!(Container::from_bytes(junk.to_vec()), Err(ArchiveError::NotAnArchive(_))), "{} bytes", junk.len());
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Limits and corruption

#[test]
fn entry_count_total_size_and_ratio_limits_are_enforced_when_opening() {
    let three = RawZip::new(vec![RawEntry::stored("a", b"1"), RawEntry::stored("b", b"2"), RawEntry::stored("c", b"3")]).build();
    assert!(matches!(
        Container::from_bytes_with(three.clone(), Limits { max_entries: 2, ..Limits::default() }),
        Err(ArchiveError::LimitExceeded { what: "entry count", .. })
    ));
    assert!(Container::from_bytes_with(three, Limits { max_entries: 3, ..Limits::default() }).is_ok());

    let two = RawZip::new(vec![RawEntry::stored("a", &[1; 100]), RawEntry::stored("b", &[2; 100])]).build();
    assert!(matches!(
        Container::from_bytes_with(two.clone(), Limits { max_total_size: 150, ..Limits::default() }),
        Err(ArchiveError::LimitExceeded { what: "total uncompressed size", .. })
    ));
    assert!(Container::from_bytes_with(two, Limits { max_total_size: 200, ..Limits::default() }).is_ok());

    // A 30-byte deflate stream that claims to expand to 400 MB is forged: deflate cannot exceed ~1032:1.
    let mut bomb = RawEntry::deflated("bomb.bin", &vec![0_u8; 1000]);
    bomb.size_override = Some(400_000_000);
    let error = Container::from_bytes(RawZip::new(vec![bomb]).build()).unwrap_err();
    assert!(matches!(error, ArchiveError::LimitExceeded { what, .. } if what.starts_with("compression ratio")), "{error}");

    let long_name = "n".repeat(300);
    let named = RawZip::new(vec![RawEntry::stored(&long_name, b"1")]).build();
    assert!(matches!(
        Container::from_bytes_with(named, Limits { max_name_len: 100, ..Limits::default() }),
        Err(ArchiveError::LimitExceeded { what: "entry name length", .. })
    ));
}

#[test]
fn oversized_entries_open_but_refuse_to_read() {
    let zip = RawZip::new(vec![RawEntry::stored("small", b"ok"), RawEntry::stored("large", &[7; 100])]).build();
    let container = Container::from_bytes_with(zip, Limits { max_entry_size: 50, ..Limits::default() }).unwrap();
    assert_eq!(container.read("small").unwrap(), b"ok");
    assert!(matches!(container.read("large"), Err(ArchiveError::LimitExceeded { what: "entry size", .. })));
}

#[test]
fn a_stream_that_expands_past_its_declared_size_is_stopped() {
    let mut liar = RawEntry::deflated("liar.bin", &vec![0_u8; 200_000]);
    liar.size_override = Some(10);
    let container = Container::from_bytes(RawZip::new(vec![liar]).build()).unwrap();
    assert!(matches!(container.read("liar.bin"), Err(ArchiveError::SizeMismatch { expected: 10, .. })));
}

#[test]
fn a_wrong_checksum_is_reported_per_entry() {
    let mut bad = RawEntry::stored("bad.txt", b"payload");
    bad.crc_override = Some(0xDEAD_BEEF);
    let container = Container::from_bytes(RawZip::new(vec![bad, RawEntry::stored("good.txt", b"fine")]).build()).unwrap();
    assert!(matches!(container.read("bad.txt"), Err(ArchiveError::Checksum { expected: 0xDEAD_BEEF, .. })));
    assert_eq!(container.read("good.txt").unwrap(), b"fine");
}

#[test]
fn a_flipped_data_byte_is_caught() {
    let bytes = RawZip::new(vec![RawEntry::stored("data.bin", &noise(500, 21))]).build();
    let mut damaged = bytes;
    // Local header is 30 bytes plus the 8-byte name.
    *damaged.get_mut(38 + 100).unwrap() ^= 0x40;
    let container = Container::from_bytes(damaged).unwrap();
    assert!(matches!(container.read("data.bin"), Err(ArchiveError::Checksum { .. })));
}

#[test]
fn corrupt_deflate_data_is_an_error_not_a_panic() {
    let mut damaged = RawZip::new(vec![RawEntry::deflated("x.xml", &xml(5000))]).build();
    for byte in damaged.iter_mut().skip(35).take(12) {
        *byte = 0xFF;
    }
    let container = Container::from_bytes(damaged).unwrap();
    assert!(container.read("x.xml").is_err());
}

#[test]
fn encrypted_and_unsupported_entries_report_why() {
    let encrypted = RawEntry::stored("secret.bin", b"12345").with_flags(0x0001);
    let mut bzip = RawEntry::stored("packed.bin", b"BZh91AY&SY");
    bzip.method = 12;
    let container = Container::from_bytes(RawZip::new(vec![encrypted, bzip]).build()).unwrap();
    assert!(container.find("secret.bin").unwrap().encrypted);
    assert!(matches!(container.read("secret.bin"), Err(ArchiveError::Encrypted { .. })));
    assert!(matches!(container.read("packed.bin"), Err(ArchiveError::UnsupportedMethod { method: 12, .. })));
    assert_eq!(container.find("packed.bin").unwrap().method, CompressionMethod::Other(12));
}

#[test]
fn entries_that_share_data_are_rejected_as_overlapping() {
    let mut bytes = RawZip::new(vec![RawEntry::stored("a", &[1; 64]), RawEntry::stored("b", &[2; 64])]).build();
    // Point the second central record at the first entry's local header (offset 0).
    let second = bytes.windows(4).enumerate().filter(|(_, w)| *w == b"PK\x01\x02").nth(1).unwrap().0;
    bytes.get_mut(second + 42..second + 46).unwrap().copy_from_slice(&0_u32.to_le_bytes());
    let error = Container::from_bytes(bytes).unwrap_err();
    assert!(error.to_string().contains("overlap"), "{error}");
}

#[test]
fn entry_offsets_pointing_outside_the_file_fail_only_when_read() {
    let mut bytes = RawZip::new(vec![RawEntry::stored("a", &[1; 64]), RawEntry::stored("b", &[2; 64])]).build();
    let second = bytes.windows(4).enumerate().filter(|(_, w)| *w == b"PK\x01\x02").nth(1).unwrap().0;
    bytes.get_mut(second + 42..second + 46).unwrap().copy_from_slice(&0x7000_0000_u32.to_le_bytes());
    match Container::from_bytes(bytes) {
        Ok(container) => {
            assert!(container.read("b").is_err());
            assert!(!container.warnings().is_empty() || container.index_source() != IndexSource::ZipCrate);
        }
        Err(error) => assert_ne!(error.to_string(), ""),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Folders

#[test]
fn folder_containers_list_sorted_and_read_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("a/b")).unwrap();
    std::fs::create_dir_all(root.join("emptydir")).unwrap();
    std::fs::write(root.join("a/b/c.txt"), b"deep").unwrap();
    std::fs::write(root.join("a/d.txt"), b"mid").unwrap();
    std::fs::write(root.join("top.txt"), b"top").unwrap();
    std::fs::write(root.join("Space Name.xml"), b"<s/>").unwrap();

    let container = Container::open(root).unwrap();
    assert_eq!(container.kind(), ContainerKind::Folder);
    assert_eq!(container.index_source(), IndexSource::Folder);
    assert_eq!(names(&container), ["Space Name.xml", "a/", "a/b/", "a/b/c.txt", "a/d.txt", "emptydir/", "top.txt"]);
    assert_eq!(container.read("a/b/c.txt").unwrap(), b"deep");
    assert_eq!(container.read("A/D.TXT").unwrap(), b"mid");
    assert_eq!(container.read_text("space name.xml").unwrap(), "<s/>");
    assert!(container.exists("emptydir"));
    assert!(container.find("a/").unwrap().is_dir);
    assert_eq!(container.find("a/d.txt").unwrap().size, 3);
    assert_eq!(container.files_under("a").count(), 2);
    assert_eq!(container.path(), Some(root));
}

#[test]
fn folder_depth_and_entry_limits_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("1/2/3/4/5");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("x"), b"1").unwrap();
    assert!(matches!(
        Container::open_with(dir.path(), Limits { max_dir_depth: 3, ..Limits::default() }),
        Err(ArchiveError::LimitExceeded { what: "folder depth", .. })
    ));
    assert!(Container::open_with(dir.path(), Limits { max_dir_depth: 5, ..Limits::default() }).is_ok());
    assert!(matches!(
        Container::open_with(dir.path(), Limits { max_entries: 3, ..Limits::default() }),
        Err(ArchiveError::LimitExceeded { what: "entry count", .. })
    ));
}

#[cfg(unix)]
#[test]
fn folder_symlinks_to_folders_are_skipped_and_to_files_followed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("real")).unwrap();
    std::fs::write(root.join("real/file.txt"), b"data").unwrap();
    std::os::unix::fs::symlink(root, root.join("real/loop")).unwrap();
    std::os::unix::fs::symlink(root.join("real/file.txt"), root.join("alias.txt")).unwrap();
    std::os::unix::fs::symlink(root.join("missing"), root.join("broken")).unwrap();
    let container = Container::open(root).unwrap();
    assert_eq!(names(&container), ["alias.txt", "real/", "real/file.txt"]);
    assert_eq!(container.read("alias.txt").unwrap(), b"data");
    assert_eq!(container.warnings().len(), 2, "{:?}", container.warnings());
}

#[test]
fn folder_files_over_the_entry_limit_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.bin"), [0_u8; 100]).unwrap();
    let container = Container::open_with(dir.path(), Limits { max_entry_size: 10, ..Limits::default() }).unwrap();
    assert!(matches!(container.read("big.bin"), Err(ArchiveError::LimitExceeded { what: "entry size", .. })));
}

#[test]
fn opening_the_xfl_marker_file_opens_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("doc.xfl"), b"PROXY-CS5").unwrap();
    std::fs::write(dir.path().join("DOMDocument.xml"), b"<DOMDocument/>").unwrap();
    std::fs::create_dir_all(dir.path().join("LIBRARY")).unwrap();
    let container = Container::open(dir.path().join("doc.xfl")).unwrap();
    assert_eq!(container.kind(), ContainerKind::Folder);
    assert!(container.exists("DOMDocument.xml"));
    assert!(container.exists("LIBRARY"));

    // A real zip with the same extension is still a zip.
    let mut writer = ZipWriter::in_memory();
    writer.add_file("DOMDocument.xml", b"<DOMDocument/>").unwrap();
    std::fs::write(dir.path().join("zipped.xfl"), writer.finish().unwrap()).unwrap();
    assert_eq!(Container::open(dir.path().join("zipped.xfl")).unwrap().kind(), ContainerKind::Zip);
}

#[test]
fn missing_paths_are_io_errors() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(Container::open(dir.path().join("nope.fla")), Err(ArchiveError::Io { .. })));
}

#[test]
fn directory_writer_escapes_validates_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("out.xfl");
    let mut writer = DirectoryWriter::create(&root).unwrap();
    writer.add_dir("LIBRARY").unwrap();
    writer.add_file("DOMDocument.xml", b"<d/>").unwrap();
    writer.add_file("LIBRARY/&#060Clip Group&#062.xml", b"<clip/>").unwrap();
    writer.add_file("LIBRARY/odd:name?.xml", b"<odd/>").unwrap();
    writer.add_file("bin/deep/er/x.dat", b"bin").unwrap();
    assert!(matches!(writer.add_file("../escape.txt", b"x"), Err(ArchiveError::InvalidName(_))));
    assert!(matches!(writer.add_file("a/../../escape.txt", b"x"), Err(ArchiveError::InvalidName(_))));
    assert!(matches!(writer.add_file("DOMDocument.xml", b"again"), Err(ArchiveError::DuplicateName(_))));
    writer.finish().unwrap();
    assert!(!dir.path().join("escape.txt").exists());

    // Characters Windows forbids are escaped on disk; names that already carry the escape are untouched.
    assert!(root.join("LIBRARY/&#060Clip Group&#062.xml").is_file());
    assert!(root.join("LIBRARY/odd&#058name&#063.xml").is_file());

    let container = Container::open(&root).unwrap();
    assert_eq!(container.read("LIBRARY/<Clip Group>.xml").unwrap(), b"<clip/>");
    assert_eq!(container.read("LIBRARY/odd:name?.xml").unwrap(), b"<odd/>");
    assert_eq!(container.read("bin/deep/er/x.dat").unwrap(), b"bin");
}

#[test]
fn container_writer_chooses_zip_or_folder_at_run_time() {
    let dir = tempfile::tempdir().unwrap();
    for (kind, name) in [(ContainerKind::Zip, "out.fla"), (ContainerKind::Folder, "out_xfl")] {
        let path = dir.path().join(name);
        let mut writer = ContainerWriter::create(&path, kind).unwrap();
        writer.add_file("mimetype", b"application/vnd.adobe.xfl").unwrap();
        writer.add_dir("LIBRARY").unwrap();
        writer.add_file("LIBRARY/a.xml", &xml(2000)).unwrap();
        writer.finish().unwrap();
        let container = Container::open(&path).unwrap();
        assert_eq!(container.kind(), kind);
        assert_eq!(container.read("mimetype").unwrap(), b"application/vnd.adobe.xfl");
        assert_eq!(container.read("LIBRARY/a.xml").unwrap(), xml(2000));
    }
}

#[test]
fn containers_can_be_read_from_several_threads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.fla");
    let mut writer = ContainerWriter::create(&path, ContainerKind::Zip).unwrap();
    for i in 0..40_u32 {
        writer.add_file(&format!("LIBRARY/{i}.xml"), &xml(2000 + usize::try_from(i).unwrap() * 37)).unwrap();
    }
    writer.finish().unwrap();
    let container = Container::open(&path).unwrap();
    std::thread::scope(|scope| {
        for worker in 0..4_usize {
            let container = &container;
            scope.spawn(move || {
                for (index, entry) in container.entries().iter().enumerate() {
                    if index % 4 == worker {
                        assert_eq!(container.read_entry(entry).unwrap(), xml(2000 + index * 37));
                    }
                }
            });
        }
    });
}
