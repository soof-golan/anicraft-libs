//! Read and write root confinement for everything an agent names by path.
//!
//! An agent may read files only under a read root (or a write root) and write only under a write root. Paths must be
//! absolute and free of `..`; symbolic links are resolved before the comparison, so a link inside a root that points
//! outside it does not escape. With no roots configured nothing is readable or writable; [`Confinement::unrestricted`]
//! opts out for trusted local use.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::protocol::{ControlError, ErrorKind};

/// Deepest JSON nesting scanned for path parameters.
const MAX_SCAN_DEPTH: usize = 16;

/// What a path is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// The file is read.
    Read,
    /// The file is created or replaced.
    Write,
}

/// The folders an agent may touch.
#[derive(Debug, Clone, Default)]
pub struct Confinement {
    read: Vec<PathBuf>,
    write: Vec<PathBuf>,
    unrestricted: bool,
}

impl Confinement {
    /// Roots that must exist (each resolved to its real path). Write roots may also be read.
    ///
    /// # Errors
    /// [`ErrorKind::InvalidParams`] when a root does not exist or cannot be resolved.
    pub fn new(read: &[PathBuf], write: &[PathBuf]) -> Result<Self, ControlError> {
        let canonical = |p: &PathBuf| p.canonicalize().map_err(|e| ControlError::invalid(format!("root {}: {e}", p.display())));
        Ok(Self {
            read: read.iter().map(canonical).collect::<Result<_, _>>()?,
            write: write.iter().map(canonical).collect::<Result<_, _>>()?,
            unrestricted: false,
        })
    }

    /// No confinement at all; every path is allowed.
    #[must_use]
    pub const fn unrestricted() -> Self {
        Self { read: Vec::new(), write: Vec::new(), unrestricted: true }
    }

    /// Whether anything is checked.
    #[must_use]
    pub const fn is_unrestricted(&self) -> bool {
        self.unrestricted
    }

    /// The configured roots, for messages and status: `(read, write)`.
    #[must_use]
    pub fn roots(&self) -> (&[PathBuf], &[PathBuf]) {
        (&self.read, &self.write)
    }

    fn allowed_roots(&self, access: Access) -> impl Iterator<Item = &PathBuf> {
        let read: &[PathBuf] = if access == Access::Read { &self.read } else { &[] };
        read.iter().chain(&self.write)
    }

    fn denied(&self, path: &str, access: Access, why: &str) -> ControlError {
        let verb = if access == Access::Read { "read" } else { "write" };
        let roots: Vec<String> = self.allowed_roots(access).map(|r| r.display().to_string()).collect();
        ControlError::new(ErrorKind::Denied, format!("cannot {verb} {path:?}: {why}")).with_data(serde_json::json!({ "allowed_roots": roots }))
    }

    /// Resolves `path` for `access` and returns the real path to use, or why it is refused.
    ///
    /// # Errors
    /// [`ErrorKind::Denied`] for relative paths, `..`, symlink escapes and paths outside the roots.
    pub fn check(&self, path: &str, access: Access) -> Result<PathBuf, ControlError> {
        if path.is_empty() || path.contains('\0') {
            return Err(self.denied(path, access, "empty or invalid path"));
        }
        let candidate = Path::new(path);
        if self.unrestricted {
            return Ok(candidate.to_path_buf());
        }
        if !candidate.is_absolute() {
            return Err(self.denied(path, access, "the path must be absolute"));
        }
        if candidate.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(self.denied(path, access, "`..` is not allowed"));
        }
        let resolved = resolve(candidate).map_err(|e| self.denied(path, access, &e))?;
        if self.allowed_roots(access).any(|root| resolved.starts_with(root)) {
            Ok(resolved)
        } else {
            Err(self.denied(path, access, "outside the allowed folders"))
        }
    }

    /// Checks every path-like parameter in `value`. `classify` says, for an object key, whether the string (or array of
    /// strings) under it is a path and how it is used.
    ///
    /// # Errors
    /// The first [`ErrorKind::Denied`] found.
    pub fn check_params(&self, value: &Value, classify: &dyn Fn(&str) -> Option<Access>) -> Result<(), ControlError> {
        if self.unrestricted {
            return Ok(());
        }
        self.scan(value, classify, 0)
    }

    fn scan(&self, value: &Value, classify: &dyn Fn(&str) -> Option<Access>, depth: usize) -> Result<(), ControlError> {
        if depth > MAX_SCAN_DEPTH {
            return Err(ControlError::invalid("parameters are nested too deeply"));
        }
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if let Some(access) = classify(key) {
                        self.check_path_value(child, access)?;
                    } else {
                        self.scan(child, classify, depth.saturating_add(1))?;
                    }
                }
                Ok(())
            }
            Value::Array(items) => items.iter().try_for_each(|child| self.scan(child, classify, depth.saturating_add(1))),
            _ => Ok(()),
        }
    }

    fn check_path_value(&self, value: &Value, access: Access) -> Result<(), ControlError> {
        match value {
            Value::String(text) => self.check(text, access).map(|_| ()),
            Value::Array(items) => items.iter().try_for_each(|item| self.check_path_value(item, access)),
            _ => Ok(()),
        }
    }
}

