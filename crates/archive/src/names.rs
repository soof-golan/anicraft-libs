//! Entry-name helpers: normalisation, case-insensitive lookup keys, and the escaping schemes XFL uses.
//!
//! Two escape layers meet in an XFL folder or FLA:
//!
//! 1. **XML escaping** of attribute text, such as `href="a&amp;b.xml"`. An XML parser undoes it before the text
//!    reaches this crate; [`xml_unescape`] and [`xml_escape`] are here for callers that handle raw text.
//! 2. **File-name escaping** by Animate itself: characters that no platform allows in file names are written as
//!    `&#NNN` (three decimal digits, no semicolon). A library item imported from Illustrator named `<Clip Group>` is
//!    stored as the entry `LIBRARY/&#060Clip Group&#062.xml`. [`unescape_filename`] and [`escape_filename`] map
//!    between the stored spelling and the readable one.

use std::borrow::Cow;
use std::fmt::Write;
use unicode_normalization::UnicodeNormalization;

/// Normalises an entry name to the form [`crate::Container`] uses.
///
/// Separators become `/`; leading `/`, `.` segments and empty segments are dropped; the name is cut at the first NUL.
/// A trailing separator is kept because it marks a directory entry. `..` segments are preserved so callers can detect
/// them with [`is_safe_relative`].
#[must_use]
pub fn normalize_name(raw: &str) -> String {
    let text = raw.split('\0').next().unwrap_or("");
    let is_dir = text.ends_with(['/', '\\']);
    let mut out = String::with_capacity(text.len());
    for part in text.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }
    if is_dir && !out.is_empty() {
        out.push('/');
    }
    out
}

/// True when `name` (already normalised) is a non-empty relative path that stays inside its root: no `..`
/// segment, no drive prefix such as `C:`, and no NUL.
#[must_use]
pub fn is_safe_relative(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') {
        return false;
    }
    let mut parts = name.split('/').filter(|p| !p.is_empty()).peekable();
    if parts.peek().is_none() {
        return false;
    }
    let first_is_drive = parts.peek().is_some_and(|p| {
        let mut chars = p.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.next() == Some(':')
    });
    if first_is_drive {
        return false;
    }
    parts.all(|p| p != "..")
}

/// Characters Animate escapes in file names: the ones Windows forbids, plus control characters.
const fn needs_filename_escape(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\') || c.is_ascii_control()
}

/// Turns Animate's `&#NNN` file-name escapes back into the characters they stand for (`&#060` becomes `<`).
///
/// Only the exact form `&#` + three decimal digits (value 1..=127, optional trailing `;`) is decoded; anything else
/// stays literal, so ordinary names pass through unchanged.
#[must_use]
pub fn unescape_filename(name: &str) -> Cow<'_, str> {
    if !name.contains("&#") {
        return Cow::Borrowed(name);
    }
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(at) = rest.find("&#") {
        let (before, tail) = rest.split_at(at);
        out.push_str(before);
        let after = tail.get(2..).unwrap_or("");
        let digits = after.get(..3).filter(|d| d.bytes().all(|b| b.is_ascii_digit()));
        let decoded = digits.and_then(|d| d.parse::<u32>().ok()).filter(|v| (1..=127).contains(v)).and_then(char::from_u32);
        if let (Some(ch), Some(rest_after)) = (decoded, after.get(3..)) {
            out.push(ch);
            rest = rest_after.strip_prefix(';').unwrap_or(rest_after);
        } else {
            out.push_str("&#");
            rest = after;
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// The inverse of [`unescape_filename`]: replaces characters that cannot appear in a file name with `&#NNN`.
///
/// Applied to a whole name or path; `/` is left alone so the separators survive.
#[must_use]
pub fn escape_filename(name: &str) -> Cow<'_, str> {
    if !name.chars().any(needs_filename_escape) {
        return Cow::Borrowed(name);
    }
    let mut out = String::with_capacity(name.len().saturating_add(8));
    for c in name.chars() {
        if needs_filename_escape(c) {
            let _ = write!(out, "&#{:03}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Undoes XML attribute escaping: `&amp; &lt; &gt; &quot; &apos;` and numeric references `&#NN;` / `&#xNN;`.
///
/// References must end in `;`, as XML requires, so Animate's semicolon-free `&#060` file-name escapes survive.
#[must_use]
pub fn xml_unescape(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        let (before, tail) = rest.split_at(at);
        out.push_str(before);
        let decoded = tail.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = tail.get(1..end)?;
            let ch = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let value = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse::<u32>().ok()?,
                    };
                    char::from_u32(value)?
                }
            };
            Some((ch, tail.get(end.checked_add(1)?..)?))
        });
        if let Some((ch, after)) = decoded {
            out.push(ch);
            rest = after;
        } else {
            out.push('&');
            rest = tail.get(1..).unwrap_or("");
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Escapes text for use inside a double-quoted XML attribute.
#[must_use]
pub fn xml_escape(text: &str) -> Cow<'_, str> {
    if !text.contains(['&', '<', '>', '"', '\'']) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len().saturating_add(16));
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// Maps an XFL `href` to the entry name it refers to.
///
/// The `href` is the attribute text as an XML parser returns it (already XML-unescaped). It is normalised and joined
/// onto `base_dir`: use `"LIBRARY"` for symbol includes and `"bin"` for media. So
/// `href_to_entry_name("Tween 1.xml", "LIBRARY")` is `"LIBRARY/Tween 1.xml"`.
#[must_use]
pub fn href_to_entry_name(href: &str, base_dir: &str) -> String {
    let href = normalize_name(href);
    let base = normalize_name(base_dir);
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        href
    } else if href.is_empty() {
        base.to_owned()
    } else {
        format!("{base}/{href}")
    }
}

