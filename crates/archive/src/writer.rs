//! The writing side shared by zips and folders: [`ArchiveWriter`], [`NewEntry`] and [`ContainerWriter`].

use crate::container::ContainerKind;
use crate::dir::DirectoryWriter;
use crate::entry::DosDateTime;
use crate::error::{ArchiveError, Result};
use crate::zip_write::{WriteOptions, ZipWriter};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// How an entry's bytes are stored in a zip. Folders ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    /// Deflate, but store the bytes instead when deflate would not make them smaller (already-compressed media).
    #[default]
    Auto,
    /// Never compress.
    Stored,
    /// Always deflate, even when that grows the entry.
    Deflate,
}

/// The name of an entry being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryName<'a> {
    /// Text; normalised, and written as UTF-8 with the language-encoding flag set.
    Text(&'a str),
    /// Exact bytes, written as given (the flag is set only when they are valid UTF-8). For writing back names that
    /// were read from a legacy code page without changing them.
    Raw(&'a [u8]),
}

/// An entry to add to an [`ArchiveWriter`].
#[derive(Debug, Clone, Copy)]
pub struct NewEntry<'a> {
    pub(crate) name: EntryName<'a>,
    pub(crate) text_name: &'a str,
    pub(crate) data: &'a [u8],
    pub(crate) is_dir: bool,
    pub(crate) compression: Option<Compression>,
    pub(crate) modified: Option<DosDateTime>,
}

impl<'a> NewEntry<'a> {
    /// A file called `name` holding `data`.
    #[must_use]
    pub const fn file(name: &'a str, data: &'a [u8]) -> Self {
        Self { name: EntryName::Text(name), text_name: name, data, is_dir: false, compression: None, modified: None }
    }

    /// A directory called `name`.
    #[must_use]
    pub const fn dir(name: &'a str) -> Self {
        Self { name: EntryName::Text(name), text_name: name, data: &[], is_dir: true, compression: None, modified: None }
    }

    /// Writes `raw` as the stored name bytes instead of the text name (zips only; folders use the text name).
    #[must_use]
    pub const fn with_raw_name(mut self, raw: &'a [u8]) -> Self {
        self.name = EntryName::Raw(raw);
        self
    }

    /// Overrides the writer's default compression for this entry.
    #[must_use]
    pub const fn with_compression(mut self, compression: Compression) -> Self {
        self.compression = Some(compression);
        self
    }

    /// Sets the modification time (zips only). `None` keeps the writer's fixed default.
    #[must_use]
    pub const fn with_modified(mut self, modified: Option<DosDateTime>) -> Self {
        self.modified = modified;
        self
    }
}

/// Something entries can be written to: a zip stream or a folder.
pub trait ArchiveWriter: Sized {
    /// What [`ArchiveWriter::finish`] hands back (the underlying writer for zips, nothing for folders).
    type Output;

    /// Adds one entry.
    ///
    /// # Errors
    ///
    /// Fails for an unsafe or duplicate name, or when the destination cannot be written.
    fn add_entry(&mut self, entry: NewEntry<'_>) -> Result<()>;

    /// Adds a file.
    ///
    /// # Errors
    ///
    /// As [`ArchiveWriter::add_entry`].
    fn add_file(&mut self, name: &str, data: &[u8]) -> Result<()> {
        self.add_entry(NewEntry::file(name, data))
    }

    /// Adds a directory.
    ///
    /// # Errors
    ///
    /// As [`ArchiveWriter::add_entry`].
    fn add_dir(&mut self, name: &str) -> Result<()> {
        self.add_entry(NewEntry::dir(name))
    }

    /// Completes the container (writes the zip central directory, flushes files).
    ///
    /// # Errors
    ///
    /// Fails when the destination cannot be written.
    fn finish(self) -> Result<Self::Output>;
}

/// A writer chosen at run time: save as a zip file or as a folder with one code path.
#[derive(Debug)]
pub enum ContainerWriter {
    /// Writing a zip file.
    Zip(ZipWriter<BufWriter<File>>),
    /// Writing a folder.
    Folder(DirectoryWriter),
}

impl ContainerWriter {
    /// Creates `path` as a zip file or a folder, according to `kind`.
    ///
    /// The destination is written in place. For crash-safe saves, write the zip somewhere temporary with
    /// [`ZipWriter::new`] on the temporary file and rename it over the target afterwards.
    ///
    /// # Errors
    ///
    /// Fails when the file or folder cannot be created.
    pub fn create(path: impl AsRef<Path>, kind: ContainerKind) -> Result<Self> {
        Self::create_with(path, kind, WriteOptions::default())
    }

    /// [`ContainerWriter::create`] with explicit zip options.
    ///
    /// # Errors
    ///
    /// Fails when the file or folder cannot be created.
    pub fn create_with(path: impl AsRef<Path>, kind: ContainerKind, options: WriteOptions) -> Result<Self> {
        let path = path.as_ref();
        match kind {
            ContainerKind::Zip => {
                let file = File::create(path).map_err(|e| ArchiveError::io(path.display().to_string(), e))?;
                Ok(Self::Zip(ZipWriter::with_options(BufWriter::new(file), options)))
            }
            ContainerKind::Folder => Ok(Self::Folder(DirectoryWriter::create(path)?)),
        }
    }
}

impl ArchiveWriter for ContainerWriter {
    type Output = ();

    fn add_entry(&mut self, entry: NewEntry<'_>) -> Result<()> {
        match self {
            Self::Zip(writer) => writer.add_entry(entry),
            Self::Folder(writer) => writer.add_entry(entry),
        }
    }

    fn finish(self) -> Result<()> {
        match self {
            Self::Zip(writer) => {
                let mut inner = writer.finish()?;
                std::io::Write::flush(&mut inner).map_err(|e| ArchiveError::io("flushing the zip file", e))
            }
            Self::Folder(writer) => writer.finish(),
        }
    }
}
