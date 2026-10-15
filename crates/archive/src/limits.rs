//! Caps that protect against hostile or damaged archives (zip bombs, endless folder trees).

use crate::error::{ArchiveError, Result};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const GIB: u64 = 1024 * MIB;

/// Safety limits applied while indexing and reading a container.
///
/// The defaults are generous enough for real projects (a 100 MB FLA, a Character Animator project with hundreds of
/// recordings) and still bound memory use against hostile input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum number of entries in one container.
    pub max_entries: usize,
    /// Maximum uncompressed size of a single entry that [`crate::Container::read`] will produce.
    pub max_entry_size: u64,
    /// Maximum of all declared uncompressed entry sizes added together. Checked when the container is opened.
    pub max_total_size: u64,
    /// Maximum uncompressed-to-compressed ratio of a deflated entry. Deflate cannot exceed about 1032:1, so anything
    /// beyond the default is a forged size or a bomb. Only applied to entries larger than `ratio_min_size`.
    pub max_ratio: u64,
    /// Entries at or below this uncompressed size are exempt from the ratio check.
    pub ratio_min_size: u64,
    /// Maximum folder depth when walking a directory container.
    pub max_dir_depth: usize,
    /// Maximum length of one entry name in bytes.
    pub max_name_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 250_000,
            max_entry_size: 2 * GIB,
            max_total_size: 64 * GIB,
            max_ratio: 1100,
            ratio_min_size: MIB,
            max_dir_depth: 64,
            max_name_len: 4096,
        }
    }
}

impl Limits {
    /// Tight limits for untrusted input of modest size (thumbnails, previews, fuzzing).
    #[must_use]
    pub const fn small() -> Self {
        Self {
            max_entries: 10_000,
            max_entry_size: 64 * MIB,
            max_total_size: 256 * MIB,
            max_ratio: 1100,
            ratio_min_size: MIB,
            max_dir_depth: 32,
            max_name_len: 4096,
        }
    }

    pub(crate) fn check_entry_count(&self, count: usize) -> Result<()> {
        if count > self.max_entries {
            return Err(ArchiveError::LimitExceeded {
                what: "entry count",
                limit: u64::try_from(self.max_entries).unwrap_or(u64::MAX),
                actual: u64::try_from(count).unwrap_or(u64::MAX),
            });
        }
        Ok(())
    }

    pub(crate) const fn check_entry_size(&self, size: u64) -> Result<()> {
        if size > self.max_entry_size {
            return Err(ArchiveError::LimitExceeded { what: "entry size", limit: self.max_entry_size, actual: size });
        }
        Ok(())
    }

    /// Rejects a deflated entry whose declared size is implausible for its compressed size.
    pub(crate) fn check_ratio(&self, size: u64, compressed: u64) -> Result<()> {
        if size <= self.ratio_min_size {
            return Ok(());
        }
        let allowed = compressed.saturating_mul(self.max_ratio);
        if size > allowed {
            return Err(ArchiveError::LimitExceeded {
                what: "compression ratio (uncompressed bytes per compressed byte)",
                limit: self.max_ratio,
                actual: size.checked_div(compressed).unwrap_or(u64::MAX),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_check_exempts_small_entries_and_flags_bombs() {
        let limits = Limits::default();
        assert!(limits.check_ratio(1000, 1).is_ok());
        assert!(limits.check_ratio(10 * 1024 * 1024, 20_000).is_ok());
        assert!(limits.check_ratio(10 * 1024 * 1024, 100).is_err());
        assert!(limits.check_ratio(u64::MAX, 0).is_err());
    }

    #[test]
    fn entry_count_and_size_are_capped() {
        let limits = Limits { max_entries: 3, max_entry_size: 10, ..Limits::default() };
        assert!(limits.check_entry_count(3).is_ok());
        assert!(limits.check_entry_count(4).is_err());
        assert!(limits.check_entry_size(10).is_ok());
        assert!(limits.check_entry_size(11).is_err());
    }
}
