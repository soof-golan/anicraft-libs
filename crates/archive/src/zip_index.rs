//! Building the entry index of a zip archive, with three strategies tried in order:
//!
//! 1. the `zip` crate (strict, handles Zip64 and archive prefixes), with its offset detection and then with a fixed
//!    zero offset;
//! 2. a tolerant walk of the central directory that ignores the End-Of-Central-Directory size and count fields.
//!    Animate writes FLA files whose `cd_size` is 54 bytes larger than the directory really is, which makes strict
//!    readers refuse them ("missing 54 bytes in zipfile");
//! 3. a scan of the local file headers (`PK\x03\x04`) from the start of the file, which recovers every complete
//!    entry of a truncated or otherwise directory-less archive. Stored and deflated entries are supported, with or
//!    without data descriptors.

use crate::bytes::{self, Cursor, le_u16, le_u32, le_u64};
use crate::error::{ArchiveError, Result};
use crate::limits::Limits;
use flate2::{Decompress, FlushDecompress, Status};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// `Read + Seek` as one object-safe trait.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek + ?Sized> ReadSeek for T {}

pub const SIG_LOCAL: [u8; 4] = *b"PK\x03\x04";
pub const SIG_CENTRAL: [u8; 4] = *b"PK\x01\x02";
pub const SIG_EOCD: [u8; 4] = *b"PK\x05\x06";
pub const SIG_EOCD64: [u8; 4] = *b"PK\x06\x06";
pub const SIG_LOCATOR64: [u8; 4] = *b"PK\x06\x07";
pub const SIG_DESCRIPTOR: [u8; 4] = *b"PK\x07\x08";
const SIG_DIGITAL: [u8; 4] = *b"PK\x05\x05";

pub const LOCAL_HEADER_LEN: u64 = 30;
const CENTRAL_HEADER_LEN: usize = 46;
const EOCD_LEN: usize = 22;
/// The EOCD comment is at most 64 KiB, so the record starts within this many bytes of the end.
const EOCD_SEARCH: u64 = 65_535 + 22 + 20;
const U32_MAX_MARK: u32 = u32::MAX;
const FLAG_ENCRYPTED: u16 = 0x0001;
const FLAG_DESCRIPTOR: u16 = 0x0008;
const EXTRA_ZIP64: u16 = 0x0001;

/// Which strategy produced the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IndexSource {
    /// A well-formed archive, read by the `zip` crate.
    ZipCrate,
    /// The `zip` crate refused the end-of-central-directory record, which disagrees with the directory itself (this is
    /// how Animate writes FLA files: the declared directory size is 54 bytes too long), but read the archive when told
    /// it starts at byte 0.
    ZipCrateFixedOffset,
    /// The central directory was walked tolerantly because the strict reader refused the archive (for example
    /// Animate's FLA files, whose declared directory size is 54 bytes too long).
    CentralDirectory,
    /// The central directory was unusable; entries were recovered by scanning local file headers.
    LocalHeaders,
    /// A folder container, not a zip.
    Folder,
}

/// One entry as found in the archive, before names are decoded and limits applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    pub raw_name: Vec<u8>,
    pub flags: u16,
    pub method: u16,
    pub dos_date: u16,
    pub dos_time: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub size: u64,
    pub header_offset: u64,
    pub external_attributes: u32,
}

impl RawEntry {
    pub const fn encrypted(&self) -> bool {
        self.flags & FLAG_ENCRYPTED != 0
    }

    /// MS-DOS directory bit, or the Unix `S_IFDIR` file type in the high half of the attributes.
    pub const fn directory_attribute(&self) -> bool {
        self.external_attributes & 0x10 != 0 || (self.external_attributes >> 16) & 0o170_000 == 0o040_000
    }
}

/// The result of indexing.
#[derive(Debug)]
pub struct Index {
    pub entries: Vec<RawEntry>,
    pub source: IndexSource,
    pub warnings: Vec<String>,
}

const fn is_hard_failure(error: &ArchiveError) -> bool {
    matches!(error, ArchiveError::LimitExceeded { .. } | ArchiveError::Io { .. })
}

fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or_else(|| ArchiveError::malformed("offset arithmetic overflows"))
}

