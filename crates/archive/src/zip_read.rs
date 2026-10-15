//! Reading one entry's bytes: locate the data behind its local header, then store/inflate it within the size caps.

use crate::bytes::Cursor;
use crate::entry::{CompressionMethod, Entry};
use crate::error::{ArchiveError, Result};
use crate::limits::Limits;
use crate::zip_index::{LOCAL_HEADER_LEN, ReadSeek, SIG_LOCAL};
use flate2::read::DeflateDecoder;
use std::io::{ErrorKind, Read, SeekFrom};

/// The most capacity reserved up front for a deflated entry; the real size is only trusted as it materialises.
const INITIAL_CAPACITY_CAP: u64 = 8 * 1024 * 1024;

/// Resolves where the data of the entry whose local header starts at `header_offset` begins.
///
/// The local header's own name and extra-field lengths are used (they may differ from the central directory's).
pub fn data_offset(reader: &mut dyn ReadSeek, header_offset: u64, len: u64) -> Result<u64> {
    let header_end = header_offset
        .checked_add(LOCAL_HEADER_LEN)
        .filter(|end| *end <= len)
        .ok_or_else(|| ArchiveError::malformed("local header lies outside the archive"))?;
    reader.seek(SeekFrom::Start(header_offset)).map_err(|e| ArchiveError::io("seeking to a local header", e))?;
    let mut fixed = [0_u8; 30];
    reader.read_exact(&mut fixed).map_err(|e| ArchiveError::io("reading a local header", e))?;
    let mut c = Cursor::new(&fixed);
    if c.take(4)? != SIG_LOCAL {
        return Err(ArchiveError::malformed(format!("no local file header at byte {header_offset}")));
    }
    c.skip(22)?;
    let name_len = u64::from(c.u16()?);
    let extra_len = u64::from(c.u16()?);
    let data =
        header_end.checked_add(name_len).and_then(|v| v.checked_add(extra_len)).ok_or_else(|| ArchiveError::malformed("local header lengths overflow"))?;
    Ok(data)
}

fn decode_io(name: &str, error: &std::io::Error) -> ArchiveError {
    match error.kind() {
        ErrorKind::InvalidData | ErrorKind::UnexpectedEof | ErrorKind::InvalidInput => ArchiveError::malformed(format!("entry {name:?} is corrupt: {error}")),
        _ => ArchiveError::io(format!("reading entry {name:?}"), std::io::Error::new(error.kind(), error.to_string())),
    }
}

/// Reads and verifies the bytes of a zip `entry` whose local header is at `header_offset`.
pub fn read_entry(reader: &mut dyn ReadSeek, len: u64, header_offset: u64, entry: &Entry, limits: &Limits) -> Result<Vec<u8>> {
    if entry.is_dir {
        return Ok(Vec::new());
    }
    if entry.encrypted {
        return Err(ArchiveError::Encrypted { name: entry.name.clone() });
    }
    limits.check_entry_size(entry.size)?;
    let start = data_offset(reader, header_offset, len)?;
    start
        .checked_add(entry.compressed_size)
        .filter(|end| *end <= len)
        .ok_or_else(|| ArchiveError::malformed(format!("entry {:?} runs past the end of the archive", entry.name)))?;
    reader.seek(SeekFrom::Start(start)).map_err(|e| ArchiveError::io("seeking to entry data", e))?;
    let source = Read::take(&mut *reader, entry.compressed_size);
    let out = match entry.method {
        CompressionMethod::Stored => read_stored(source, entry)?,
        CompressionMethod::Deflate => read_deflated(source, entry)?,
        CompressionMethod::Other(method) => return Err(ArchiveError::UnsupportedMethod { name: entry.name.clone(), method }),
    };
    let actual = crc32fast::hash(&out);
    if actual != entry.crc32 {
        return Err(ArchiveError::Checksum { name: entry.name.clone(), expected: entry.crc32, actual });
    }
    Ok(out)
}

fn read_stored(source: impl Read, entry: &Entry) -> Result<Vec<u8>> {
    if entry.size != entry.compressed_size {
        return Err(ArchiveError::SizeMismatch { name: entry.name.clone(), expected: entry.size, actual: entry.compressed_size });
    }
    let capacity = usize::try_from(entry.size.min(INITIAL_CAPACITY_CAP)).unwrap_or(0);
    let mut out = Vec::with_capacity(capacity);
    let mut limited = source.take(entry.size);
    limited.read_to_end(&mut out).map_err(|e| decode_io(&entry.name, &e))?;
    check_len(entry, &out)?;
    Ok(out)
}

fn read_deflated(source: impl Read, entry: &Entry) -> Result<Vec<u8>> {
    let capacity = usize::try_from(entry.size.min(INITIAL_CAPACITY_CAP)).unwrap_or(0);
    let mut out = Vec::with_capacity(capacity);
    // One byte more than declared, so a stream that expands past its claimed size is noticed (and stops there).
    let mut limited = DeflateDecoder::new(source).take(entry.size.saturating_add(1));
    limited.read_to_end(&mut out).map_err(|e| decode_io(&entry.name, &e))?;
    check_len(entry, &out)?;
    Ok(out)
}

fn check_len(entry: &Entry, out: &[u8]) -> Result<()> {
    let actual = u64::try_from(out.len()).unwrap_or(u64::MAX);
    if actual == entry.size { Ok(()) } else { Err(ArchiveError::SizeMismatch { name: entry.name.clone(), expected: entry.size, actual }) }
}
