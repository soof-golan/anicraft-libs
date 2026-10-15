//! Content-addressed bytes for bitmaps, sounds and video, kept outside the cloneable [`crate::Document`].
//!
//! Library items carry a [`MediaId`]; the bytes live in a [`MediaStore`] owned by the session next to the document.
//! Undo snapshots clone only the document, so they stay cheap however many hundred megabytes of media are loaded.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::SceneError;

/// Identifier of a blob in a [`MediaStore`]: a 64-bit FNV-1a hash of its content (probed forward on collision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MediaId(pub u64);

impl MediaId {
    /// The id a blob of `bytes` would get in an empty store.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = OFFSET;
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(PRIME);
        }
        Self(hash)
    }
}

/// Default cap on the total size of a store: 4 GiB.
pub const DEFAULT_MEDIA_LIMIT: u64 = 4 * 1024 * 1024 * 1024;

/// Content-addressed blob storage. Inserting equal bytes twice returns the same id and stores them once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaStore {
    blobs: BTreeMap<MediaId, Box<[u8]>>,
    total_bytes: u64,
    limit: u64,
}

impl Default for MediaStore {
    fn default() -> Self {
        Self::with_limit(DEFAULT_MEDIA_LIMIT)
    }
}

impl MediaStore {
    /// An empty store with the default size cap.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty store refusing to hold more than `limit` bytes in total.
    #[must_use]
    pub const fn with_limit(limit: u64) -> Self {
        Self { blobs: BTreeMap::new(), total_bytes: 0, limit }
    }

    /// Stores `bytes` and returns their id.
    ///
    /// # Errors
    /// [`SceneError::LimitExceeded`] when the store would grow past its cap.
    pub fn insert(&mut self, bytes: Vec<u8>) -> Result<MediaId, SceneError> {
        let mut id = MediaId::of(&bytes);
        loop {
            match self.blobs.get(&id) {
                Some(existing) if **existing == *bytes => return Ok(id),
                Some(_) => id = MediaId(id.0.wrapping_add(1)),
                None => break,
            }
        }
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let new_total = self.total_bytes.saturating_add(len);
        if new_total > self.limit {
            return Err(SceneError::LimitExceeded("media store size"));
        }
        self.total_bytes = new_total;
        self.blobs.insert(id, bytes.into_boxed_slice());
        Ok(id)
    }

    /// The bytes stored under `id`.
    #[must_use]
    pub fn get(&self, id: MediaId) -> Option<&[u8]> {
        self.blobs.get(&id).map(AsRef::as_ref)
    }

    /// True when `id` is stored.
    #[must_use]
    pub fn contains(&self, id: MediaId) -> bool {
        self.blobs.contains_key(&id)
    }

    /// Removes a blob, returning its bytes.
    pub fn remove(&mut self, id: MediaId) -> Option<Box<[u8]>> {
        let removed = self.blobs.remove(&id)?;
        self.total_bytes = self.total_bytes.saturating_sub(u64::try_from(removed.len()).unwrap_or(u64::MAX));
        Some(removed)
    }

    /// Number of blobs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    /// True when empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// Total bytes held.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Ids in ascending order.
    pub fn ids(&self) -> impl Iterator<Item = MediaId> + '_ {
        self.blobs.keys().copied()
    }

    /// Drops every blob not listed in `keep`, returning how many bytes were freed.
    pub fn retain_only(&mut self, keep: &[MediaId]) -> u64 {
        let before = self.total_bytes;
        self.blobs.retain(|id, _| keep.contains(id));
        self.total_bytes = self.blobs.values().fold(0_u64, |acc, b| acc.saturating_add(u64::try_from(b.len()).unwrap_or(u64::MAX)));
        before.saturating_sub(self.total_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_content_shares_one_blob() {
        let mut s = MediaStore::new();
        let a = s.insert(vec![1, 2, 3]).unwrap();
        let b = s.insert(vec![1, 2, 3]).unwrap();
        let c = s.insert(vec![9]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(s.len(), 2);
        assert_eq!(s.total_bytes(), 4);
        assert_eq!(s.get(a), Some(&[1, 2, 3][..]));
        assert_eq!(a, MediaId::of(&[1, 2, 3]));
    }

    #[test]
    fn limit_is_enforced_and_remove_frees() {
        let mut s = MediaStore::with_limit(4);
        let a = s.insert(vec![0; 3]).unwrap();
        assert_eq!(s.insert(vec![1; 2]), Err(SceneError::LimitExceeded("media store size")));
        assert_eq!(s.len(), 1);
        assert!(s.remove(a).is_some());
        assert_eq!(s.total_bytes(), 0);
        assert!(s.insert(vec![1; 2]).is_ok());
        assert!(s.remove(MediaId(7)).is_none());
    }

    #[test]
    fn retain_only_frees_the_rest() {
        let mut s = MediaStore::new();
        let a = s.insert(vec![1; 10]).unwrap();
        let _b = s.insert(vec![2; 5]).unwrap();
        assert_eq!(s.retain_only(&[a]), 5);
        assert_eq!(s.len(), 1);
        assert!(s.contains(a));
        assert!(!s.is_empty());
        assert_eq!(s.ids().count(), 1);
    }

    #[test]
    fn empty_blob_is_storable() {
        let mut s = MediaStore::new();
        let id = s.insert(Vec::new()).unwrap();
        assert_eq!(s.get(id), Some(&[][..]));
    }
}
