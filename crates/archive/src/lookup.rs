//! Name lookup tables for a container's entries: exact, lenient (case/escape/Unicode-insensitive) and by directory.

use crate::entry::Entry;
use crate::names::{lookup_key, normalize_name};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::Hasher;

/// Hashes of every `/`-separated prefix of `key`, shortest first.
///
/// Directory membership is kept as hashes instead of strings so that an entry with a huge, deeply nested name costs
/// eight bytes per level rather than a copy of every prefix. A collision can only make a directory look like it
/// exists, which is harmless.
fn prefix_hashes(key: &str) -> Vec<u64> {
    let mut hasher = DefaultHasher::new();
    let mut out = Vec::new();
    for (index, part) in key.split('/').enumerate() {
        if index > 0 {
            hasher.write(b"/");
        }
        hasher.write(part.as_bytes());
        out.push(hasher.clone().finish());
    }
    out
}

/// Index over a fixed list of entries. Later entries win over earlier ones with the same name, as in zip readers.
#[derive(Debug, Default)]
pub struct NameIndex {
    exact: HashMap<String, usize>,
    folded: HashMap<String, usize>,
    keys: Vec<String>,
    dirs: HashSet<u64>,
}

impl NameIndex {
    pub fn build(entries: &[Entry]) -> Self {
        let mut index = Self::default();
        for (position, entry) in entries.iter().enumerate() {
            let key = lookup_key(&entry.name);
            index.exact.insert(entry.name.clone(), position);
            index.folded.insert(key.clone(), position);
            let hashes = prefix_hashes(&key);
            let parents = if entry.is_dir { hashes.len() } else { hashes.len().saturating_sub(1) };
            index.dirs.extend(hashes.into_iter().take(parents));
            index.keys.push(key);
        }
        index
    }

    /// The position of the entry called `name`: exact match first, then the lenient match.
    pub fn find(&self, name: &str) -> Option<usize> {
        let normal = normalize_name(name);
        if let Some(position) = self.exact.get(&normal) {
            return Some(*position);
        }
        let with_slash = format!("{}/", normal.trim_end_matches('/'));
        if let Some(position) = self.exact.get(&with_slash) {
            return Some(*position);
        }
        self.folded.get(&lookup_key(name)).copied()
    }

    /// True when `name` is a directory entry or a parent of some entry.
    pub fn is_dir(&self, name: &str) -> bool {
        prefix_hashes(&lookup_key(name)).last().is_some_and(|hash| self.dirs.contains(hash))
    }

    /// Positions of the entries whose lookup key lies below the directory `dir` (`""` is everything).
    pub fn below<'a>(&'a self, dir: &str) -> impl Iterator<Item = usize> + 'a {
        let key = lookup_key(dir);
        let prefix = if key.is_empty() { String::new() } else { format!("{key}/") };
        self.keys.iter().enumerate().filter(move |(_, candidate)| candidate.starts_with(prefix.as_str())).map(|(position, _)| position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file(name: &str) -> Entry {
        Entry::from_file(name.to_owned(), name.as_bytes().to_vec(), 0, PathBuf::new())
    }

    #[test]
    fn exact_lenient_and_directory_lookups() {
        let entries = vec![file("LIBRARY/M_body.xml"), file("bin/deep/x.dat"), Entry::from_dir("empty/".to_owned(), b"empty".to_vec(), PathBuf::new())];
        let index = NameIndex::build(&entries);
        assert_eq!(index.find("LIBRARY/M_body.xml"), Some(0));
        assert_eq!(index.find("library/m_BODY.xml"), Some(0));
        assert_eq!(index.find("empty"), Some(2));
        assert_eq!(index.find("nothing"), None);
        assert!(index.is_dir("bin") && index.is_dir("BIN/deep") && index.is_dir("empty") && index.is_dir("library"));
        assert!(!index.is_dir("bin/deep/x.dat") && !index.is_dir("bi") && !index.is_dir("") && !index.is_dir("nothing"));
        assert_eq!(index.below("").collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(index.below("bin").collect::<Vec<_>>(), [1]);
        assert_eq!(index.below("bi").count(), 0);
    }

    #[test]
    fn later_entries_win_for_duplicate_names() {
        let entries = vec![file("a.txt"), file("A.TXT"), file("a.txt")];
        let index = NameIndex::build(&entries);
        assert_eq!(index.find("a.txt"), Some(2));
        assert_eq!(index.find("A.TXT"), Some(1));
        assert_eq!(index.find("a.Txt"), Some(2));
    }

    #[test]
    fn a_hugely_nested_name_costs_little() {
        let name = format!("{}f", "d/".repeat(30_000));
        let entries = vec![file(&name)];
        let index = NameIndex::build(&entries);
        assert!(index.is_dir(&"d/".repeat(15_000)));
        assert_eq!(index.find(&name), Some(0));
    }
}