/// The inverse of [`href_to_entry_name`]: strips `base_dir/` from an entry name.
///
/// `None` when the entry is not under `base_dir`. The result is plain text; XML-escape it with [`xml_escape`] before
/// writing it into an attribute.
#[must_use]
pub fn entry_name_to_href(name: &str, base_dir: &str) -> Option<String> {
    let name = normalize_name(name);
    let base = normalize_name(base_dir);
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        return Some(name);
    }
    name.strip_prefix(base).and_then(|rest| rest.strip_prefix('/')).map(str::to_owned)
}

/// The key used for lenient lookups.
///
/// It is the normalised name with file-name escapes decoded, Unicode NFC, lower-cased and without a trailing
/// separator. Two names with the same key are "the same file" for case-insensitive and escape-insensitive matching
/// (and for macOS file systems that store decomposed Unicode).
#[must_use]
pub fn lookup_key(name: &str) -> String {
    let normalized = normalize_name(name);
    let unescaped = unescape_filename(&normalized);
    let nfc: String = unescaped.nfc().collect();
    nfc.to_lowercase().trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_cleans_separators_and_dots() {
        assert_eq!(normalize_name("LIBRARY\\a.xml"), "LIBRARY/a.xml");
        assert_eq!(normalize_name("/abs//x/./y.xml"), "abs/x/y.xml");
        assert_eq!(normalize_name("dir/"), "dir/");
        assert_eq!(normalize_name("dir\\"), "dir/");
        assert_eq!(normalize_name("./"), "");
        assert_eq!(normalize_name("a\0b/c"), "a");
        assert_eq!(normalize_name(""), "");
    }

    #[test]
    fn safety_check_rejects_traversal() {
        assert!(is_safe_relative("a/b.xml"));
        assert!(is_safe_relative("dir/"));
        assert!(!is_safe_relative(""));
        assert!(!is_safe_relative("../x"));
        assert!(!is_safe_relative("a/../../x"));
        assert!(!is_safe_relative("C:/x"));
        assert!(!is_safe_relative("a\0b"));
    }

    #[test]
    fn filename_escapes_round_trip() {
        let stored = "LIBRARY/flash0.ai_2/Isolation Mode/&#060Clip Group&#062.xml";
        let readable = unescape_filename(stored);
        assert_eq!(readable, "LIBRARY/flash0.ai_2/Isolation Mode/<Clip Group>.xml");
        assert_eq!(escape_filename(&readable), stored);
        assert!(matches!(unescape_filename("plain.xml"), Cow::Borrowed(_)));
    }

    #[test]
    fn filename_unescape_leaves_lookalikes_alone() {
        assert_eq!(unescape_filename("a&#xyz"), "a&#xyz");
        assert_eq!(unescape_filename("a&#06"), "a&#06");
        assert_eq!(unescape_filename("a&#000"), "a&#000");
        assert_eq!(unescape_filename("a&#999"), "a&#999");
        assert_eq!(unescape_filename("a&#060;b"), "a<b");
        assert_eq!(unescape_filename("&#"), "&#");
        assert_eq!(unescape_filename("&#&#060"), "&#<");
    }

    #[test]
    fn xml_unescape_needs_semicolons() {
        assert_eq!(xml_unescape("a&amp;#060Clip&amp;#062.xml"), "a&#060Clip&#062.xml");
        assert_eq!(xml_unescape("&#060Clip"), "&#060Clip");
        assert_eq!(xml_unescape("&lt;x&gt; &quot;q&quot; &apos;"), "<x> \"q\" '");
        assert_eq!(xml_unescape("&#60;&#x3C;"), "<<");
        assert_eq!(xml_unescape("a & b"), "a & b");
        assert_eq!(xml_unescape("&unknown;"), "&unknown;");
    }

    #[test]
    fn xml_escape_round_trips() {
        let text = "a&b<c>\"d\"'e'";
        assert_eq!(xml_unescape(&xml_escape(text)), text);
        assert!(matches!(xml_escape("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn href_mapping_joins_and_strips() {
        assert_eq!(href_to_entry_name("Tween 1.xml", "LIBRARY"), "LIBRARY/Tween 1.xml");
        assert_eq!(href_to_entry_name("sub\\a.xml", "LIBRARY/"), "LIBRARY/sub/a.xml");
        assert_eq!(href_to_entry_name("x.dat", ""), "x.dat");
        assert_eq!(entry_name_to_href("LIBRARY/sub/a.xml", "LIBRARY").as_deref(), Some("sub/a.xml"));
        assert_eq!(entry_name_to_href("bin/a", "LIBRARY"), None);
        assert_eq!(entry_name_to_href("LIBRARYX/a", "LIBRARY"), None);
    }

    #[test]
    fn lookup_key_ignores_case_escapes_and_normalisation() {
        assert_eq!(lookup_key("LIBRARY/&#060Clip&#062.XML"), lookup_key("library/<clip>.xml"));
        // "e" + combining acute versus the precomposed form (macOS file systems store the former).
        assert_eq!(lookup_key("caf\u{65}\u{301}.xml"), lookup_key("caf\u{e9}.xml"));
        assert_eq!(lookup_key("dir/"), lookup_key("DIR"));
    }
}
