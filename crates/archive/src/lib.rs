//! Robust containers for the file formats of the `AniCraft` family: zipped FLA files, XFL folders, `.puppet` zips and
//! Character Animator project folders.
//!
//! # Reading
//!
//! [`Container::open`] takes a path and works out what it is: a directory becomes a folder container, anything else is
//! indexed as a zip. A container lists its [`Entry`] values (normalised `/` names plus the raw name bytes as stored),
//! tests for existence, and reads entries with size caps and CRC verification:
//!
//! ```no_run
//! use anim_archive::Container;
//!
//! # fn main() -> Result<(), anim_archive::ArchiveError> {
//! let container = Container::open("unicorn.fla")?;
//! for entry in container.files() {
//!     println!("{} ({} bytes)", entry.name, entry.size);
//! }
//! let document = container.read("DOMDocument.xml")?;
//! # let _ = document;
//! # Ok(())
//! # }
//! ```
//!
//! Lookup by name is exact first, then falls back to a case-insensitive, escape-insensitive and
//! normalisation-insensitive match (see [`names::lookup_key`]).
//!
//! ## Tolerant zip reading
//!
//! Real FLA files written by Animate declare a central directory 54 bytes longer than it is, and strict readers
//! (including the `zip` crate and `unzip`) refuse them. Indexing therefore tries, in order: the `zip` crate; a
//! tolerant walk of the central directory; a scan of the local file headers. [`Container::index_source`] says which
//! one succeeded and [`Container::warnings`] explains what was repaired.
//!
//! ## Names
//!
//! Names are UTF-8 when general-purpose flag bit 11 is set or the bytes are valid UTF-8; otherwise they are decoded
//! from a legacy code page chosen by [`encoding::decode_name`] (Windows-1255 and CP862 Hebrew, Windows-1252, CP437).
//! [`Entry::raw_name`] always keeps the original bytes so they can be written back unchanged. The [`names`] module
//! maps between XFL `href` text and entry names, including Animate's `&#060` style file-name escapes.
//!
//! # Writing
//!
//! [`ZipWriter`] and [`DirectoryWriter`] implement [`ArchiveWriter`]. The zip output is deterministic (fixed
//! timestamps, insertion order, UTF-8 names, `mimetype` stored first when requested, others deflated unless deflate
//! does not help). [`ContainerWriter`] picks zip or folder at run time, and [`Container::copy_to`] converts between
//! the two.
//!
//! # Safety
//!
//! Every archive is treated as hostile: [`Limits`] caps the entry count, per-entry and total size, expansion ratio
//! and name length; offsets are checked; overlapping entries are rejected; nothing here panics on malformed input.

mod bytes;
mod container;
mod dir;
pub mod encoding;
mod entry;
mod error;
mod limits;
mod lookup;
pub mod names;
mod writer;
mod zip_index;
mod zip_read;
mod zip_write;

pub use container::{Container, ContainerKind};
pub use dir::DirectoryWriter;
pub use encoding::NameEncoding;
pub use entry::{CompressionMethod, DosDateTime, Entry};
pub use error::{ArchiveError, Result};
pub use limits::Limits;
pub use writer::{ArchiveWriter, Compression, ContainerWriter, EntryName, NewEntry};
pub use zip_index::IndexSource;
pub use zip_write::{WriteOptions, XFL_MIMETYPE, Zip64, ZipWriter};
