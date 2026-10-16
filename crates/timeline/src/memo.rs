//! A bounded cache keyed by the address of an item of the document.

use std::collections::HashMap;

/// Values derived from document items, found again by the item's address.
///
/// The evaluator borrows its document, so items neither move nor change while it lives and an address names one item
/// for the cache's whole life. Never key by a temporary. The cache is emptied when it is full.
pub struct Memo<T> {
    entries: HashMap<usize, T>,
    limit: usize,
}

impl<T> Memo<T> {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self { entries: HashMap::new(), limit }
    }

    fn key<K: ?Sized>(item: &K) -> usize {
        std::ptr::from_ref(item).addr()
    }

    #[must_use]
    pub fn get<K: ?Sized>(&self, item: &K) -> Option<&T> {
        self.entries.get(&Self::key(item))
    }

    /// Takes the value out, for use while the evaluator is borrowed mutably; give it back with [`Memo::insert`].
    pub fn take<K: ?Sized>(&mut self, item: &K) -> Option<T> {
        self.entries.remove(&Self::key(item))
    }

    pub fn insert<K: ?Sized>(&mut self, item: &K, value: T) {
        if self.entries.len() >= self.limit {
            self.entries.clear();
        }
        self.entries.insert(Self::key(item), value);
    }

    /// The value for `item`, made by `make` the first time.
    pub fn get_or_insert_with<K: ?Sized>(&mut self, item: &K, make: impl FnOnce() -> T) -> &T {
        if self.entries.len() >= self.limit {
            self.entries.clear();
        }
        self.entries.entry(Self::key(item)).or_insert_with(make)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