/// Indexes the zip in `reader` (`len` bytes long), trying every strategy in turn.
pub fn build_index(reader: &mut dyn ReadSeek, len: u64, limits: &Limits) -> Result<Index> {
    let mut warnings = Vec::new();
    let first_error = match index_with_zip_crate(reader, limits) {
        Ok((entries, source)) => {
            if source == IndexSource::ZipCrateFixedOffset {
                warnings.push("the end-of-central-directory record disagrees with the directory (as in files saved by Animate); the archive was read as starting at byte 0".to_owned());
            }
            return Ok(Index { entries, source, warnings });
        }
        Err(e) if is_hard_failure(&e) => return Err(e),
        Err(e) => e,
    };
    match index_central_directory(reader, len, limits, &mut warnings) {
        Ok(entries) if !entries.is_empty() => {
            warnings.push(format!("strict zip reader refused the archive ({first_error}); read the central directory tolerantly"));
            return Ok(Index { entries, source: IndexSource::CentralDirectory, warnings });
        }
        Err(e) if is_hard_failure(&e) => return Err(e),
        Ok(_) | Err(_) => {}
    }
    match scan_local_headers(reader, len, limits, &mut warnings) {
        Ok(entries) if !entries.is_empty() => {
            warnings.push(format!("no usable central directory ({first_error}); recovered entries by scanning local file headers"));
            Ok(Index { entries, source: IndexSource::LocalHeaders, warnings })
        }
        Err(e) if is_hard_failure(&e) => Err(e),
        Ok(_) | Err(_) => Err(first_error),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Strategy 1: the zip crate

fn index_with_zip_crate(reader: &mut dyn ReadSeek, limits: &Limits) -> Result<(Vec<RawEntry>, IndexSource)> {
    match zip_crate_pass(reader, zip::read::Config::default(), limits) {
        Ok(entries) => Ok((entries, IndexSource::ZipCrate)),
        Err(first) if !is_hard_failure(&first) => {
            // Archives whose end record disagrees with the directory (Animate's FLA files) open when the archive is
            // declared to start at byte 0 instead of being "detected".
            match zip_crate_pass(reader, zip::read::Config { archive_offset: zip::read::ArchiveOffset::Known(0) }, limits) {
                Ok(entries) => Ok((entries, IndexSource::ZipCrateFixedOffset)),
                Err(second) if is_hard_failure(&second) => Err(second),
                Err(_) => Err(first),
            }
        }
        Err(other) => Err(other),
    }
}

fn zip_crate_pass(reader: &mut dyn ReadSeek, config: zip::read::Config, limits: &Limits) -> Result<Vec<RawEntry>> {
    reader.seek(SeekFrom::Start(0)).map_err(|e| ArchiveError::io("seeking to the start of the archive", e))?;
    // The zip crate has panicked on hostile input in the past (and overflow checks are on in dev builds); a panic in
    // a dependency must never take the application down, so it counts as "this strategy failed".
    let attempt = catch_unwind(AssertUnwindSafe(|| -> Result<Vec<RawEntry>> {
        let archive = zip::ZipArchive::with_config(config, &mut *reader).map_err(zip_error)?;
        let meta = archive.metadata();
        limits.check_entry_count(meta.len())?;
        let mut out = Vec::with_capacity(meta.len().min(4096));
        for index in 0..meta.len() {
            let e = meta.entry(index).map_err(zip_error)?;
            let (dos_date, dos_time) = e.last_modified().map_or((0, 0), |t| (t.datepart(), t.timepart()));
            out.push(RawEntry {
                raw_name: e.name_raw().to_vec(),
                flags: e.flags().as_u16(),
                method: method_code(e.compression()),
                dos_date,
                dos_time,
                crc32: e.crc32(),
                compressed_size: e.compressed_size(),
                size: e.size(),
                header_offset: e.header_start(),
                external_attributes: e.external_attributes(),
            });
        }
        Ok(out)
    }));
    attempt.unwrap_or_else(|_| Err(ArchiveError::malformed("the zip reader panicked on this archive")))
}

#[expect(
    deprecated,
    reason = "the zip crate offers no other way to get the raw method code; with its compression features off every method but stored is `Unsupported(code)`"
)]
const fn method_code(method: zip::CompressionMethod) -> u16 {
    method.to_u16()
}