/// The real location of `path`: the path itself when it exists, otherwise its longest existing ancestor resolved plus
/// the missing tail (so a write to a new file under a symlinked folder is judged by where the folder really is).
fn resolve(path: &Path) -> Result<PathBuf, String> {
    if let Ok(real) = path.canonicalize() {
        return Ok(real);
    }
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut current = path;
    loop {
        if let Ok(mut real) = current.canonicalize() {
            for part in tail.iter().rev() {
                real.push(part);
            }
            return Ok(real);
        }
        let name = current.file_name().ok_or_else(|| "no existing parent folder".to_owned())?;
        tail.push(name);
        current = current.parent().ok_or_else(|| "no existing parent folder".to_owned())?;
    }
}

/// Default classification of a parameter key: which keys hold file paths. Commands that write (their id mentions save,
/// export or write) treat them as write paths.
#[must_use]
pub fn default_path_access(command: &str, key: &str) -> Option<Access> {
    const KEYS: &[&str] = &["path", "paths", "file", "files", "folder", "dir", "directory", "output", "src", "source", "dest", "destination", "target_path"];
    let lower = key.to_ascii_lowercase();
    if !KEYS.contains(&lower.as_str()) && !lower.ends_with("_path") && !lower.ends_with("_file") && !lower.ends_with("_paths") {
        return None;
    }
    let writes = ["save", "export", "write", "render", "output", "dest"].iter().any(|w| command.contains(w) || lower.contains(w));
    Some(if writes { Access::Write } else { Access::Read })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_inside_roots_pass_and_outside_fail() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        std::fs::write(other.path().join("b.txt"), "x").unwrap();
        let c = Confinement::new(&[dir.path().to_path_buf()], &[]).unwrap();
        assert!(c.check(file.to_str().unwrap(), Access::Read).is_ok());
        assert_eq!(c.check(other.path().join("b.txt").to_str().unwrap(), Access::Read).unwrap_err().kind, ErrorKind::Denied);
        assert_eq!(c.check(file.to_str().unwrap(), Access::Write).unwrap_err().kind, ErrorKind::Denied);
    }

    #[test]
    fn dot_dot_and_relative_paths_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confinement::new(&[], &[dir.path().to_path_buf()]).unwrap();
        let sneaky = format!("{}/sub/../../x", dir.path().display());
        assert_eq!(c.check(&sneaky, Access::Write).unwrap_err().kind, ErrorKind::Denied);
        assert_eq!(c.check("relative.png", Access::Write).unwrap_err().kind, ErrorKind::Denied);
        assert_eq!(c.check("", Access::Write).unwrap_err().kind, ErrorKind::Denied);
    }

    #[test]
    fn new_files_under_a_write_root_are_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confinement::new(&[], &[dir.path().to_path_buf()]).unwrap();
        let target = dir.path().join("new").join("out.png");
        assert!(c.check(target.to_str().unwrap(), Access::Write).is_ok());
        assert!(c.check(target.to_str().unwrap(), Access::Read).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        let c = Confinement::new(&[dir.path().to_path_buf()], &[dir.path().to_path_buf()]).unwrap();
        let via = dir.path().join("link").join("secret");
        assert_eq!(c.check(via.to_str().unwrap(), Access::Read).unwrap_err().kind, ErrorKind::Denied);
        let new_via = dir.path().join("link").join("new.png");
        assert_eq!(c.check(new_via.to_str().unwrap(), Access::Write).unwrap_err().kind, ErrorKind::Denied);
    }

    #[test]
    fn no_roots_means_nothing_is_allowed_unless_unrestricted() {
        let c = Confinement::default();
        assert!(c.check("/tmp/x", Access::Read).is_err());
        assert!(Confinement::unrestricted().check("/tmp/x", Access::Write).is_ok());
    }

    #[test]
    fn parameter_scan_finds_nested_paths() {
        let dir = tempfile::tempdir().unwrap();
        let c = Confinement::new(&[dir.path().to_path_buf()], &[]).unwrap();
        let classify = |key: &str| default_path_access("file.import", key);
        let ok = json!({"name": "x", "nested": [{"note": "/etc/passwd"}]});
        assert!(c.check_params(&ok, &classify).is_ok());
        let bad = json!({"nested": [{"paths": ["/etc/passwd"]}]});
        assert_eq!(c.check_params(&bad, &classify).unwrap_err().kind, ErrorKind::Denied);
    }

    #[test]
    fn deep_nesting_is_refused_not_overflowed() {
        let c = Confinement::default();
        let mut value = json!({});
        for _ in 0..100 {
            value = json!({ "a": value });
        }
        assert_eq!(c.check_params(&value, &|_| None).unwrap_err().kind, ErrorKind::InvalidParams);
    }

    #[test]
    fn key_classification() {
        assert_eq!(default_path_access("file.import", "path"), Some(Access::Read));
        assert_eq!(default_path_access("file.export_gif", "path"), Some(Access::Write));
        assert_eq!(default_path_access("x.y", "output_path"), Some(Access::Write));
        assert_eq!(default_path_access("x.y", "name"), None);
    }
}
