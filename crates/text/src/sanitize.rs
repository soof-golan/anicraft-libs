//! Making text safe for the shaper without moving any byte offset.
//!
//! The text engine promises that offsets into the laid-out text are offsets into the author's text, so nothing here may
//! change the UTF-8 length of anything. What it does change:
//!
//! - tabs become spaces and carriage returns become line feeds (`"\r\n"` becomes a space and a line feed);
//! - a combining mark that directly follows a forced line break is replaced by an invisible character of the same
//!   length. Parley's cluster analysis glues such a mark to the break character, and a later assertion in its glyph
//!   bookkeeping then fails (a panic in debug builds, a garbled cluster in release builds). The mark has nothing to
//!   attach to there anyway;
//! - the last-resort *scrub* replaces every non-ASCII character by a visible placeholder of the same length, used only
//!   when shaping the real text panicked.

use icu_properties::props::GraphemeClusterBreak;
use parley_data::Properties;

/// Appends text to a shaper-bound string, remembering whether the previous character was a forced line break.
#[derive(Debug, Default)]
pub struct Sanitizer {
    after_break: bool,
    scrub: bool,
}

impl Sanitizer {
    pub const fn new(scrub: bool) -> Self {
        Self { after_break: false, scrub }
    }

    pub fn push(&mut self, out: &mut String, text: &str) {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let c = match c {
                '\t' => ' ',
                '\r' if chars.peek() == Some(&'\n') => ' ',
                '\r' => '\n',
                other => other,
            };
            let props = Properties::get(c);
            let c = if self.scrub {
                placeholder(c)
            } else if self.after_break && attaches_to_previous(props, c) {
                invisible(c.len_utf8()).unwrap_or(c)
            } else {
                c
            };
            self.after_break = Properties::get(c).is_mandatory_linebreak();
            out.push(c);
        }
    }
}

/// Whether parley's analysis may merge `c` into the cluster of the character before it: grapheme extenders (except the
/// zero-width non-joiner), spacing marks, the zero-width joiner and variation selectors. Found by fuzzing: after a line
/// break each of these makes parley's glyph bookkeeping fail an assertion.
fn attaches_to_previous(props: Properties, c: char) -> bool {
    let gcb = props.grapheme_cluster_break();
    (gcb == GraphemeClusterBreak::Extend && c != '\u{200C}')
        || gcb == GraphemeClusterBreak::SpacingMark
        || gcb == GraphemeClusterBreak::ZWJ
        || props.is_variation_selector()
}

/// An invisible default-ignorable character that is `len` bytes long in UTF-8.
const fn invisible(len: usize) -> Option<char> {
    match len {
        2 => Some('\u{00AD}'),
        3 => Some('\u{2060}'),
        4 => Some('\u{E0001}'),
        _ => None,
    }
}

/// A visible stand-in of the same UTF-8 length.
const fn placeholder(c: char) -> char {
    match c.len_utf8() {
        2 => '\u{00B7}',
        3 => '\u{25A1}',
        4 => '\u{1F4A0}',
        _ if c.is_ascii_control() && c != '\n' => ' ',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, scrub: bool) -> String {
        let mut out = String::new();
        Sanitizer::new(scrub).push(&mut out, text);
        out
    }

    #[test]
    fn tabs_and_returns_keep_their_length() {
        assert_eq!(run("a\tb\r\nc\rd", false), "a b \nc\nd");
    }

    #[test]
    fn marks_after_a_line_break_are_replaced_by_invisible_characters_of_equal_length() {
        for text in ["a\n\u{301}b", "\n\u{64C}a", "x\u{2028}\u{301}y", "\r\u{301}z", "q\n\u{903}w", "a\n\u{200D}b", "\u{2029}\u{FE0F}c", "\n\u{E0100}d"] {
            let cleaned = run(text, false);
            assert_eq!(cleaned.len(), text.len(), "{text:?}");
            assert_ne!(cleaned, text.replace('\r', "\n"), "{text:?} must change");
        }
        // Marks after letters, and the letter after the break, are untouched.
        assert_eq!(run("a\u{301}\nb", false), "a\u{301}\nb");
        assert_eq!(run("\n\u{200C}a", false), "\n\u{200C}a");
    }

    #[test]
    fn the_state_carries_across_runs() {
        let mut s = Sanitizer::new(false);
        let mut out = String::new();
        s.push(&mut out, "line\n");
        s.push(&mut out, "\u{301}mark");
        assert_eq!(out.len(), "line\n\u{301}mark".len());
        assert!(!out.contains('\u{301}'));
    }

    #[test]
    fn scrubbing_preserves_lengths_and_line_structure() {
        let text = "a\nשלום 中 😀\u{301}";
        let scrubbed = run(text, true);
        assert_eq!(scrubbed.len(), text.len());
        assert_eq!(scrubbed.matches('\n').count(), 1);
        assert!(scrubbed.is_ascii() || scrubbed.chars().all(|c| !matches!(c, 'ש' | '中')));
    }
}