fn zip_error(error: zip::result::ZipError) -> ArchiveError {
    match error {
        // A short read means the file is cut off or lies about its layout: damage, not a failing disk.
        zip::result::ZipError::Io(e) if matches!(e.kind(), std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::InvalidData) => {
            ArchiveError::malformed(e.to_string())
        }
        zip::result::ZipError::Io(e) => ArchiveError::io("reading the zip directory", e),
        other => ArchiveError::malformed(other.to_string()),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Strategy 2: tolerant central directory

/// The fields of the End Of Central Directory record that matter here.
#[derive(Debug, Clone, Copy)]
struct Eocd {
    /// Absolute position of the EOCD signature.
    position: u64,
    cd_offset: u64,
    cd_size: u64,
}

fn read_exact_at(reader: &mut dyn ReadSeek, at: u64, count: usize) -> Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(at)).map_err(|e| ArchiveError::io("seeking in the archive", e))?;
    let mut buf = vec![0_u8; count];
    reader
        .read_exact(&mut buf)
        .map_err(|e| if e.kind() == std::io::ErrorKind::UnexpectedEof { ArchiveError::truncated() } else { ArchiveError::io("reading the archive", e) })?;
    Ok(buf)
}

fn find_eocd(reader: &mut dyn ReadSeek, len: u64) -> Result<Eocd> {
    let tail_len = len.min(EOCD_SEARCH);
    let tail_start = len.saturating_sub(tail_len);
    let tail = read_exact_at(reader, tail_start, usize::try_from(tail_len).map_err(|_| ArchiveError::truncated())?)?;
    // Prefer an EOCD whose comment length reaches exactly to the end of the file; otherwise take the last signature.
    let mut candidate: Option<usize> = None;
    let mut search_end = tail.len();
    while let Some(window) = tail.get(..search_end) {
        let Some(at) = bytes::rfind(window, &SIG_EOCD) else { break };
        let comment_len = le_u16(&tail, at.saturating_add(20)).map(usize::from);
        let exact = comment_len.is_some_and(|c| at.saturating_add(EOCD_LEN).saturating_add(c) == tail.len());
        if exact {
            candidate = Some(at);
            break;
        }
        if candidate.is_none() {
            candidate = Some(at);
        }
        search_end = at;
    }
    let at = candidate.ok_or_else(|| ArchiveError::malformed("no end-of-central-directory record"))?;
    let record = tail.get(at..).ok_or_else(ArchiveError::truncated)?;
    let mut c = Cursor::new(record);
    c.skip(4)?;
    let (_disk, _cd_disk, _entries_here, entries_total) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?);
    let cd_size = u64::from(c.u32()?);
    let cd_offset = u64::from(c.u32()?);
    let position = add(tail_start, u64::try_from(at).map_err(|_| ArchiveError::truncated())?)?;
    let mut eocd = Eocd { position, cd_offset, cd_size };
    let needs_zip64 = cd_offset == u64::from(U32_MAX_MARK) || cd_size == u64::from(U32_MAX_MARK) || entries_total == u16::MAX;
    if needs_zip64 {
        // The Zip64 locator sits immediately before the EOCD and points at the Zip64 EOCD record.
        if let Some(locator_at) = position.checked_sub(20) {
            let locator = read_exact_at(reader, locator_at, 20)?;
            if locator.get(..4) == Some(&SIG_LOCATOR64) {
                let record_at = le_u64(&locator, 8).ok_or_else(ArchiveError::truncated)?;
                let record = read_exact_at(reader, record_at, 56)?;
                if record.get(..4) == Some(&SIG_EOCD64) {
                    eocd.cd_size = le_u64(&record, 40).ok_or_else(ArchiveError::truncated)?;
                    eocd.cd_offset = le_u64(&record, 48).ok_or_else(ArchiveError::truncated)?;
                }
            }
        }
    }
    Ok(eocd)
}

