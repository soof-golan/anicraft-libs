//! The call journal: every request a session owner handles is recorded, in memory and optionally as JSON lines.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How a journaled call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The call succeeded.
    Ok,
    /// The call failed.
    Error,
    /// The call was accepted and finishes later (a screenshot waiting for the next frame, a background export).
    Deferred,
}

/// One journaled call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Sequence number, from 1.
    pub seq: u64,
    /// Wall-clock time the call arrived, milliseconds since the Unix epoch.
    pub at_ms: u64,
    /// Who asked.
    pub origin: String,
    /// What was asked (`command.run edit.rename`).
    pub request: String,
    /// How it ended.
    pub outcome: Outcome,
    /// The error text for failures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Time spent handling it, milliseconds.
    pub took_ms: u64,
}

/// A bounded in-memory journal with an optional JSON-lines file.
#[derive(Debug)]
pub struct Journal {
    entries: VecDeque<JournalEntry>,
    capacity: usize,
    next_seq: u64,
    file: Option<File>,
}

impl Default for Journal {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl Journal {
    /// A journal keeping the newest `capacity` entries.
    #[must_use]
    pub const fn new(capacity: usize) -> Self {
        Self { entries: VecDeque::new(), capacity, next_seq: 1, file: None }
    }

    /// Also appends every entry to `path` as one JSON object per line.
    ///
    /// # Errors
    /// I/O errors opening the file.
    pub fn with_file(mut self, path: &Path) -> std::io::Result<Self> {
        self.file = Some(OpenOptions::new().create(true).append(true).open(path)?);
        Ok(self)
    }

    /// Records a call.
    pub fn record(&mut self, origin: &str, request: String, outcome: Outcome, error: Option<String>, took_ms: u64) {
        let at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let entry = JournalEntry { seq: self.next_seq, at_ms, origin: origin.to_owned(), request, outcome, error, took_ms };
        self.next_seq = self.next_seq.saturating_add(1);
        if let Some(file) = &mut self.file {
            let line = serde_json::to_string(&entry).unwrap_or_default();
            if writeln!(file, "{line}").and_then(|()| file.flush()).is_err() {
                self.file = None; // a broken log must not break editing
            }
        }
        self.entries.push_back(entry);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    /// The newest `limit` entries, oldest first.
    #[must_use]
    pub fn tail(&self, limit: usize) -> Vec<JournalEntry> {
        let skip = self.entries.len().saturating_sub(limit);
        self.entries.iter().skip(skip).cloned().collect()
    }

    /// Total calls recorded so far (including entries already dropped from memory).
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_entries() {
        let mut journal = Journal::new(3);
        for i in 0..5 {
            journal.record("t", format!("call {i}"), Outcome::Ok, None, 1);
        }
        let tail = journal.tail(10);
        assert_eq!(tail.len(), 3);
        assert_eq!(tail.first().map(|e| e.seq), Some(3));
        assert_eq!(journal.total(), 5);
        assert_eq!(journal.tail(1).first().map(|e| e.request.clone()), Some("call 4".to_owned()));
    }

    #[test]
    fn writes_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let mut journal = Journal::new(10).with_file(&path).unwrap();
        journal.record("mcp", "status".into(), Outcome::Error, Some("boom".into()), 2);
        let text = std::fs::read_to_string(&path).unwrap();
        let entry: JournalEntry = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(entry.error.as_deref(), Some("boom"));
        assert_eq!(entry.outcome, Outcome::Error);
    }
}
