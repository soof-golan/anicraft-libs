//! Settings pieces shared by both apps, stored with `craft-settings` (`config.toml` in the platform config folder).
//!
//! Each app defines its own settings type (with its `APP_ID`) and embeds [`CommonSettings`] as a field:
//!
//! ```ignore
//! #[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
//! #[serde(default)]
//! struct Prefs { common: CommonSettings, view: ViewPrefs }
//! impl Settings for Prefs { const APP_ID: &'static str = "anicraft"; const SCHEMA_VERSION: u32 = 1; }
//! let (mut store, report) = SettingsStore::<Prefs>::open(Location::Auto);
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::history::Budget;

/// Most recently used files, newest first. Paths that are not valid UTF-8 are not remembered (TOML cannot hold them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentFiles {
    /// Maximum number of entries kept.
    pub limit: u32,
    files: Vec<String>,
}

impl Default for RecentFiles {
    fn default() -> Self {
        Self { limit: 12, files: Vec::new() }
    }
}

impl RecentFiles {
    fn limit_usize(&self) -> usize {
        usize::try_from(self.limit).unwrap_or(usize::MAX)
    }

    /// Moves `path` to the front (adding it if new) and trims the list to the limit.
    pub fn push(&mut self, path: &Path) {
        let Some(text) = path.to_str() else {
            return;
        };
        self.files.retain(|f| Path::new(f) != path);
        self.files.insert(0, text.to_owned());
        self.files.truncate(self.limit_usize());
    }

    /// Forgets one path.
    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|f| Path::new(f) != path);
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.files.clear();
    }

    /// Drops entries whose file no longer exists. Returns how many were dropped.
    pub fn prune_missing(&mut self) -> usize {
        let before = self.files.len();
        self.files.retain(|f| Path::new(f).exists());
        before.saturating_sub(self.files.len())
    }

    /// Entries, newest first.
    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        self.files.iter().map(Path::new)
    }

    /// Entries as owned paths, newest first.
    #[must_use]
    pub fn to_vec(&self) -> Vec<PathBuf> {
        self.iter().map(Path::to_path_buf).collect()
    }

    /// Number of entries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether there are no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Undo budget as the user sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    /// Maximum undo steps.
    pub max_steps: u32,
    /// Maximum megabytes of snapshots.
    pub max_megabytes: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self { max_steps: 200, max_megabytes: 512 }
    }
}

impl HistorySettings {
    /// The equivalent [`Budget`].
    #[must_use]
    pub fn budget(self) -> Budget {
        let steps = usize::try_from(self.max_steps).unwrap_or(usize::MAX);
        let megabytes = usize::try_from(self.max_megabytes).unwrap_or(usize::MAX);
        Budget { max_steps: steps, max_bytes: megabytes.saturating_mul(1024 * 1024) }
    }
}

/// Settings both apps have.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CommonSettings {
    /// Recently opened files.
    pub recent: RecentFiles,
    /// Undo limits.
    pub history: HistorySettings,
    /// Autosave interval in minutes; zero turns it off.
    pub autosave_minutes: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_settings::{Settings, SettingsStore};

    #[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
    #[serde(default)]
    struct Prefs {
        common: CommonSettings,
        theme: ThemePrefs,
    }
    #[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
    #[serde(default)]
    struct ThemePrefs {
        dark: bool,
    }
    impl Settings for Prefs {
        const APP_ID: &'static str = "anim-session-test";
        const SCHEMA_VERSION: u32 = 1;
    }

    #[test]
    fn recent_files_order_dedupe_and_limit() {
        let mut r = RecentFiles { limit: 3, ..RecentFiles::default() };
        for name in ["a", "b", "c", "a", "d"] {
            r.push(Path::new(name));
        }
        assert_eq!(r.to_vec(), [PathBuf::from("d"), PathBuf::from("a"), PathBuf::from("c")]);
        r.remove(Path::new("a"));
        assert_eq!(r.len(), 2);
        r.clear();
        assert!(r.is_empty());
    }

    #[test]
    fn prune_missing_keeps_existing() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.fla");
        std::fs::write(&real, b"x").unwrap();
        let mut r = RecentFiles::default();
        r.push(&dir.path().join("gone.fla"));
        r.push(&real);
        assert_eq!(r.prune_missing(), 1);
        assert_eq!(r.to_vec(), [real]);
    }

    #[test]
    fn budget_conversion() {
        let b = HistorySettings { max_steps: 10, max_megabytes: 2 }.budget();
        assert_eq!(b, Budget { max_steps: 10, max_bytes: 2 * 1024 * 1024 });
    }

    #[test]
    fn settings_round_trip_in_a_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, report) = SettingsStore::<Prefs>::open_at(dir.path());
        assert!(!report.has_problems());
        store.update(|p| {
            p.common.recent.push(Path::new("/some/where/a.fla"));
            p.common.recent.push(Path::new("/some/where/b.fla"));
            p.common.autosave_minutes = 7;
            p.common.history.max_steps = 50;
            p.theme.dark = true;
        });
        store.flush().unwrap();
        assert!(dir.path().join("config.toml").exists());

        let (reopened, report) = SettingsStore::<Prefs>::open_at(dir.path());
        assert!(!report.has_problems());
        let p = reopened.get();
        assert_eq!(p.common.autosave_minutes, 7);
        assert_eq!(p.common.history.max_steps, 50);
        assert_eq!(p.common.history.max_megabytes, 512, "untouched fields keep defaults");
        assert!(p.theme.dark);
        assert_eq!(p.common.recent.to_vec(), [PathBuf::from("/some/where/b.fla"), PathBuf::from("/some/where/a.fla")]);
    }

    #[test]
    fn damaged_section_resets_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "schema_version = 1\n[common]\nautosave_minutes = \"soon\"\n[theme]\ndark = true\n").unwrap();
        let (store, report) = SettingsStore::<Prefs>::open_at(dir.path());
        assert!(report.has_problems());
        assert_eq!(store.get().common, CommonSettings::default());
        assert!(store.get().theme.dark);
    }
}