/// Reads Zip64 extended information, replacing the 32-bit fields that hold the `0xFFFFFFFF` marker.
fn apply_zip64_extra(
    extra: &[u8],
    size: &mut u64,
    compressed: &mut u64,
    header_offset: Option<&mut u64>,
    size_marked: bool,
    compressed_marked: bool,
    offset_marked: bool,
) {
    let mut c = Cursor::new(extra);
    let mut offset_slot = header_offset;
    while c.remaining() >= 4 {
        let (Ok(id), Ok(field_len)) = (c.u16(), c.u16()) else { return };
        let Ok(body) = c.take(usize::from(field_len)) else { return };
        if id != EXTRA_ZIP64 {
            continue;
        }
        let mut b = Cursor::new(body);
        if size_marked && let Ok(v) = b.u64() {
            *size = v;
        }
        if compressed_marked && let Ok(v) = b.u64() {
            *compressed = v;
        }
        if offset_marked && let (Ok(v), Some(slot)) = (b.u64(), offset_slot.as_deref_mut()) {
            *slot = v;
        }
        return;
    }
}

fn index_central_directory(reader: &mut dyn ReadSeek, len: u64, limits: &Limits, warnings: &mut Vec<String>) -> Result<Vec<RawEntry>> {
    let eocd = find_eocd(reader, len)?;
    // The declared offset first; then the position implied by the directory size (archives with a prefix).
    let implied = eocd.position.checked_sub(eocd.cd_size);
    let mut start = None;
    for candidate in [Some(eocd.cd_offset), implied].into_iter().flatten() {
        if candidate < len && read_exact_at(reader, candidate, 4).is_ok_and(|sig| sig == SIG_CENTRAL) {
            start = Some(candidate);
            break;
        }
    }
    let start = start.ok_or_else(|| ArchiveError::malformed("central directory not found where the end record says"))?;
    // Local header offsets are relative to where the archive starts; with a prefix that is `start - cd_offset`.
    let shift = start.saturating_sub(eocd.cd_offset);
    reader.seek(SeekFrom::Start(start)).map_err(|e| ArchiveError::io("seeking to the central directory", e))?;
    let mut buffered = BufReader::with_capacity(64 * 1024, &mut *reader);
    let mut entries = Vec::new();
    loop {
        let mut fixed = [0_u8; CENTRAL_HEADER_LEN];
        if buffered.read_exact(&mut fixed).is_err() || fixed.get(..4) != Some(&SIG_CENTRAL) {
            break;
        }
        let mut c = Cursor::new(&fixed);
        c.skip(4)?;
        let (_made, _needed, flags, method, time, date) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?, c.u16()?, c.u16()?);
        let crc32 = c.u32()?;
        let compressed_32 = c.u32()?;
        let size_32 = c.u32()?;
        let (name_len, extra_len, comment_len) = (usize::from(c.u16()?), usize::from(c.u16()?), usize::from(c.u16()?));
        let (_disk, _internal) = (c.u16()?, c.u16()?);
        let external_attributes = c.u32()?;
        let offset_32 = c.u32()?;
        let variable = name_len.saturating_add(extra_len).saturating_add(comment_len);
        let mut tail = vec![0_u8; variable];
        if buffered.read_exact(&mut tail).is_err() {
            warnings.push("central directory is cut short; later entries were dropped".to_owned());
            break;
        }
        let raw_name = tail.get(..name_len).ok_or_else(ArchiveError::truncated)?.to_vec();
        let extra = tail.get(name_len..name_len.saturating_add(extra_len)).ok_or_else(ArchiveError::truncated)?;
        let mut size = u64::from(size_32);
        let mut compressed_size = u64::from(compressed_32);
        let mut header_offset = u64::from(offset_32);
        apply_zip64_extra(
            extra,
            &mut size,
            &mut compressed_size,
            Some(&mut header_offset),
            size_32 == U32_MAX_MARK,
            compressed_32 == U32_MAX_MARK,
            offset_32 == U32_MAX_MARK,
        );
        entries.push(RawEntry {
            raw_name,
            flags,
            method,
            dos_date: date,
            dos_time: time,
            crc32,
            compressed_size,
            size,
            header_offset: add(header_offset, shift)?,
            external_attributes,
        });
        limits.check_entry_count(entries.len())?;
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------------------------------------------
// Strategy 3: scanning local headers

/// How many bytes of a stored entry with a data descriptor are searched for the descriptor signature at a time.
const SCAN_CHUNK: usize = 256 * 1024;

/// What the scan knows about the entry whose local header it just parsed.
struct LocalHeader {
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc32: u32,
    compressed_size: u64,
    size: u64,
    raw_name: Vec<u8>,
    data_start: u64,
}

fn read_local_header(reader: &mut dyn ReadSeek, at: u64, len: u64) -> Result<Option<LocalHeader>> {
    if add(at, LOCAL_HEADER_LEN)? > len {
        return Ok(None);
    }
    let fixed = read_exact_at(reader, at, 30)?;
    let mut c = Cursor::new(&fixed);
    c.skip(4)?;
    let (_needed, flags, method, time, date) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?, c.u16()?);
    let crc32 = c.u32()?;
    let compressed_32 = c.u32()?;
    let size_32 = c.u32()?;
    let (name_len, extra_len) = (usize::from(c.u16()?), usize::from(c.u16()?));
    let variable_len = u64::try_from(name_len.saturating_add(extra_len)).unwrap_or(u64::MAX);
    let data_start = add(add(at, LOCAL_HEADER_LEN)?, variable_len)?;
    if data_start > len {
        return Ok(None);
    }
    let variable = read_exact_at(reader, add(at, LOCAL_HEADER_LEN)?, name_len.saturating_add(extra_len))?;
    let raw_name = variable.get(..name_len).ok_or_else(ArchiveError::truncated)?.to_vec();
    let extra = variable.get(name_len..).ok_or_else(ArchiveError::truncated)?;
    let mut size = u64::from(size_32);
    let mut compressed_size = u64::from(compressed_32);
    apply_zip64_extra(extra, &mut size, &mut compressed_size, None, size_32 == U32_MAX_MARK, compressed_32 == U32_MAX_MARK, false);
    Ok(Some(LocalHeader { flags, method, time, date, crc32, compressed_size, size, raw_name, data_start }))
}

