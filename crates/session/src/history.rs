//! Snapshot undo/redo.
//!
//! The history stores whole document snapshots (the document model is plain data, so a snapshot is a clone).
//! The caller hands over the state *before* a change with [`History::record`]; undo and redo swap the live
//! document with a stored snapshot, so no extra clones are made.
//!
//! * Labels: every step has a label for "Undo Move Keyframe" menu items.
//! * Merge keys: consecutive records with the same key collapse into one step (typing, nudging with arrow keys).
//! * Continuous drags: [`History::begin`] stores the base state, the UI previews freely, and
//!   [`History::commit`] turns the whole gesture into one step (or [`History::cancel`] hands the base back).
//! * Budgets: a step count and a byte count (through [`SizeHint`]); the oldest steps are dropped first.
//! * Clean tracking: every state has a version number; the saved state is remembered by version, so undoing back
//!   to the saved state is clean again and redoing past it is dirty again.

/// Approximate heap size of a snapshot, used for the byte budget.
pub trait SizeHint {
    /// Bytes this value occupies, roughly. It only has to be proportional, not exact.
    fn size_hint(&self) -> usize;
}

impl SizeHint for () {
    fn size_hint(&self) -> usize {
        0
    }
}
impl SizeHint for String {
    fn size_hint(&self) -> usize {
        self.len()
    }
}
impl SizeHint for Vec<u8> {
    fn size_hint(&self) -> usize {
        self.len()
    }
}
impl SizeHint for i32 {
    fn size_hint(&self) -> usize {
        size_of::<Self>()
    }
}
impl SizeHint for u64 {
    fn size_hint(&self) -> usize {
        size_of::<Self>()
    }
}
impl SizeHint for serde_json::Value {
    fn size_hint(&self) -> usize {
        match self {
            Self::Null | Self::Bool(_) | Self::Number(_) => 16,
            Self::String(s) => s.len().saturating_add(16),
            Self::Array(a) => a.iter().fold(24_usize, |n, v| n.saturating_add(v.size_hint())),
            Self::Object(o) => o.iter().fold(24_usize, |n, (k, v)| n.saturating_add(k.len()).saturating_add(v.size_hint())),
        }
    }
}

/// Limits on stored history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Maximum number of undo steps kept. Zero disables undo.
    pub max_steps: usize,
    /// Maximum total bytes of snapshots (undo and redo). The newest undo step is always kept.
    pub max_bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self { max_steps: 200, max_bytes: 512 * 1024 * 1024 }
    }
}

impl Budget {
    /// No limits at all (tests, small documents).
    #[must_use]
    pub const fn unlimited() -> Self {
        Self { max_steps: usize::MAX, max_bytes: usize::MAX }
    }
}

struct Entry<D> {
    state: D,
    version: u64,
    label: String,
    bytes: usize,
}

struct Pending<D> {
    base: D,
    label: String,
}

/// Undo and redo stacks of snapshots of `D`.
pub struct History<D> {
    undo: Vec<Entry<D>>,
    redo: Vec<Entry<D>>,
    version: u64,
    next_version: u64,
    clean: Option<u64>,
    last_merge_key: Option<String>,
    pending: Option<Pending<D>>,
    budget: Budget,
}

impl<D: SizeHint> Default for History<D> {
    fn default() -> Self {
        Self::new(Budget::default())
    }
}

