//! The one 64-bit FNV-1a.
//!
//! For content ids and fingerprints that must stay the same across runs, machines and versions, which the hashers in
//! `std` do not promise.

use std::hash::Hasher;

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// An incremental 64-bit FNV-1a hash.
///
/// Feed it with [`Hasher::write`] and read it with [`Hasher::finish`], which does not consume the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a(u64);

impl Fnv1a {
    /// A hash that has seen no bytes.
    #[must_use]
    pub const fn new() -> Self {
        Self(OFFSET)
    }

    /// The hash of `bytes` alone.
    #[must_use]
    pub fn of(bytes: &[u8]) -> u64 {
        let mut hasher = Self::new();
        hasher.write(bytes);
        hasher.finish()
    }
}

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for Fnv1a {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(PRIME);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_published_test_vectors() {
        assert_eq!(Fnv1a::of(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(Fnv1a::of(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(Fnv1a::of(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn writing_in_pieces_equals_writing_at_once() {
        let mut pieces = Fnv1a::new();
        pieces.write(b"foo");
        pieces.write(b"bar");
        assert_eq!(pieces.finish(), Fnv1a::of(b"foobar"));
        assert_eq!(pieces.finish(), pieces.finish(), "finish leaves the state alone");
    }
}