/// The outcome of measuring an entry whose sizes live in a data descriptor.
struct Measured {
    crc32: u32,
    compressed_size: u64,
    size: u64,
    /// Position of the first byte after the entry (including its descriptor).
    next: u64,
}

/// Inflates the deflate stream at `data_start` without keeping the output, to learn where it ends.
fn measure_deflated(reader: &mut dyn ReadSeek, data_start: u64, len: u64, limits: &Limits) -> Result<Option<Measured>> {
    reader.seek(SeekFrom::Start(data_start)).map_err(|e| ArchiveError::io("seeking in the archive", e))?;
    let mut input = BufReader::with_capacity(64 * 1024, &mut *reader);
    let mut inflater = Decompress::new(false);
    let mut hasher = crc32fast::Hasher::new();
    let mut sink = vec![0_u8; 64 * 1024];
    loop {
        let buf = input.fill_buf().map_err(|e| ArchiveError::io("reading the archive", e))?;
        if buf.is_empty() {
            return Ok(None);
        }
        let before_in = inflater.total_in();
        let before_out = inflater.total_out();
        let status =
            inflater.decompress(buf, &mut sink, FlushDecompress::None).map_err(|e| ArchiveError::malformed(format!("deflate stream is corrupt: {e}")))?;
        let used = usize::try_from(inflater.total_in().saturating_sub(before_in)).map_err(|_| ArchiveError::truncated())?;
        let produced = usize::try_from(inflater.total_out().saturating_sub(before_out)).map_err(|_| ArchiveError::truncated())?;
        hasher.update(sink.get(..produced).ok_or_else(ArchiveError::truncated)?);
        input.consume(used);
        limits.check_entry_size(inflater.total_out())?;
        if matches!(status, Status::StreamEnd) {
            break;
        }
        if used == 0 && produced == 0 {
            // No progress: the stream is truncated or the decoder is stuck.
            return Ok(None);
        }
    }
    let compressed_size = inflater.total_in();
    let end = add(data_start, compressed_size)?;
    if end > len {
        return Ok(None);
    }
    Ok(Some(Measured { crc32: hasher.finalize(), compressed_size, size: inflater.total_out(), next: end }))
}

