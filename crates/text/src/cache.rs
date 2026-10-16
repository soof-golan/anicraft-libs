//! A bounded cache of shaped paragraphs.
//!
//! Shaping (fallback fonts, bidi, harfrust) is nearly all of the cost of a layout, and it depends only on a paragraph's
//! direction, indent and runs: not on the alignment, margins or line spacing, and not on the box width (lines are broken
//! afterwards). So typing into one paragraph of a long document, resizing a text box, or re-aligning reshapes nothing
//! that did not change.
//!
//! Two generations bound the memory: when the young one has taken [`BUDGET`] bytes of text it becomes the old one and the
//! previous old one is dropped. A hit in the old generation moves the entry to the young one.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use anim_core::Rgba8;
use parley::Layout;

use crate::model::{Direction, Paragraph, Run};

/// Bytes of paragraph text the young generation takes before it is retired.
const BUDGET: usize = 512 << 10;

/// What shaping depends on.
struct Entry {
    direction: Direction,
    indent: u64,
    runs: Vec<Run>,
    layout: Layout<Rgba8>,
}

impl Entry {
    fn matches(&self, paragraph: &Paragraph) -> bool {
        self.direction == paragraph.direction && self.indent == paragraph.indent.to_bits() && self.runs == paragraph.runs
    }
}

#[derive(Default)]
pub struct ShapeCache {
    young: HashMap<u64, Entry>,
    old: HashMap<u64, Entry>,
    bytes: usize,
}

fn key(paragraph: &Paragraph) -> u64 {
    let mut hasher = DefaultHasher::new();
    paragraph.direction.hash(&mut hasher);
    paragraph.indent.to_bits().hash(&mut hasher);
    for run in &paragraph.runs {
        run.text.hash(&mut hasher);
        run.family.hash(&mut hasher);
        run.size.to_bits().hash(&mut hasher);
        run.letter_spacing.to_bits().hash(&mut hasher);
        (run.bold, run.italic, run.underline, run.color).hash(&mut hasher);
    }
    hasher.finish()
}

impl ShapeCache {
    /// A copy of the unbroken layout shaped from `paragraph`, if one is cached.
    pub fn get(&mut self, paragraph: &Paragraph) -> Option<Layout<Rgba8>> {
        let key = key(paragraph);
        if let Some(entry) = self.young.get(&key) {
            return entry.matches(paragraph).then(|| entry.layout.clone());
        }
        let entry = self.old.remove(&key).filter(|e| e.matches(paragraph))?;
        let layout = entry.layout.clone();
        self.young.insert(key, entry);
        Some(layout)
    }

    /// Remembers the unbroken `layout` shaped from `paragraph`.
    pub fn insert(&mut self, paragraph: &Paragraph, layout: &Layout<Rgba8>) {
        self.bytes = self.bytes.saturating_add(paragraph.text_len());
        if self.bytes > BUDGET {
            self.old = std::mem::take(&mut self.young);
            self.bytes = paragraph.text_len();
        }
        let entry = Entry { direction: paragraph.direction, indent: paragraph.indent.to_bits(), runs: paragraph.runs.clone(), layout: layout.clone() };
        self.young.insert(key(paragraph), entry);
    }

    /// Forgets everything (the fonts or the language changed).
    pub fn clear(&mut self) {
        self.young.clear();
        self.old.clear();
        self.bytes = 0;
    }
}