impl<D: SizeHint> History<D> {
    /// An empty history whose current state counts as saved.
    #[must_use]
    pub const fn new(budget: Budget) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), version: 0, next_version: 1, clean: Some(0), last_merge_key: None, pending: None, budget }
    }

    const fn fresh_version(&mut self) -> u64 {
        let v = self.next_version;
        self.next_version = self.next_version.saturating_add(1);
        v
    }

    /// Records that the document is about to change (or just changed) from `before`.
    ///
    /// With `merge_key` equal to the previous record's key (and no undo, redo, save or other record between),
    /// the change folds into the previous step: that step keeps its older `before`, and takes the new label.
    pub fn record(&mut self, before: D, label: impl Into<String>, merge_key: Option<&str>) {
        let label = label.into();
        self.redo.clear();
        let merges = merge_key.is_some() && self.last_merge_key.as_deref() == merge_key && !self.undo.is_empty();
        let old_version = self.version;
        self.version = self.fresh_version();
        if merges {
            if let Some(top) = self.undo.last_mut() {
                top.label = label;
            }
        } else {
            let bytes = before.size_hint();
            self.undo.push(Entry { state: before, version: old_version, label, bytes });
        }
        self.last_merge_key = merge_key.map(str::to_owned);
        self.evict();
    }

    fn evict(&mut self) {
        let mut total = self.bytes();
        let mut drop_count = 0_usize;
        let len = self.undo.len();
        for entry in &self.undo {
            let too_many = len.saturating_sub(drop_count) > self.budget.max_steps;
            let too_big = total > self.budget.max_bytes && len.saturating_sub(drop_count) > 1;
            if !(too_many || too_big) {
                break;
            }
            total = total.saturating_sub(entry.bytes);
            drop_count = drop_count.saturating_add(1);
        }
        if drop_count > 0 {
            self.undo.drain(..drop_count.min(len));
        }
    }

    /// Steps back: swaps `doc` with the newest undo snapshot. Returns the label of the undone step.
    pub fn undo(&mut self, doc: &mut D) -> Option<String> {
        let mut entry = self.undo.pop()?;
        self.step(doc, &mut entry);
        let label = entry.label.clone();
        self.redo.push(entry);
        Some(label)
    }

    /// Steps forward: swaps `doc` with the newest redo snapshot. Returns the label of the redone step.
    pub fn redo(&mut self, doc: &mut D) -> Option<String> {
        let mut entry = self.redo.pop()?;
        self.step(doc, &mut entry);
        let label = entry.label.clone();
        self.undo.push(entry);
        self.evict();
        Some(label)
    }

    fn step(&mut self, doc: &mut D, entry: &mut Entry<D>) {
        std::mem::swap(doc, &mut entry.state);
        std::mem::swap(&mut self.version, &mut entry.version);
        entry.bytes = entry.state.size_hint();
        self.last_merge_key = None;
    }

    /// Starts a continuous gesture (drag, scrub, slider): `base` is the state before it. Replaces an open gesture.
    pub fn begin(&mut self, base: D, label: impl Into<String>) {
        self.pending = Some(Pending { base, label: label.into() });
    }

    /// Whether a gesture is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.pending.is_some()
    }

    /// Ends the gesture as one undo step. Returns false when none was open.
    pub fn commit(&mut self) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        self.record(pending.base, pending.label, None);
        true
    }

    /// Abandons the gesture and returns the base state the caller should restore.
    pub fn cancel(&mut self) -> Option<D> {
        self.pending.take().map(|p| p.base)
    }

    /// Whether there is a step to undo.
    #[must_use]
    pub const fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is a step to redo.
    #[must_use]
    pub const fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Label of the step undo would revert.
    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    /// Label of the step redo would reapply.
    #[must_use]
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Undo step labels, newest first.
    pub fn undo_labels(&self) -> impl Iterator<Item = &str> {
        self.undo.iter().rev().map(|e| e.label.as_str())
    }

    /// Redo step labels, next-to-redo first.
    pub fn redo_labels(&self) -> impl Iterator<Item = &str> {
        self.redo.iter().rev().map(|e| e.label.as_str())
    }

    /// Number of undo steps.
    #[must_use]
    pub const fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Number of redo steps.
    #[must_use]
    pub const fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Bytes held by snapshots (undo, redo and an open gesture's base).
    #[must_use]
    pub fn bytes(&self) -> usize {
        let stacks = self.undo.iter().chain(&self.redo).fold(0_usize, |n, e| n.saturating_add(e.bytes));
        let pending = self.pending.as_ref().map_or(0, |p| p.base.size_hint());
        stacks.saturating_add(pending)
    }

    /// The current budget.
    #[must_use]
    pub const fn budget(&self) -> Budget {
        self.budget
    }

    /// Changes the budget and drops the oldest steps that no longer fit.
    pub fn set_budget(&mut self, budget: Budget) {
        self.budget = budget;
        self.evict();
    }

    /// Remembers the current state as the saved one. Later edits no longer merge into earlier ones.
    pub fn mark_clean(&mut self) {
        self.clean = Some(self.version);
        self.last_merge_key = None;
    }

    /// Whether the current state is the saved one (follows undo and redo).
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.clean == Some(self.version)
    }

    /// Forgets all steps and treats the current state as saved (after opening a file).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.last_merge_key = None;
        self.version = self.fresh_version();
        self.clean = Some(self.version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Applies an edit the way a session does: record the old state, then change it.
    fn edit(h: &mut History<i32>, doc: &mut i32, to: i32, label: &str, key: Option<&str>) {
        h.record(*doc, label, key);
        *doc = to;
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 0;
        edit(&mut h, &mut doc, 1, "one", None);
        edit(&mut h, &mut doc, 2, "two", None);
        assert_eq!(h.undo_label(), Some("two"));
        assert_eq!(h.undo(&mut doc).as_deref(), Some("two"));
        assert_eq!(doc, 1);
        assert_eq!(h.redo_label(), Some("two"));
        assert_eq!(h.undo(&mut doc).as_deref(), Some("one"));
        assert_eq!(doc, 0);
        assert!(h.undo(&mut doc).is_none());
        assert_eq!(h.redo(&mut doc).as_deref(), Some("one"));
        assert_eq!(h.redo(&mut doc).as_deref(), Some("two"));
        assert_eq!(doc, 2);
        assert!(h.redo(&mut doc).is_none());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 0;
        edit(&mut h, &mut doc, 1, "one", None);
        h.undo(&mut doc);
        assert!(h.can_redo());
        edit(&mut h, &mut doc, 5, "five", None);
        assert!(!h.can_redo());
        assert_eq!(h.undo_labels().collect::<Vec<_>>(), ["five"]);
    }

    #[test]
    fn merge_keys_collapse_steps() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 0;
        for i in 1..=5 {
            edit(&mut h, &mut doc, i, "nudge", Some("nudge"));
        }
        assert_eq!(h.undo_len(), 1);
        edit(&mut h, &mut doc, 10, "other", Some("other"));
        assert_eq!(h.undo_len(), 2);
        h.undo(&mut doc);
        assert_eq!(doc, 5);
        h.undo(&mut doc);
        assert_eq!(doc, 0);
    }

    #[test]
    fn merge_is_sealed_by_undo_and_save() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 0;
        edit(&mut h, &mut doc, 1, "a", Some("k"));
        h.undo(&mut doc);
        h.redo(&mut doc);
        edit(&mut h, &mut doc, 2, "b", Some("k"));
        assert_eq!(h.undo_len(), 2, "undo/redo in between seals the merge");
        h.mark_clean();
        edit(&mut h, &mut doc, 3, "c", Some("k"));
        assert_eq!(h.undo_len(), 3, "saving seals the merge");
        assert!(!h.is_clean());
        h.undo(&mut doc);
        assert!(h.is_clean());
    }

    #[test]
    fn gesture_commit_is_one_step() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 7;
        h.begin(doc, "Drag");
        assert!(h.is_open());
        for v in 8..20 {
            doc = v; // previews do not touch the history
        }
        assert_eq!(h.undo_len(), 0);
        assert!(h.commit());
        assert!(!h.commit());
        assert_eq!(h.undo_len(), 1);
        h.undo(&mut doc);
        assert_eq!(doc, 7);
    }

    #[test]
    fn gesture_cancel_returns_base() {
        let mut h = History::<i32>::new(Budget::unlimited());
        h.begin(3, "Drag");
        assert_eq!(h.cancel(), Some(3));
        assert_eq!(h.cancel(), None);
        assert_eq!(h.undo_len(), 0);
        assert!(!h.is_open());
    }

    #[test]
    fn step_budget_drops_oldest() {
        let mut h = History::<i32>::new(Budget { max_steps: 3, max_bytes: usize::MAX });
        let mut doc = 0;
        for i in 1..=10 {
            edit(&mut h, &mut doc, i, &format!("s{i}"), None);
        }
        assert_eq!(h.undo_len(), 3);
        assert_eq!(h.undo_labels().collect::<Vec<_>>(), ["s10", "s9", "s8"]);
        while h.undo(&mut doc).is_some() {}
        assert_eq!(doc, 7);
    }

    #[test]
    fn byte_budget_drops_oldest_but_keeps_newest() {
        let mut h = History::<String>::new(Budget { max_steps: 100, max_bytes: 25 });
        let mut doc = "x".repeat(10);
        for i in 0..5 {
            h.record(doc.clone(), format!("e{i}"), None);
            doc.push('y');
        }
        assert!(h.bytes() <= 25 + 14, "bytes {}", h.bytes());
        assert!(h.undo_len() >= 1 && h.undo_len() < 5);
        let mut h = History::<String>::new(Budget { max_steps: 100, max_bytes: 1 });
        h.record("x".repeat(100), "big", None);
        assert_eq!(h.undo_len(), 1, "newest step survives even when over budget");
    }

    #[test]
    fn zero_steps_disables_undo() {
        let mut h = History::<i32>::new(Budget { max_steps: 0, max_bytes: usize::MAX });
        h.record(0, "x", None);
        assert!(!h.can_undo());
    }

    #[test]
    fn shrinking_budget_evicts() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 0;
        for i in 1..=6 {
            edit(&mut h, &mut doc, i, "e", None);
        }
        h.set_budget(Budget { max_steps: 2, max_bytes: usize::MAX });
        assert_eq!(h.undo_len(), 2);
    }

    #[test]
    fn clean_follows_undo_and_redo() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 0;
        assert!(h.is_clean());
        edit(&mut h, &mut doc, 1, "a", None);
        assert!(!h.is_clean());
        h.mark_clean();
        assert!(h.is_clean());
        edit(&mut h, &mut doc, 2, "b", None);
        assert!(!h.is_clean());
        h.undo(&mut doc);
        assert!(h.is_clean());
        h.undo(&mut doc);
        assert!(!h.is_clean());
        h.redo(&mut doc);
        assert!(h.is_clean());
        h.redo(&mut doc);
        assert!(!h.is_clean());
    }

    #[test]
    fn saved_state_lost_after_branching() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 0;
        edit(&mut h, &mut doc, 1, "a", None);
        edit(&mut h, &mut doc, 2, "b", None);
        h.mark_clean();
        h.undo(&mut doc);
        edit(&mut h, &mut doc, 9, "c", None);
        // The saved state (2) is gone from the redo stack, so no reachable state is clean.
        assert!(!h.is_clean());
        h.undo(&mut doc);
        assert!(!h.is_clean());
        h.undo(&mut doc);
        assert!(!h.is_clean());
    }

    #[test]
    fn clear_makes_current_clean() {
        let mut h = History::<i32>::new(Budget::unlimited());
        let mut doc = 0;
        edit(&mut h, &mut doc, 1, "a", None);
        h.begin(1, "g");
        h.clear();
        assert!(h.is_clean() && !h.can_undo() && !h.is_open());
    }

    #[test]
    fn size_hint_of_json() {
        let v = serde_json::json!({ "a": [1, 2, 3], "b": "text" });
        assert!(v.size_hint() > 50);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Edit(i32, bool),
        Undo,
        Redo,
        Save,
    }

    #[expect(clippy::disallowed_types, reason = "prop_oneof! shares its strategies through an Arc internally")]
    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![(any::<i32>(), any::<bool>()).prop_map(|(v, m)| Op::Edit(v, m)), Just(Op::Undo), Just(Op::Redo), Just(Op::Save),]
    }

    proptest! {
        /// Against a plain model: states list plus a cursor.
        #[test]
        fn matches_a_cursor_model(ops in proptest::collection::vec(op_strategy(), 0..60)) {
            let mut h = History::<i32>::new(Budget::unlimited());
            let mut doc = 0_i32;
            let mut states = vec![0_i32];
            let mut cursor = 0_usize;
            let mut saved: Option<usize> = Some(0);
            let mut sealed = true;
            for op in ops {
                match op {
                    Op::Edit(v, merge) => {
                        h.record(doc, "e", merge.then_some("k"));
                        doc = v;
                        if merge && !sealed && cursor > 0 {
                            // merged: the last state is replaced in place
                            states.truncate(cursor + 1);
                            if let Some(last) = states.last_mut() { *last = v; }
                            // a saved marker pointing at the replaced state is gone
                            if saved == Some(cursor) { saved = None; }
                        } else {
                            states.truncate(cursor + 1);
                            // a saved marker pointing at a discarded redo state is gone
                            if saved.is_some_and(|s| s > cursor) { saved = None; }
                            states.push(v);
                            cursor += 1;
                        }
                        sealed = !merge;
                    }
                    Op::Undo => {
                        let done = h.undo(&mut doc).is_some();
                        prop_assert_eq!(done, cursor > 0);
                        if done {
                            cursor -= 1;
                            sealed = true;
                        }
                    }
                    Op::Redo => {
                        let done = h.redo(&mut doc).is_some();
                        prop_assert_eq!(done, cursor + 1 < states.len());
                        if done {
                            cursor += 1;
                            sealed = true;
                        }
                    }
                    Op::Save => { h.mark_clean(); saved = Some(cursor); sealed = true; }
                }
                prop_assert_eq!(doc, states[cursor]);
                prop_assert_eq!(h.is_clean(), saved == Some(cursor));
            }
        }
    }
}
