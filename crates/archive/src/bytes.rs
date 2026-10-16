//! A bounds-checked little-endian cursor, so no zip parsing code needs indexing or slicing.

use crate::error::{ArchiveError, Result};

/// Reads little-endian integers and byte runs from a slice; every read is checked.
#[derive(Debug, Clone)]
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Starts at the beginning of `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Bytes not yet consumed.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Consumes `n` bytes.
    ///
    /// # Errors
    /// Truncated when fewer than `n` bytes remain.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(ArchiveError::truncated)?;
        let out = self.data.get(self.pos..end).ok_or_else(ArchiveError::truncated)?;
        self.pos = end;
        Ok(out)
    }

    /// Skips `n` bytes.
    ///
    /// # Errors
    /// Truncated when fewer than `n` bytes remain.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let bytes = self.take(N)?;
        <[u8; N]>::try_from(bytes).map_err(|_| ArchiveError::truncated())
    }

    /// A little-endian `u16`.
    ///
    /// # Errors
    /// Truncated when fewer than 2 bytes remain.
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// A little-endian `u32`.
    ///
    /// # Errors
    /// Truncated when fewer than 4 bytes remain.
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// A little-endian `u64`.
    ///
    /// # Errors
    /// Truncated when fewer than 8 bytes remain.
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }
}

/// Little-endian `u16` at `at`, or `None` when out of range.
#[must_use]
pub fn le_u16(data: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    Some(u16::from_le_bytes(<[u8; 2]>::try_from(data.get(at..end)?).ok()?))
}

/// Little-endian `u32` at `at`, or `None` when out of range.
#[must_use]
pub fn le_u32(data: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(<[u8; 4]>::try_from(data.get(at..end)?).ok()?))
}

/// Little-endian `u64` at `at`, or `None` when out of range.
#[must_use]
pub fn le_u64(data: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    Some(u64::from_le_bytes(<[u8; 8]>::try_from(data.get(at..end)?).ok()?))
}

/// Finds the last occurrence of `needle` in `haystack`.
#[must_use]
pub fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|window| window == needle)
}

/// Finds the first occurrence of `needle` in `haystack`.
#[must_use]
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_reads_little_endian_and_stops_at_the_end() {
        let data = [1, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 9];
        let mut c = Cursor::new(&data);
        assert_eq!(c.u16().unwrap(), 1);
        assert_eq!(c.u32().unwrap(), 2);
        assert_eq!(c.u64().unwrap(), 3);
        assert_eq!(c.remaining(), 1);
        assert_eq!(c.take(1).unwrap(), &[9]);
        assert!(c.u16().is_err());
        assert!(c.take(usize::MAX).is_err());
    }

    #[test]
    fn fixed_offset_reads_are_checked() {
        let data = [0x34, 0x12, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(le_u16(&data, 0), Some(0x1234));
        assert_eq!(le_u32(&data, 2), Some(0x1234_5678));
        assert_eq!(le_u32(&data, 3), None);
        assert_eq!(le_u16(&data, usize::MAX), None);
        assert_eq!(le_u64(&data, 0), None);
    }

    #[test]
    fn search_helpers_find_first_and_last() {
        let hay = b"abcabc";
        assert_eq!(find(hay, b"bc"), Some(1));
        assert_eq!(rfind(hay, b"bc"), Some(4));
        assert_eq!(find(hay, b"zz"), None);
        assert_eq!(find(hay, b""), None);
    }
}
