//! Folder containers: walking a folder into entries, reading its files, and [`DirectoryWriter`].

use crate::entry::Entry;
use crate::error::{ArchiveError, Result};
use crate::limits::Limits;
use crate::names::{escape_filename, is_safe_relative, normalize_name};
use crate::writer::{ArchiveWriter, EntryName, NewEntry};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The file-system name as bytes: exact on Unix, UTF-8 elsewhere.
fn os_bytes(name: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        std::os::unix::ffi::OsStrExt::as_bytes(name).to_vec()
    }
    #[cfg(not(unix))]
    {
        name.to_string_lossy().into_owned().into_bytes()
    }
}

/// Lists every file and folder below `root` (excluding `root` itself) as entries, sorted by name at each level,
/// parents before children. Symbolic links to folders are skipped (loops); symbolic links to files are followed.
pub fn walk(root: &Path, limits: &Limits) -> Result<(Vec<Entry>, Vec<String>)> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    walk_level(root, "", 0, limits, &mut entries, &mut warnings)?;
    Ok((entries, warnings))
}

fn walk_level(dir: &Path, prefix: &str, depth: usize, limits: &Limits, entries: &mut Vec<Entry>, warnings: &mut Vec<String>) -> Result<()> {
    if depth > limits.max_dir_depth {
        return Err(ArchiveError::LimitExceeded {
            what: "folder depth",
            limit: u64::try_from(limits.max_dir_depth).unwrap_or(u64::MAX),
            actual: u64::try_from(depth).unwrap_or(u64::MAX),
        });
    }
    let context = dir.display().to_string();
    let mut children: Vec<(std::ffi::OsString, PathBuf)> = Vec::new();
    for child in fs::read_dir(dir).map_err(|e| ArchiveError::io(context.clone(), e))? {
        let child = child.map_err(|e| ArchiveError::io(context.clone(), e))?;
        children.push((child.file_name(), child.path()));
        if children.len() > limits.max_entries {
            return Err(ArchiveError::LimitExceeded {
                what: "entry count",
                limit: u64::try_from(limits.max_entries).unwrap_or(u64::MAX),
                actual: u64::try_from(children.len()).unwrap_or(u64::MAX),
            });
        }
    }
    children.sort_by_key(|(name, _)| os_bytes(name));
    for (file_name, path) in children {
        let link = fs::symlink_metadata(&path).map_err(|e| ArchiveError::io(path.display().to_string(), e))?;
        let is_link = link.file_type().is_symlink();
        // Follow links to files; for anything else use the link's own metadata.
        let meta = if is_link { fs::metadata(&path).ok() } else { Some(link) };
        let Some(meta) = meta else {
            warnings.push(format!("skipped broken link {}", path.display()));
            continue;
        };
        let part = file_name.to_string_lossy();
        let name = if prefix.is_empty() { part.to_string() } else { format!("{prefix}{part}") };
        if meta.is_dir() {
            if is_link {
                warnings.push(format!("skipped link to a folder {}", path.display()));
                continue;
            }
            let dir_name = format!("{name}/");
            entries.push(Entry::from_dir(dir_name.clone(), os_bytes(&file_name), path.clone()));
            check_count(entries.len(), limits)?;
            walk_level(&path, &dir_name, depth.saturating_add(1), limits, entries, warnings)?;
        } else if meta.is_file() {
            entries.push(Entry::from_file(name, os_bytes(&file_name), meta.len(), path));
            check_count(entries.len(), limits)?;
        } else {
            warnings.push(format!("skipped special file {}", path.display()));
        }
    }
    Ok(())
}

fn check_count(count: usize, limits: &Limits) -> Result<()> {
    limits.check_entry_count(count)
}

/// Reads a whole file, refusing files larger than the per-entry limit.
pub fn read_file(path: &Path, limits: &Limits) -> Result<Vec<u8>> {
    let context = path.display().to_string();
    let file = fs::File::open(path).map_err(|e| ArchiveError::io(context.clone(), e))?;
    let size = file.metadata().map_err(|e| ArchiveError::io(context.clone(), e))?.len();
    limits.check_entry_size(size)?;
    let mut out = Vec::with_capacity(usize::try_from(size.min(64 * 1024 * 1024)).unwrap_or(0));
    // A file can grow between the size check and the read; the take() keeps the cap honest.
    file.take(limits.max_entry_size.saturating_add(1)).read_to_end(&mut out).map_err(|e| ArchiveError::io(context, e))?;
    limits.check_entry_size(u64::try_from(out.len()).unwrap_or(u64::MAX))?;
    Ok(out)
}

/// Writes entries into a folder, with the same API as [`crate::ZipWriter`].
///
/// Names are validated (no `..`, no absolute paths) and characters no platform allows in file names are escaped the
/// way Animate does (`<` becomes `&#060`), which is a no-op for names that already carry the escape.
#[derive(Debug)]
pub struct DirectoryWriter {
    root: PathBuf,
    written: HashSet<String>,
}

impl DirectoryWriter {
    /// Creates (or reuses) the folder `root`.
    ///
    /// # Errors
    ///
    /// Fails when the folder cannot be created.
    pub fn create(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|e| ArchiveError::io(root.display().to_string(), e))?;
        Ok(Self { root, written: HashSet::new() })
    }

    /// The folder being written.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn target(&self, name: &str) -> Result<(String, PathBuf)> {
        let normalized = normalize_name(name);
        if !is_safe_relative(&normalized) {
            return Err(ArchiveError::InvalidName(name.to_owned()));
        }
        let escaped = escape_filename(normalized.trim_end_matches('/'));
        let mut path = self.root.clone();
        for part in escaped.split('/') {
            path.push(part);
        }
        Ok((normalized, path))
    }
}

impl ArchiveWriter for DirectoryWriter {
    type Output = ();

    fn add_entry(&mut self, entry: NewEntry<'_>) -> Result<()> {
        let text = match entry.name {
            EntryName::Text(text) => text,
            EntryName::Raw(_) => entry.text_name,
        };
        let (key, path) = self.target(text)?;
        if !self.written.insert(key.clone()) {
            return Err(ArchiveError::DuplicateName(key));
        }
        let context = path.display().to_string();
        if entry.is_dir {
            fs::create_dir_all(&path).map_err(|e| ArchiveError::io(context, e))
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| ArchiveError::io(parent.display().to_string(), e))?;
            }
            fs::write(&path, entry.data).map_err(|e| ArchiveError::io(context, e))
        }
    }

    fn finish(self) -> Result<()> {
        Ok(())
    }
}
