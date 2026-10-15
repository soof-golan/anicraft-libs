//! [`Container`]: one read-only view over a zip file, an in-memory zip, or a folder.

use crate::dir;
use crate::encoding::decode_name;
use crate::entry::{CompressionMethod, DosDateTime, Entry, Location};
use crate::error::{ArchiveError, Result};
use crate::limits::Limits;
use crate::lookup::NameIndex;
use crate::names::normalize_name;
use crate::writer::{ArchiveWriter, NewEntry};
use crate::zip_index::{self, IndexSource, RawEntry, ReadSeek};
use crate::zip_read;
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};

/// What a container is stored as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    /// A zip archive (FLA, `.puppet`, zipped project).
    Zip,
    /// A folder (XFL folder, Character Animator project folder).
    Folder,
}

#[derive(Debug)]
enum Source {
    File { path: PathBuf, len: u64 },
    Memory(Vec<u8>),
    Folder(PathBuf),
}

/// Read access to the entries of an archive or folder.
///
/// Open one with [`Container::open`]; it figures out whether the path is a zip or a folder. Reading is `&self` and
/// opens its own file handle per call, so a container can be shared across threads that only read.
///
/// ```no_run
/// # fn main() -> Result<(), anim_archive::ArchiveError> {
/// let container = anim_archive::Container::open("unicorn.fla")?;
/// let document = container.read_text("DOMDocument.xml")?;
/// # let _ = document;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Container {
    source: Source,
    index_source: IndexSource,
    entries: Vec<Entry>,
    names: NameIndex,
    warnings: Vec<String>,
    limits: Limits,
}

