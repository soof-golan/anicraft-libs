//! Font discovery and per-script fallback chains.
//!
//! [`Fonts`] wraps parley's font context (fontique underneath: system fonts through fontconfig, `DirectWrite` or
//! `CoreText`, plus any font data registered at run time). On top of it, [`Fonts::chain`] answers "which installed
//! families can draw this script", by testing the preferred families of [`Script`] against the script's sample text
//! with skrifa's `cmap` reader, and scanning every installed family when none of the preferred ones is present.

use std::collections::HashMap;

use fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use parley::FontContext;
use skrifa::{FontRef, MetadataProvider};

use crate::script::Script;

/// The longest fallback chain kept per script.
const MAX_CHAIN: usize = 6;
/// Installed families examined by the exhaustive scan, so a machine with a pathological font folder stays responsive.
const MAX_SCAN: usize = 3000;
/// Registered font data larger than this is refused.
pub const MAX_FONT_BYTES: usize = 128 << 20;

pub struct Fonts {
    pub cx: FontContext,
    chains: HashMap<Script, Vec<String>>,
}

impl Fonts {
    /// A font database; `system` loads (lazily) the fonts installed on this machine.
    pub fn new(system: bool) -> Self {
        let collection = Collection::new(CollectionOptions { shared: false, system_fonts: system });
        Self { cx: FontContext { collection, source_cache: SourceCache::default() }, chains: HashMap::new() }
    }

    /// Registers TTF/OTF/TTC data and returns the family names it added.
    pub fn register(&mut self, bytes: Vec<u8>) -> Vec<String> {
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
            return Vec::new();
        }
        let added = self.cx.collection.register_fonts(Blob::from(bytes), None);
        let mut names = Vec::new();
        for (id, _) in added {
            if let Some(name) = self.cx.collection.family_name(id)
                && !names.iter().any(|n: &String| n == name)
            {
                names.push(name.to_owned());
            }
        }
        // A machine (or test) without system fonts has no generic sans-serif: let the first registered family stand in.
        if let Some(first) = names.first().and_then(|n| self.cx.collection.family_id(n)) {
            for generic in [GenericFamily::SansSerif, GenericFamily::SystemUi, GenericFamily::UiSansSerif] {
                if self.cx.collection.generic_families(generic).next().is_none() {
                    self.cx.collection.set_generic_families(generic, std::iter::once(first));
                }
            }
        }
        self.chains.clear();
        names
    }

    /// Every installed or registered family name, sorted, without private (dot-prefixed) families.
    pub fn families(&mut self) -> Vec<String> {
        let mut names: Vec<String> = self.cx.collection.family_names().filter(|n| !n.starts_with('.')).map(str::to_owned).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    pub fn has_family(&mut self, name: &str) -> bool {
        self.cx.collection.family_id(name).is_some()
    }

    /// Whether the regular face of `family` maps every character of `sample`.
    pub fn family_covers(&mut self, family: &str, sample: &str) -> bool {
        let Some(info) = self.cx.collection.family_by_name(family) else { return false };
        let Some(font) = info.default_font() else { return false };
        let Some(blob) = font.load(Some(&mut self.cx.source_cache)) else { return false };
        let Ok(face) = FontRef::from_index(blob.data(), font.index()) else { return false };
        let charmap = face.charmap();
        sample.chars().all(|c| charmap.map(u32::from(c)).is_some_and(|g| g.to_u32() != 0))
    }

    /// Families that can draw `script`, best first; empty when nothing installed covers it.
    pub fn chain(&mut self, script: Script) -> Vec<String> {
        if let Some(chain) = self.chains.get(&script) {
            return chain.clone();
        }
        let sample = script.sample();
        let mut chain: Vec<String> = Vec::new();
        for name in script.preferred_families() {
            if chain.len() >= MAX_CHAIN {
                break;
            }
            if self.has_family(name) && self.family_covers(name, sample) {
                chain.push((*name).to_owned());
            }
        }
        if chain.is_empty() {
            // None of the well-known families: any installed family that covers the script will do. Plain text faces
            // first (monospace, condensed, display and weight-named families come last).
            let mut found: Vec<String> = Vec::new();
            let names: Vec<String> = self.families().into_iter().take(MAX_SCAN).collect();
            for name in names {
                if self.family_covers(&name, sample) {
                    found.push(name);
                }
            }
            found.sort_by_key(|n| (is_unusual_face(n), n.to_lowercase()));
            chain.extend(found.into_iter().take(MAX_CHAIN));
        }
        self.chains.insert(script, chain.clone());
        chain
    }
}

/// Family names that are poor general-purpose fallbacks.
fn is_unusual_face(name: &str) -> bool {
    const WORDS: [&str; 12] = ["mono", "condensed", "narrow", "light", "thin", "black", "heavy", "display", "decorative", "typewriter", "typist", "poster"];
    let lower = name.to_lowercase();
    lower.starts_with("kacst") || lower.split(|c: char| !c.is_alphanumeric()).any(|word| WORDS.contains(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_database_has_no_families_and_no_chains() {
        let mut fonts = Fonts::new(false);
        assert_eq!(fonts.families(), Vec::<String>::new());
        assert!(!fonts.has_family("DejaVu Sans"));
        assert_eq!(fonts.chain(Script::Hebrew), Vec::<String>::new());
        assert!(!fonts.family_covers("nothing", "a"));
    }

    #[test]
    fn garbage_font_data_registers_nothing() {
        let mut fonts = Fonts::new(false);
        assert_eq!(fonts.register(Vec::new()), Vec::<String>::new());
        assert_eq!(fonts.register(vec![1, 2, 3, 4, 5, 6, 7, 8]), Vec::<String>::new());
        assert_eq!(fonts.families(), Vec::<String>::new());
    }

    #[test]
    fn unusual_faces_sort_last() {
        assert!(is_unusual_face("Noto Sans Mono CJK SC"));
        assert!(!is_unusual_face("DejaVu Sans"));
    }
}
