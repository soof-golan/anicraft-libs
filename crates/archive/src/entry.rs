//! What the container knows about each entry.

use crate::encoding::NameEncoding;
use std::path::PathBuf;

/// How an entry's bytes are stored in the zip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompressionMethod {
    /// Stored as is (method 0). Also reported for directory-container files.
    Stored,
    /// Deflate (method 8).
    Deflate,
    /// Any other method code; reading such an entry fails with [`crate::ArchiveError::UnsupportedMethod`].
    Other(u16),
}

impl CompressionMethod {
    /// Maps a zip method code.
    #[must_use]
    pub const fn from_code(code: u16) -> Self {
        match code {
            0 => Self::Stored,
            8 => Self::Deflate,
            other => Self::Other(other),
        }
    }

    /// The zip method code.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::Stored => 0,
            Self::Deflate => 8,
            Self::Other(code) => code,
        }
    }
}

/// A calendar date and time in the MS-DOS format zips use (2-second resolution, 1980..=2107, no time zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DosDateTime {
    /// 1980 ..= 2107.
    pub year: u16,
    /// 1 ..= 12.
    pub month: u8,
    /// 1 ..= 31.
    pub day: u8,
    /// 0 ..= 23.
    pub hour: u8,
    /// 0 ..= 59.
    pub minute: u8,
    /// 0 ..= 58, even.
    pub second: u8,
}

impl DosDateTime {
    /// 1980-01-01 00:00:00, the earliest representable time and the writer's deterministic default.
    pub const EPOCH: Self = Self { year: 1980, month: 1, day: 1, hour: 0, minute: 0, second: 0 };

    /// Decodes the two 16-bit MS-DOS fields. `None` for out-of-range fields (zero date, month 13, ...).
    #[must_use]
    pub fn from_dos(date: u16, time: u16) -> Option<Self> {
        let day = u8::try_from(date & 0x1F).ok()?;
        let month = u8::try_from((date >> 5) & 0x0F).ok()?;
        let year = 1980_u16.checked_add(date >> 9)?;
        let second = u8::try_from((time & 0x1F).checked_mul(2)?).ok()?;
        let minute = u8::try_from((time >> 5) & 0x3F).ok()?;
        let hour = u8::try_from(time >> 11).ok()?;
        let valid = (1..=31).contains(&day) && (1..=12).contains(&month) && hour < 24 && minute < 60 && second < 60;
        valid.then_some(Self { year, month, day, hour, minute, second })
    }

    /// Encodes to the `(date, time)` 16-bit MS-DOS fields, clamping into the representable range.
    #[must_use]
    pub fn to_dos(self) -> (u16, u16) {
        let year = self.year.clamp(1980, 2107).saturating_sub(1980);
        let month = u16::from(self.month.clamp(1, 12));
        let day = u16::from(self.day.clamp(1, 31));
        let date = (year << 9) | (month << 5) | day;
        let time = (u16::from(self.hour.min(23)) << 11) | (u16::from(self.minute.min(59)) << 5) | u16::from(self.second.min(59) / 2);
        (date, time)
    }
}

impl Default for DosDateTime {
    fn default() -> Self {
        Self::EPOCH
    }
}

/// Where an entry's bytes live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// A zip local header at this byte offset of the archive (the data offset is resolved when reading).
    Zip { header_offset: u64 },
    /// A file in a folder container.
    File(PathBuf),
}

/// One file or directory in a container.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Entry {
    /// Normalised name: `/` separators, no leading `/`, trailing `/` for directories. Use [`Entry::is_safe`] before
    /// mapping it onto a real file system.
    pub name: String,
    /// The name exactly as stored in the container (zip name bytes, or the file-system name bytes), for writing back.
    pub raw_name: Vec<u8>,
    /// How [`Entry::raw_name`] was decoded into [`Entry::name`].
    pub name_encoding: NameEncoding,
    /// True for directory entries.
    pub is_dir: bool,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// Size of the stored (possibly compressed) bytes. Equals `size` for folder containers.
    pub compressed_size: u64,
    /// CRC-32 of the uncompressed bytes (0 for folder containers, which are not checksummed).
    pub crc32: u32,
    /// Storage method.
    pub method: CompressionMethod,
    /// True when the zip marks the entry as encrypted (reading it fails).
    pub encrypted: bool,
    /// Last-modified time when the container records a valid one.
    pub modified: Option<DosDateTime>,
    pub(crate) location: Location,
}

impl Entry {
    /// A file as a folder container reports it.
    pub(crate) const fn from_file(name: String, raw_name: Vec<u8>, size: u64, path: PathBuf) -> Self {
        Self {
            name,
            raw_name,
            name_encoding: NameEncoding::FileSystem,
            is_dir: false,
            size,
            compressed_size: size,
            crc32: 0,
            method: CompressionMethod::Stored,
            encrypted: false,
            modified: None,
            location: Location::File(path),
        }
    }

    /// A directory as a folder container reports it.
    pub(crate) fn from_dir(name: String, raw_name: Vec<u8>, path: PathBuf) -> Self {
        Self { is_dir: true, ..Self::from_file(name, raw_name, 0, path) }
    }

    /// True when the normalised name is a relative path that cannot escape its root.
    #[must_use]
    pub fn is_safe(&self) -> bool {
        crate::names::is_safe_relative(&self.name)
    }

    /// The name without a trailing `/`.
    #[must_use]
    pub fn path(&self) -> &str {
        self.name.trim_end_matches('/')
    }

    /// The last path component.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.path().rsplit('/').next().unwrap_or("")
    }

    /// The name with Animate's `&#NNN` file-name escapes decoded (see [`crate::names::unescape_filename`]).
    #[must_use]
    pub fn display_name(&self) -> String {
        crate::names::unescape_filename(&self.name).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_time_round_trips() {
        let t = DosDateTime { year: 2022, month: 4, day: 3, hour: 20, minute: 53, second: 34 };
        let (d, tm) = t.to_dos();
        assert_eq!(DosDateTime::from_dos(d, tm), Some(t));
        assert_eq!(DosDateTime::from_dos(0, 0), None);
        assert_eq!(DosDateTime::EPOCH.to_dos(), (0x0021, 0));
    }

    #[test]
    fn dos_time_clamps_instead_of_overflowing() {
        let t = DosDateTime { year: 3000, month: 99, day: 99, hour: 99, minute: 99, second: 99 };
        let (d, tm) = t.to_dos();
        let back = DosDateTime::from_dos(d, tm).unwrap();
        assert_eq!((back.year, back.month, back.day, back.hour, back.minute), (2107, 12, 31, 23, 59));
    }

    #[test]
    fn method_codes_round_trip() {
        for code in [0_u16, 8, 9, 12, 93] {
            assert_eq!(CompressionMethod::from_code(code).code(), code);
        }
        assert_eq!(CompressionMethod::from_code(8), CompressionMethod::Deflate);
    }
}