impl Container {
    /// Opens a zip file or a folder with the default [`Limits`].
    ///
    /// A path to a small non-zip file named `*.xfl` (the marker file inside an XFL folder) opens that file's folder.
    ///
    /// # Errors
    ///
    /// Fails when the path cannot be read, is not a zip that any recovery strategy can index, or exceeds a limit.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, Limits::default())
    }

    /// [`Container::open`] with explicit limits.
    ///
    /// # Errors
    ///
    /// As [`Container::open`].
    pub fn open_with(path: impl AsRef<Path>, limits: Limits) -> Result<Self> {
        let path = path.as_ref();
        let context = path.display().to_string();
        let meta = std::fs::metadata(path).map_err(|e| ArchiveError::io(context.clone(), e))?;
        if meta.is_dir() {
            return Self::open_folder(path, limits);
        }
        if let Some(folder) = xfl_marker_folder(path, meta.len())? {
            return Self::open_folder(&folder, limits);
        }
        let file = File::open(path).map_err(|e| ArchiveError::io(context.clone(), e))?;
        let len = meta.len();
        let mut reader = BufReader::new(file);
        let index = zip_index::build_index(&mut reader, len, &limits).map_err(|e| match e {
            ArchiveError::Malformed(reason) => ArchiveError::NotAnArchive(format!("{context} ({reason})")),
            other => other,
        })?;
        Self::assemble(Source::File { path: path.to_path_buf(), len }, index, limits)
    }

    /// Opens a folder container explicitly (no zip detection).
    ///
    /// # Errors
    ///
    /// Fails when the folder cannot be read or holds more than the configured entries or depth.
    pub fn open_folder(path: impl AsRef<Path>, limits: Limits) -> Result<Self> {
        let path = path.as_ref();
        let (entries, warnings) = dir::walk(path, &limits)?;
        Ok(Self { source: Source::Folder(path.to_path_buf()), index_source: IndexSource::Folder, names: NameIndex::build(&entries), entries, warnings, limits })
    }

    /// Opens a zip held in memory (embedded resources, network downloads, tests).
    ///
    /// # Errors
    ///
    /// As [`Container::open`] for the zip structure.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::from_bytes_with(bytes, Limits::default())
    }

    /// [`Container::from_bytes`] with explicit limits.
    ///
    /// # Errors
    ///
    /// As [`Container::open`] for the zip structure.
    pub fn from_bytes_with(bytes: Vec<u8>, limits: Limits) -> Result<Self> {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let index = zip_index::build_index(&mut Cursor::new(bytes.as_slice()), len, &limits).map_err(|e| match e {
            ArchiveError::Malformed(reason) => ArchiveError::NotAnArchive(format!("in-memory data ({reason})")),
            other => other,
        })?;
        Self::assemble(Source::Memory(bytes), index, limits)
    }

    fn assemble(source: Source, index: zip_index::Index, limits: Limits) -> Result<Self> {
        let len = match &source {
            Source::File { len, .. } => *len,
            Source::Memory(bytes) => u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            Source::Folder(_) => 0,
        };
        let mut warnings = index.warnings;
        let entries = finalize_zip_entries(index.entries, len, &limits, &mut warnings)?;
        Ok(Self { source, index_source: index.source, names: NameIndex::build(&entries), entries, warnings, limits })
    }

    /// Whether this is a zip or a folder.
    #[must_use]
    pub const fn kind(&self) -> ContainerKind {
        match self.source {
            Source::File { .. } | Source::Memory(_) => ContainerKind::Zip,
            Source::Folder(_) => ContainerKind::Folder,
        }
    }

    /// Which strategy indexed the container: [`IndexSource::ZipCrate`] for well-formed zips, a recovery strategy for
    /// damaged ones such as Animate's FLA files, [`IndexSource::Folder`] for folders.
    #[must_use]
    pub const fn index_source(&self) -> IndexSource {
        self.index_source
    }

    /// Notes about anything unusual found while indexing (recovery strategies used, entries dropped).
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// The path this container was opened from, if it came from the file system.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match &self.source {
            Source::File { path, .. } | Source::Folder(path) => Some(path),
            Source::Memory(_) => None,
        }
    }

    /// The limits in force.
    #[must_use]
    pub const fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Every entry, in the container's own order (zip order; sorted pre-order for folders).
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Entries that are files (not directories).
    pub fn files(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| !e.is_dir)
    }

    /// File entries below the directory `dir` at any depth (`""` means everything), matched case-insensitively.
    pub fn files_under<'a>(&'a self, dir: &str) -> impl Iterator<Item = &'a Entry> + 'a {
        self.names.below(dir).filter_map(|position| self.entries.get(position)).filter(|entry| !entry.is_dir)
    }

    /// Number of entries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when there are no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Looks an entry up by name: exact match on the normalised name first, then a lenient match that ignores case,
    /// Animate's `&#NNN` file-name escapes and Unicode normalisation form. A name for a directory may omit the `/`.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.names.find(name).and_then(|position| self.entries.get(position))
    }

    /// True when `name` is an entry or a directory implied by the entries below it (zips often omit directory
    /// entries). Lookup is as lenient as [`Container::find`].
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        self.find(name).is_some() || self.names.is_dir(name)
    }

    /// Reads and verifies the bytes of the entry called `name`.
    ///
    /// # Errors
    ///
    /// [`ArchiveError::NotFound`] when there is no such entry; otherwise whatever reading the entry fails with: a
    /// limit, corruption, an unsupported method, encryption or an I/O error.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self.find(name).ok_or_else(|| ArchiveError::NotFound(name.to_owned()))?;
        self.read_entry(entry)
    }

    /// Reads and verifies the bytes of `entry`, which must come from this container.
    ///
    /// # Errors
    ///
    /// As [`Container::read`] without the not-found case.
    pub fn read_entry(&self, entry: &Entry) -> Result<Vec<u8>> {
        match &entry.location {
            Location::File(path) => dir::read_file(path, &self.limits),
            Location::Zip { header_offset } => self.with_zip_reader(|reader, len| zip_read::read_entry(reader, len, *header_offset, entry, &self.limits)),
        }
    }

    /// Reads an entry as text: UTF-8 (a leading byte-order mark is removed) or UTF-16 with a byte-order mark.
    ///
    /// # Errors
    ///
    /// As [`Container::read`], or [`ArchiveError::Text`] when the bytes are not valid text.
    pub fn read_text(&self, name: &str) -> Result<String> {
        let bytes = self.read(name)?;
        decode_text(name, &bytes)
    }

    /// Copies every entry into `writer`, keeping raw names, directory entries and modification times.
    ///
    /// This is the conversion between container kinds: a zipped FLA into an XFL folder, or back.
    ///
    /// # Errors
    ///
    /// Fails on the first entry that cannot be read or written.
    pub fn copy_to<W: ArchiveWriter>(&self, writer: &mut W) -> Result<()> {
        for entry in &self.entries {
            if !entry.is_safe() {
                return Err(ArchiveError::InvalidName(entry.name.clone()));
            }
            let data = if entry.is_dir { Vec::new() } else { self.read_entry(entry)? };
            let mut new = if entry.is_dir { NewEntry::dir(&entry.name) } else { NewEntry::file(&entry.name, &data) };
            if !entry.name_encoding.is_unicode() {
                new = new.with_raw_name(&entry.raw_name);
            }
            writer.add_entry(new.with_modified(entry.modified))?;
        }
        Ok(())
    }

    fn with_zip_reader<T>(&self, f: impl FnOnce(&mut dyn ReadSeek, u64) -> Result<T>) -> Result<T> {
        match &self.source {
            Source::File { path, len } => {
                let file = File::open(path).map_err(|e| ArchiveError::io(path.display().to_string(), e))?;
                let mut reader = BufReader::new(file);
                f(&mut reader, *len)
            }
            Source::Memory(bytes) => {
                let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                f(&mut Cursor::new(bytes.as_slice()), len)
            }
            Source::Folder(path) => Err(ArchiveError::NotAnArchive(path.display().to_string())),
        }
    }
}

