//! A deterministic zip writer.
//!
//! Output depends only on the entries added and the [`WriteOptions`]: no clock is read, entries keep insertion order,
//! names are written as UTF-8 with the language-encoding flag, and deflate output comes from a pure-Rust encoder, so
//! the same inputs always produce the same bytes. `mimetype` entries are always stored uncompressed, and
//! [`ZipWriter::with_mimetype`] writes one first, as the XFL/ODF family of formats expects.

use crate::encoding::decode_name;
use crate::entry::DosDateTime;
use crate::error::{ArchiveError, Result};
use crate::names::{is_safe_relative, normalize_name};
use crate::writer::{ArchiveWriter, Compression, EntryName, NewEntry};
use crate::zip_index::{SIG_CENTRAL, SIG_EOCD, SIG_EOCD64, SIG_LOCAL, SIG_LOCATOR64};
use flate2::write::DeflateEncoder;
use std::collections::HashSet;
use std::io::Write;

/// The `mimetype` content of an XFL/FLA document.
pub const XFL_MIMETYPE: &str = "application/vnd.adobe.xfl";

const FLAG_UTF8: u16 = 0x0800;
const MARK_32: u32 = u32::MAX;
const MARK_16: u16 = u16::MAX;

/// When to emit Zip64 structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Zip64 {
    /// Only when a size, offset or entry count needs it.
    #[default]
    Auto,
    /// Always (valid but larger; mostly for testing readers).
    Always,
}

/// Options for [`ZipWriter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    /// Default compression for entries that do not choose their own.
    pub compression: Compression,
    /// Deflate level 0 ..= 9.
    pub level: u32,
    /// Modification time given to entries without their own. The fixed default keeps output reproducible.
    pub modified: DosDateTime,
    /// Zip64 policy.
    pub zip64: Zip64,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self { compression: Compression::Auto, level: 6, modified: DosDateTime::EPOCH, zip64: Zip64::Auto }
    }
}

/// What the central directory needs to remember about a written entry.
#[derive(Debug)]
struct Record {
    name: Vec<u8>,
    flags: u16,
    method: u16,
    date: u16,
    time: u16,
    crc32: u32,
    compressed_size: u64,
    size: u64,
    header_offset: u64,
    external_attributes: u32,
    zip64_sizes: bool,
}

/// Writes a zip file to any [`Write`] sink (no seeking needed: every local header is written with final sizes).
#[derive(Debug)]
pub struct ZipWriter<W: Write> {
    inner: W,
    position: u64,
    records: Vec<Record>,
    names: HashSet<Vec<u8>>,
    options: WriteOptions,
}

fn len64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

fn put_u16(buf: &mut Vec<u8>, value: u16) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(buf: &mut Vec<u8>, value: u64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn clamp32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(MARK_32)
}

fn name_len16(name: &[u8]) -> Result<u16> {
    u16::try_from(name.len()).map_err(|_| ArchiveError::InvalidName(String::from_utf8_lossy(name).into_owned()))
}

impl<W: Write> ZipWriter<W> {
    /// A writer with the default options (auto compression, level 6, fixed 1980 timestamps).
    pub fn new(inner: W) -> Self {
        Self::with_options(inner, WriteOptions::default())
    }

    /// A writer with explicit options.
    pub fn with_options(inner: W, options: WriteOptions) -> Self {
        Self { inner, position: 0, records: Vec::new(), names: HashSet::new(), options }
    }

    /// A writer whose first entry is a stored `mimetype` file with the given content.
    ///
    /// # Errors
    ///
    /// Fails when the sink cannot be written.
    pub fn with_mimetype(inner: W, mimetype: &str, options: WriteOptions) -> Result<Self> {
        let mut writer = Self::with_options(inner, options);
        writer.add_entry(NewEntry::file("mimetype", mimetype.as_bytes()).with_compression(Compression::Stored))?;
        Ok(writer)
    }