/// Looks for a data descriptor at `at` that agrees with the measured values; returns its length (0 if absent).
fn descriptor_len(reader: &mut dyn ReadSeek, at: u64, len: u64, crc32: u32, compressed_size: u64, size: u64) -> Result<u64> {
    let available = usize::try_from(len.saturating_sub(at).min(32)).unwrap_or(32);
    if available == 0 {
        return Ok(0);
    }
    let buf = read_exact_at(reader, at, available)?;
    let (body, signature_len) = if buf.get(..4) == Some(&SIG_DESCRIPTOR) { (buf.get(4..).unwrap_or_default(), 4_u64) } else { (buf.as_slice(), 0_u64) };
    if le_u32(body, 0) != Some(crc32) {
        return Ok(0);
    }
    let small = le_u32(body, 4).map(u64::from) == Some(compressed_size) && le_u32(body, 8).map(u64::from) == Some(size);
    if small {
        return add(signature_len, 12);
    }
    let big = le_u64(body, 4) == Some(compressed_size) && le_u64(body, 12) == Some(size);
    if big {
        return add(signature_len, 20);
    }
    Ok(0)
}

/// Finds the end of a stored entry that has a data descriptor by searching for a descriptor whose sizes equal the
/// distance travelled so far (the only way to tell where stored data ends).
fn measure_stored_with_descriptor(reader: &mut dyn ReadSeek, data_start: u64, len: u64) -> Result<Option<Measured>> {
    /// A descriptor is its signature plus CRC and two 32-bit sizes.
    const DESCRIPTOR_LEN: usize = 16;
    let chunk_len = u64::try_from(SCAN_CHUNK).unwrap_or(u64::MAX);
    let mut from = data_start;
    while from < len {
        let remaining = len.saturating_sub(from);
        let want = usize::try_from(remaining.min(chunk_len)).unwrap_or(SCAN_CHUNK);
        let chunk = read_exact_at(reader, from, want)?;
        let mut search = 0_usize;
        while let Some(rel) = chunk.get(search..).and_then(|rest| bytes::find(rest, &SIG_DESCRIPTOR)) {
            let at = search.saturating_add(rel);
            search = at.saturating_add(1);
            let position = add(from, u64::try_from(at).unwrap_or(u64::MAX))?;
            let distance = position.saturating_sub(data_start);
            let Some(fields) = chunk.get(at.saturating_add(4)..) else { continue };
            let matches = le_u32(fields, 4).map(u64::from) == Some(distance) && le_u32(fields, 8).map(u64::from) == Some(distance);
            if let (Some(crc32), true) = (le_u32(fields, 0), matches) {
                return Ok(Some(Measured {
                    crc32,
                    compressed_size: distance,
                    size: distance,
                    next: add(position, u64::try_from(DESCRIPTOR_LEN).unwrap_or(16))?,
                }));
            }
        }
        if remaining <= chunk_len {
            break;
        }
        // Step back far enough that a descriptor straddling the chunk boundary is complete in the next chunk.
        let step = u64::try_from(want.saturating_sub(DESCRIPTOR_LEN.saturating_sub(1))).unwrap_or(1).max(1);
        from = add(from, step)?;
    }
    Ok(None)
}

/// Searches forward from `from` for the next local-header or central-directory signature.
fn resync(reader: &mut dyn ReadSeek, from: u64, len: u64) -> Result<Option<u64>> {
    let mut position = from;
    while position < len {
        let want = usize::try_from(len.saturating_sub(position).min(u64::try_from(SCAN_CHUNK).unwrap_or(u64::MAX))).unwrap_or(SCAN_CHUNK);
        let chunk = read_exact_at(reader, position, want)?;
        let local = bytes::find(&chunk, &SIG_LOCAL);
        let central = bytes::find(&chunk, &SIG_CENTRAL);
        let hit = match (local, central) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if let Some(rel) = hit {
            return Ok(Some(add(position, u64::try_from(rel).unwrap_or(0))?));
        }
        if want < 4 {
            break;
        }
        position = add(position, u64::try_from(want.saturating_sub(3)).unwrap_or(1).max(1))?;
    }
    Ok(None)
}