fn decode_text(name: &str, bytes: &[u8]) -> Result<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE
            .decode_without_bom_handling_and_without_replacement(rest)
            .map(std::borrow::Cow::into_owned)
            .ok_or_else(|| ArchiveError::Text { name: name.to_owned(), reason: "invalid UTF-16LE" });
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE
            .decode_without_bom_handling_and_without_replacement(rest)
            .map(std::borrow::Cow::into_owned)
            .ok_or_else(|| ArchiveError::Text { name: name.to_owned(), reason: "invalid UTF-16BE" });
    }
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8(body.to_vec()).map_err(|_| ArchiveError::Text { name: name.to_owned(), reason: "invalid UTF-8" })
}

/// A small `*.xfl` file that is not a zip is XFL's marker ("PROXY-CS5"); the document is the folder around it.
fn xfl_marker_folder(path: &Path, len: u64) -> Result<Option<PathBuf>> {
    let is_xfl = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xfl"));
    if !is_xfl || len > 4096 {
        return Ok(None);
    }
    let mut head = Vec::new();
    File::open(path).and_then(|f| f.take(4096).read_to_end(&mut head)).map_err(|e| ArchiveError::io(path.display().to_string(), e))?;
    if head.starts_with(b"PK") {
        return Ok(None);
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    Ok(Some(parent))
}

/// Applies limits and decodes names, turning raw zip records into [`Entry`] values.
fn finalize_zip_entries(raw: Vec<RawEntry>, len: u64, limits: &Limits, warnings: &mut Vec<String>) -> Result<Vec<Entry>> {
    limits.check_entry_count(raw.len())?;
    let mut total = 0_u64;
    let mut extents: Vec<(u64, u64)> = Vec::new();
    let mut entries = Vec::with_capacity(raw.len());
    for record in raw {
        if record.raw_name.len() > limits.max_name_len {
            return Err(ArchiveError::LimitExceeded {
                what: "entry name length",
                limit: u64::try_from(limits.max_name_len).unwrap_or(u64::MAX),
                actual: u64::try_from(record.raw_name.len()).unwrap_or(u64::MAX),
            });
        }
        let (decoded, name_encoding) = decode_name(&record.raw_name, record.flags & 0x0800 != 0);
        let name_is_dir = decoded.ends_with(['/', '\\']);
        let mut name = normalize_name(&decoded);
        if name.is_empty() {
            warnings.push(format!("skipped an entry with an empty name at byte {}", record.header_offset));
            continue;
        }
        let is_dir = name_is_dir || record.directory_attribute();
        if is_dir && !name.ends_with('/') {
            name.push('/');
        }
        let method = CompressionMethod::from_code(record.method);
        if !is_dir {
            if matches!(method, CompressionMethod::Deflate) {
                limits.check_ratio(record.size, record.compressed_size)?;
            }
            total = total.saturating_add(record.size);
            if total > limits.max_total_size {
                return Err(ArchiveError::LimitExceeded { what: "total uncompressed size", limit: limits.max_total_size, actual: total });
            }
            if record.compressed_size > 0 {
                // Lower bound of where this entry's data ends: header + stored bytes. Names and extras only add to it.
                let end = record.header_offset.saturating_add(zip_index::LOCAL_HEADER_LEN).saturating_add(record.compressed_size);
                extents.push((record.header_offset, end));
            }
        }
        entries.push(Entry {
            name,
            raw_name: record.raw_name.clone(),
            name_encoding,
            is_dir,
            size: if is_dir { 0 } else { record.size },
            compressed_size: if is_dir { 0 } else { record.compressed_size },
            crc32: record.crc32,
            method,
            encrypted: record.encrypted(),
            modified: DosDateTime::from_dos(record.dos_date, record.dos_time),
            location: Location::Zip { header_offset: record.header_offset },
        });
    }
    extents.sort_unstable();
    let overlapping = extents.windows(2).any(|pair| matches!(pair, [(_, end), (start, _)] if start < end));
    if overlapping {
        return Err(ArchiveError::malformed("entries overlap (shared or forged data ranges)"));
    }
    // Entries that point past the end of the file survive (they fail when read) but are worth a note.
    let beyond = entries.iter().filter(|e| matches!(e.location, Location::Zip { header_offset } if header_offset >= len)).count();
    if beyond > 0 {
        warnings.push(format!("{beyond} entries point past the end of the archive (truncated file?)"));
    }
    Ok(entries)
}