    /// Number of entries written so far.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.records.len()
    }

    /// True before the first entry.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    fn put(&mut self, bytes: &[u8]) -> Result<()> {
        self.inner.write_all(bytes).map_err(|e| ArchiveError::io("writing the zip", e))?;
        self.position = self.position.checked_add(len64(bytes.len())).ok_or_else(|| ArchiveError::malformed("zip is too large"))?;
        Ok(())
    }

    fn resolve_name(entry: &NewEntry<'_>) -> Result<Vec<u8>> {
        let text = match entry.name {
            EntryName::Text(text) => text.to_owned(),
            EntryName::Raw(raw) => decode_name(raw, false).0,
        };
        let mut normalized = normalize_name(&text);
        // Reading cuts names at a NUL, so a name holding one could never be read back as written.
        if text.contains('\0') || !is_safe_relative(&normalized) {
            return Err(ArchiveError::InvalidName(text));
        }
        if entry.is_dir && !normalized.ends_with('/') {
            normalized.push('/');
        }
        let bytes = match entry.name {
            EntryName::Text(_) => normalized.into_bytes(),
            EntryName::Raw(raw) => {
                let mut bytes = raw.to_vec();
                if entry.is_dir && !bytes.ends_with(b"/") {
                    bytes.push(b'/');
                }
                bytes
            }
        };
        name_len16(&bytes)?;
        Ok(bytes)
    }

    fn compress(&self, entry: &NewEntry<'_>, name: &[u8]) -> Result<(u16, Vec<u8>)> {
        if entry.is_dir || name == b"mimetype" {
            return Ok((0, entry.data.to_vec()));
        }
        let policy = entry.compression.unwrap_or(self.options.compression);
        let deflated = || -> Result<Vec<u8>> {
            let mut encoder = DeflateEncoder::new(Vec::with_capacity(entry.data.len() / 2), flate2::Compression::new(self.options.level.min(9)));
            encoder.write_all(entry.data).map_err(|e| ArchiveError::io("compressing an entry", e))?;
            encoder.finish().map_err(|e| ArchiveError::io("compressing an entry", e))
        };
        match policy {
            Compression::Stored => Ok((0, entry.data.to_vec())),
            Compression::Deflate => Ok((8, deflated()?)),
            Compression::Auto => {
                if entry.data.is_empty() {
                    return Ok((0, Vec::new()));
                }
                let candidate = deflated()?;
                if candidate.len() < entry.data.len() { Ok((8, candidate)) } else { Ok((0, entry.data.to_vec())) }
            }
        }
    }

    fn write_entry(&mut self, entry: &NewEntry<'_>) -> Result<()> {
        let name = Self::resolve_name(entry)?;
        if self.names.contains(&name) {
            return Err(ArchiveError::DuplicateName(String::from_utf8_lossy(&name).into_owned()));
        }
        let (method, stored) = self.compress(entry, &name)?;
        let size = len64(entry.data.len());
        let compressed_size = len64(stored.len());
        let zip64_sizes = self.options.zip64 == Zip64::Always || size >= u64::from(MARK_32) || compressed_size >= u64::from(MARK_32);
        let flags = if std::str::from_utf8(&name).is_ok() { FLAG_UTF8 } else { 0 };
        let (date, time) = entry.modified.unwrap_or(self.options.modified).to_dos();
        let crc32 = crc32fast::hash(entry.data);
        let header_offset = self.position;

        let mut header = Vec::with_capacity(name.len().saturating_add(50));
        header.extend_from_slice(&SIG_LOCAL);
        put_u16(&mut header, if zip64_sizes { 45 } else { 20 });
        put_u16(&mut header, flags);
        put_u16(&mut header, method);
        put_u16(&mut header, time);
        put_u16(&mut header, date);
        put_u32(&mut header, crc32);
        put_u32(&mut header, if zip64_sizes { MARK_32 } else { clamp32(compressed_size) });
        put_u32(&mut header, if zip64_sizes { MARK_32 } else { clamp32(size) });
        put_u16(&mut header, name_len16(&name)?);
        put_u16(&mut header, if zip64_sizes { 20 } else { 0 });
        header.extend_from_slice(&name);
        if zip64_sizes {
            put_u16(&mut header, 1);
            put_u16(&mut header, 16);
            put_u64(&mut header, size);
            put_u64(&mut header, compressed_size);
        }
        self.put(&header)?;
        self.put(&stored)?;

        self.names.insert(name.clone());
        self.records.push(Record {
            name,
            flags,
            method,
            date,
            time,
            crc32,
            compressed_size,
            size,
            header_offset,
            external_attributes: if entry.is_dir { 0x10 } else { 0 },
            zip64_sizes,
        });
        Ok(())
    }

    fn write_central_record(&mut self, index: usize) -> Result<()> {
        let record = self.records.get(index).ok_or_else(|| ArchiveError::malformed("record index out of range"))?;
        let offset_needs_zip64 = record.header_offset >= u64::from(MARK_32);
        let mut extra = Vec::new();
        if record.zip64_sizes || offset_needs_zip64 {
            let mut body = Vec::new();
            if record.zip64_sizes {
                put_u64(&mut body, record.size);
                put_u64(&mut body, record.compressed_size);
            }
            if offset_needs_zip64 {
                put_u64(&mut body, record.header_offset);
            }
            put_u16(&mut extra, 1);
            put_u16(&mut extra, u16::try_from(body.len()).unwrap_or(0));
            extra.extend_from_slice(&body);
        }
        let zip64 = !extra.is_empty();
        let mut buf = Vec::with_capacity(record.name.len().saturating_add(extra.len()).saturating_add(46));
        buf.extend_from_slice(&SIG_CENTRAL);
        put_u16(&mut buf, if zip64 { 45 } else { 20 });
        put_u16(&mut buf, if zip64 { 45 } else { 20 });
        put_u16(&mut buf, record.flags);
        put_u16(&mut buf, record.method);
        put_u16(&mut buf, record.time);
        put_u16(&mut buf, record.date);
        put_u32(&mut buf, record.crc32);
        put_u32(&mut buf, if record.zip64_sizes { MARK_32 } else { clamp32(record.compressed_size) });
        put_u32(&mut buf, if record.zip64_sizes { MARK_32 } else { clamp32(record.size) });
        put_u16(&mut buf, name_len16(&record.name)?);
        put_u16(&mut buf, u16::try_from(extra.len()).unwrap_or(0));
        put_u16(&mut buf, 0);
        put_u16(&mut buf, 0);
        put_u16(&mut buf, 0);
        put_u32(&mut buf, record.external_attributes);
        put_u32(&mut buf, if offset_needs_zip64 { MARK_32 } else { clamp32(record.header_offset) });
        buf.extend_from_slice(&record.name);
        buf.extend_from_slice(&extra);
        self.put(&buf)
    }

    fn write_end(&mut self, directory_offset: u64) -> Result<()> {
        let count = len64(self.records.len());
        let directory_size = self.position.saturating_sub(directory_offset);
        let needs_zip64 = self.options.zip64 == Zip64::Always
            || count >= u64::from(MARK_16)
            || directory_size >= u64::from(MARK_32)
            || directory_offset >= u64::from(MARK_32);
        if needs_zip64 {
            let record_offset = self.position;
            let mut z = Vec::with_capacity(76);
            z.extend_from_slice(&SIG_EOCD64);
            put_u64(&mut z, 44);
            put_u16(&mut z, 45);
            put_u16(&mut z, 45);
            put_u32(&mut z, 0);
            put_u32(&mut z, 0);
            put_u64(&mut z, count);
            put_u64(&mut z, count);
            put_u64(&mut z, directory_size);
            put_u64(&mut z, directory_offset);
            z.extend_from_slice(&SIG_LOCATOR64);
            put_u32(&mut z, 0);
            put_u64(&mut z, record_offset);
            put_u32(&mut z, 1);
            self.put(&z)?;
        }
        let mut end = Vec::with_capacity(22);
        end.extend_from_slice(&SIG_EOCD);
        put_u16(&mut end, 0);
        put_u16(&mut end, 0);
        let count16 = u16::try_from(count).unwrap_or(MARK_16);
        put_u16(&mut end, if needs_zip64 { MARK_16 } else { count16 });
        put_u16(&mut end, if needs_zip64 { MARK_16 } else { count16 });
        put_u32(&mut end, if needs_zip64 { MARK_32 } else { clamp32(directory_size) });
        put_u32(&mut end, if needs_zip64 { MARK_32 } else { clamp32(directory_offset) });
        put_u16(&mut end, 0);
        self.put(&end)
    }
}

impl<W: Write> ArchiveWriter for ZipWriter<W> {
    type Output = W;

    fn add_entry(&mut self, entry: NewEntry<'_>) -> Result<()> {
        self.write_entry(&entry)
    }

    fn finish(mut self) -> Result<W> {
        let directory_offset = self.position;
        for index in 0..self.records.len() {
            self.write_central_record(index)?;
        }
        self.write_end(directory_offset)?;
        self.inner.flush().map_err(|e| ArchiveError::io("flushing the zip", e))?;
        Ok(self.inner)
    }
}

impl ZipWriter<Vec<u8>> {
    /// A writer that collects the zip in memory.
    #[must_use]
    pub fn in_memory() -> Self {
        Self::new(Vec::new())
    }
}
