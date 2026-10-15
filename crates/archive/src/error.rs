//! Typed errors for reading and writing containers.

use std::io;

/// Everything that can go wrong while opening, reading or writing a container.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ArchiveError {
    /// An operating system I/O call failed. `context` says which file or folder was involved.
    #[error("{context}: {source}")]
    Io {
        /// What was being accessed.
        context: String,
        /// The underlying error.
        source: io::Error,
    },
    /// No entry with that name exists in the container.
    #[error("no such entry: {0:?}")]
    NotFound(String),
    /// The path is neither a readable zip archive nor a folder.
    #[error("not a zip archive or a folder: {0}")]
    NotAnArchive(String),
    /// The zip structure is damaged in a way that no recovery strategy could repair.
    #[error("malformed archive: {0}")]
    Malformed(String),
    /// A configured safety limit was exceeded (see [`crate::Limits`]).
    #[error("limit exceeded: {what} is {actual}, the limit is {limit}")]
    LimitExceeded {
        /// Which limit.
        what: &'static str,
        /// The configured maximum.
        limit: u64,
        /// The offending value.
        actual: u64,
    },
    /// The entry uses a compression method other than stored or deflate.
    #[error("entry {name:?} uses unsupported compression method {method}")]
    UnsupportedMethod {
        /// Entry name.
        name: String,
        /// Zip method code.
        method: u16,
    },
    /// The entry is encrypted; encrypted entries are not supported.
    #[error("entry {name:?} is encrypted")]
    Encrypted {
        /// Entry name.
        name: String,
    },
    /// The decompressed bytes do not match the stored CRC-32.
    #[error("entry {name:?} is corrupt: stored CRC-32 {expected:08x}, computed {actual:08x}")]
    Checksum {
        /// Entry name.
        name: String,
        /// CRC-32 recorded in the archive.
        expected: u32,
        /// CRC-32 of the bytes that were read.
        actual: u32,
    },
    /// The entry decompressed to a different number of bytes than the archive claims.
    #[error("entry {name:?} is corrupt: expected {expected} bytes, found {actual}")]
    SizeMismatch {
        /// Entry name.
        name: String,
        /// Size recorded in the archive.
        expected: u64,
        /// Size actually produced.
        actual: u64,
    },
    /// An entry name is empty, absolute, or escapes the container root.
    #[error("invalid entry name {0:?}")]
    InvalidName(String),
    /// A writer was given the same entry name twice.
    #[error("duplicate entry name {0:?}")]
    DuplicateName(String),
    /// The entry bytes are not valid text.
    #[error("entry {name:?} is not valid text: {reason}")]
    Text {
        /// Entry name.
        name: String,
        /// Why decoding failed.
        reason: &'static str,
    },
}

impl ArchiveError {
    /// Wraps an [`io::Error`] with the path or description it concerns.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io { context: context.into(), source }
    }

    /// A [`ArchiveError::Malformed`] with the given description.
    pub fn malformed(what: impl Into<String>) -> Self {
        Self::Malformed(what.into())
    }

    /// The error for a record that ends before its fixed fields do.
    #[must_use]
    pub fn truncated() -> Self {
        Self::Malformed("unexpected end of data".to_owned())
    }
}

/// Result alias used across the crate.
pub type Result<T, E = ArchiveError> = std::result::Result<T, E>;