fn scan_local_headers(reader: &mut dyn ReadSeek, len: u64, limits: &Limits, warnings: &mut Vec<String>) -> Result<Vec<RawEntry>> {
    let mut entries: Vec<Option<RawEntry>> = Vec::new();
    let mut seen: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut position = 0_u64;
    while add(position, 4)? <= len {
        let signature = read_exact_at(reader, position, 4)?;
        if signature == SIG_LOCAL {
            let Some(header) = read_local_header(reader, position, len)? else {
                warnings.push(format!("entry at byte {position} is cut short; scan stopped"));
                break;
            };
            let descriptor = header.flags & FLAG_DESCRIPTOR != 0;
            // With a descriptor the local header usually holds zeros; trust it only when it was filled in anyway.
            let sizes_unknown = descriptor && (header.compressed_size == 0 || header.crc32 == 0);
            let (crc32, compressed_size, size, next) = if sizes_unknown {
                let measured = match header.method {
                    8 => measure_deflated(reader, header.data_start, len, limits)?,
                    0 => measure_stored_with_descriptor(reader, header.data_start, len)?,
                    _ => None,
                };
                let Some(m) = measured else {
                    warnings.push(format!("entry at byte {position} has a data descriptor that could not be located; scan stopped"));
                    break;
                };
                let tail = if m.next == add(header.data_start, m.compressed_size)? && header.method == 8 {
                    descriptor_len(reader, m.next, len, m.crc32, m.compressed_size, m.size)?
                } else {
                    0
                };
                (m.crc32, m.compressed_size, m.size, add(m.next, tail)?)
            } else {
                let end = add(header.data_start, header.compressed_size)?;
                if end > len {
                    warnings.push(format!("entry at byte {position} is truncated; scan stopped"));
                    break;
                }
                let tail = if descriptor { descriptor_len(reader, end, len, header.crc32, header.compressed_size, header.size)? } else { 0 };
                (header.crc32, header.compressed_size, header.size, add(end, tail)?)
            };
            let name_is_dir = header.raw_name.last().is_some_and(|b| *b == b'/' || *b == b'\\');
            let record = RawEntry {
                raw_name: header.raw_name,
                flags: header.flags,
                method: header.method,
                dos_date: header.date,
                dos_time: header.time,
                crc32,
                compressed_size,
                size,
                header_offset: position,
                external_attributes: if name_is_dir { 0x10 } else { 0 },
            };
            // Animate saves incrementally and can leave a superseded copy of an entry behind (the FLA samples carry two
            // `mimetype` entries but list one). The later copy wins, at its own position, like a central directory
            // with one record per name.
            if let Some(earlier) = seen.insert(record.raw_name.clone(), entries.len()).and_then(|index| entries.get_mut(index)) {
                *earlier = None;
            }
            entries.push(Some(record));
            limits.check_entry_count(seen.len())?;
            position = next;
        } else if signature == SIG_CENTRAL || signature == SIG_EOCD || signature == SIG_EOCD64 || signature == SIG_LOCATOR64 || signature == SIG_DIGITAL {
            break;
        } else if signature == SIG_DESCRIPTOR {
            position = add(position, 4)?;
        } else {
            match resync(reader, add(position, 1)?, len)? {
                Some(next) => {
                    warnings.push(format!("skipped {} unrecognised bytes at byte {position}", next.saturating_sub(position)));
                    position = next;
                }
                None => break,
            }
        }
    }
    Ok(entries.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor as IoCursor;

    fn local(name: &str, data: &[u8], flags: u16, method: u16, crc: u32, csize: u32, size: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&SIG_LOCAL);
        v.extend_from_slice(&20_u16.to_le_bytes());
        v.extend_from_slice(&flags.to_le_bytes());
        v.extend_from_slice(&method.to_le_bytes());
        v.extend_from_slice(&[0, 0, 0x21, 0]);
        v.extend_from_slice(&crc.to_le_bytes());
        v.extend_from_slice(&csize.to_le_bytes());
        v.extend_from_slice(&size.to_le_bytes());
        v.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        v.extend_from_slice(&0_u16.to_le_bytes());
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn scan_recovers_stored_entries_without_a_directory() {
        let mut zip = local("a.txt", b"hello", 0, 0, crc32fast::hash(b"hello"), 5, 5);
        zip.extend(local("dir/b.txt", b"world!", 0, 0, crc32fast::hash(b"world!"), 6, 6));
        let len = u64::try_from(zip.len()).unwrap();
        let mut warnings = Vec::new();
        let entries = scan_local_headers(&mut IoCursor::new(zip), len, &Limits::default(), &mut warnings).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].raw_name, b"a.txt");
        assert_eq!(entries[1].size, 6);
    }

    #[test]
    fn scan_drops_a_truncated_last_entry() {
        let mut zip = local("a.txt", b"hello", 0, 0, crc32fast::hash(b"hello"), 5, 5);
        let second = local("b.txt", b"world!", 0, 0, 0, 6, 6);
        zip.extend_from_slice(second.get(..second.len() - 3).unwrap());
        let len = u64::try_from(zip.len()).unwrap();
        let mut warnings = Vec::new();
        let entries = scan_local_headers(&mut IoCursor::new(zip), len, &Limits::default(), &mut warnings).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(warnings.iter().any(|w| w.contains("truncated")));
    }

    #[test]
    fn scan_resynchronises_over_garbage() {
        let mut zip = local("a.txt", b"hello", 0, 0, crc32fast::hash(b"hello"), 5, 5);
        zip.extend_from_slice(b"garbage-between-entries");
        zip.extend(local("b.txt", b"xy", 0, 0, crc32fast::hash(b"xy"), 2, 2));
        let len = u64::try_from(zip.len()).unwrap();
        let mut warnings = Vec::new();
        let entries = scan_local_headers(&mut IoCursor::new(zip), len, &Limits::default(), &mut warnings).unwrap();
        assert_eq!(entries.len(), 2);
    }

    /// Runs the three strategies on the real sample archives (when `ANICRAFT_SAMPLES` is set) and checks that they
    /// agree on every entry: the tolerant and scanning strategies must reproduce what the central directory says.
    #[test]
    fn all_strategies_agree_on_sample_archives() {
        let Some(dir) = std::env::var_os("ANICRAFT_SAMPLES").map(std::path::PathBuf::from) else { return };
        let limits = Limits::default();
        let files = ["unicorn_4AE.fla", "derwood_green_choreo_short.fla", "Boy.puppet", "blood_cells.puppet", "chproj_zip/news_anchors.zip"];
        for name in files {
            let path = dir.join(name);
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            let open = || BufReader::new(std::fs::File::open(&path).unwrap());
            let key = |e: &RawEntry| (e.raw_name.clone(), e.method, e.crc32, e.compressed_size, e.size, e.header_offset);

            let mut warnings = Vec::new();
            let (strict, source) = index_with_zip_crate(&mut open(), &limits).unwrap();
            let tolerant = index_central_directory(&mut open(), meta.len(), &limits, &mut warnings).unwrap();
            let scanned = scan_local_headers(&mut open(), meta.len(), &limits, &mut warnings).unwrap();
            eprintln!("{name}: {} entries, strict source {source:?}, warnings {warnings:?}", strict.len());

            let describe = |entries: &[RawEntry]| {
                entries
                    .iter()
                    .map(|e| {
                        format!(
                            "{} m{} crc{:08x} c{} u{} @{}",
                            String::from_utf8_lossy(&e.raw_name),
                            e.method,
                            e.crc32,
                            e.compressed_size,
                            e.size,
                            e.header_offset
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let strict_keys: Vec<_> = strict.iter().map(key).collect();
            let strict_lines = describe(&strict);
            for (label, found) in [("tolerant central directory", &tolerant), ("local header scan", &scanned)] {
                let lines = describe(found);
                let only_found: Vec<_> = lines.iter().filter(|l| !strict_lines.contains(l)).collect();
                let only_strict: Vec<_> = strict_lines.iter().filter(|l| !lines.contains(l)).collect();
                assert!(
                    only_found.is_empty() && only_strict.is_empty(),
                    "{name} {label}: only in {label}: {only_found:?}; only in the zip crate's index: {only_strict:?}"
                );
                assert_eq!(found.iter().map(key).collect::<Vec<_>>(), strict_keys, "{name}: {label} order or values");
            }
        }
    }

    #[test]
    fn eocd_search_fails_cleanly_on_junk() {
        let junk = vec![7_u8; 100];
        let len = 100;
        assert!(find_eocd(&mut IoCursor::new(junk), len).is_err());
    }
}
