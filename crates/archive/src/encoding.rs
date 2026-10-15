//! Decoding of zip entry names that are not (flagged) UTF-8.
//!
//! The zip format says a name is UTF-8 only when general-purpose flag bit 11 is set; otherwise it is "IBM code page
//! 437". In practice old tools wrote whatever the local code page was: Windows-1252 on Western systems,
//! Windows-1255 or DOS code page 862 on Hebrew systems. [`decode_name`] tries the likely candidates and picks the
//! one that reads most like a real file name, preferring Hebrew when Hebrew letters appear.

use encoding_rs::{WINDOWS_1252, WINDOWS_1255};

/// How an entry name was turned into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NameEncoding {
    /// Seven-bit ASCII: identical in every encoding.
    Ascii,
    /// UTF-8, declared by general-purpose flag bit 11.
    Utf8Flagged,
    /// Valid UTF-8 without the flag.
    Utf8,
    /// Windows-1255 (Hebrew ANSI).
    Windows1255,
    /// Windows-1252 (Western European ANSI).
    Windows1252,
    /// DOS code page 437, the zip specification's default.
    Cp437,
    /// DOS code page 862 (Hebrew OEM).
    Cp862,
    /// The name came from a file system (a folder container), not from zip bytes.
    FileSystem,
}

impl NameEncoding {
    /// True for the encodings that are lossless for any Unicode name.
    #[must_use]
    pub const fn is_unicode(self) -> bool {
        matches!(self, Self::Ascii | Self::Utf8Flagged | Self::Utf8 | Self::FileSystem)
    }
}

/// The 128 characters of code page 437 for bytes `0x80..=0xFF`.
const CP437_HIGH: &str =
    "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// Code page 862 equals 437 except that `0x80..=0x9A` hold the Hebrew letters alef to tav.
const CP862_FIRST_HEBREW: u8 = 0x80;
const CP862_LAST_HEBREW: u8 = 0x9A;
const HEBREW_ALEF: u32 = 0x05D0;

fn cp437_char(byte: u8) -> char {
    byte.checked_sub(0x80).and_then(|i| CP437_HIGH.chars().nth(usize::from(i))).unwrap_or(char::REPLACEMENT_CHARACTER)
}

fn cp862_char(byte: u8) -> char {
    if (CP862_FIRST_HEBREW..=CP862_LAST_HEBREW).contains(&byte) {
        let offset = u32::from(byte.saturating_sub(CP862_FIRST_HEBREW));
        char::from_u32(HEBREW_ALEF.saturating_add(offset)).unwrap_or(char::REPLACEMENT_CHARACTER)
    } else {
        cp437_char(byte)
    }
}

/// Decodes `bytes` as code page 437.
#[must_use]
pub fn decode_cp437(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| if b.is_ascii() { char::from(b) } else { cp437_char(b) }).collect()
}

/// Decodes `bytes` as code page 862 (Hebrew DOS).
#[must_use]
pub fn decode_cp862(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| if b.is_ascii() { char::from(b) } else { cp862_char(b) }).collect()
}

fn is_hebrew_letter(c: char) -> bool {
    ('\u{05D0}'..='\u{05EA}').contains(&c)
}

fn is_hebrew_mark(c: char) -> bool {
    ('\u{0591}'..='\u{05C7}').contains(&c)
}

/// How plausible `text` is as a file name: letters score, controls and stray symbols cost, and a Hebrew letter glued
/// to an ASCII letter (which real names almost never contain) costs more.
fn plausibility(text: &str) -> i32 {
    let mut score = 0_i32;
    let mut previous: Option<char> = None;
    for c in text.chars() {
        let delta = if c.is_ascii() {
            0
        } else if is_hebrew_letter(c) {
            3
        } else if is_hebrew_mark(c) {
            1
        } else if c.is_control() || c == char::REPLACEMENT_CHARACTER {
            -6
        } else if c.is_alphabetic() {
            2
        } else {
            -1
        };
        score = score.saturating_add(delta);
        if let Some(prev) = previous {
            let glued = (prev.is_ascii_alphabetic() && is_hebrew_letter(c)) || (is_hebrew_letter(prev) && c.is_ascii_alphabetic());
            if glued {
                score = score.saturating_sub(4);
            }
        }
        previous = Some(c);
    }
    score
}

/// Decodes a zip entry name.
///
/// UTF-8 is used when `utf8_flag` (general-purpose bit 11) is set or the bytes happen to be valid UTF-8. Otherwise
/// the legacy candidates Windows-1255, Windows-1252, CP437 and CP862 are scored with a plausibility heuristic (see
/// the module docs) and the best wins; ties go to the earlier candidate in that order. Never fails: a flagged name
/// that is not valid UTF-8 is treated as legacy bytes too.
#[must_use]
pub fn decode_name(raw: &[u8], utf8_flag: bool) -> (String, NameEncoding) {
    if raw.is_ascii() {
        let text = String::from_utf8_lossy(raw).into_owned();
        return (text, if utf8_flag { NameEncoding::Utf8Flagged } else { NameEncoding::Ascii });
    }
    if let Ok(text) = std::str::from_utf8(raw) {
        return (text.to_owned(), if utf8_flag { NameEncoding::Utf8Flagged } else { NameEncoding::Utf8 });
    }
    let candidates = [
        (WINDOWS_1255.decode_without_bom_handling(raw).0.into_owned(), NameEncoding::Windows1255),
        (WINDOWS_1252.decode_without_bom_handling(raw).0.into_owned(), NameEncoding::Windows1252),
        (decode_cp437(raw), NameEncoding::Cp437),
        (decode_cp862(raw), NameEncoding::Cp862),
    ];
    let mut best: Option<(i32, String, NameEncoding)> = None;
    for (text, encoding) in candidates {
        let score = plausibility(&text);
        if best.as_ref().is_none_or(|(top, _, _)| score > *top) {
            best = Some((score, text, encoding));
        }
    }
    best.map_or_else(|| (String::from_utf8_lossy(raw).into_owned(), NameEncoding::Utf8), |(_, text, encoding)| (text, encoding))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_and_utf8_pass_through() {
        assert_eq!(decode_name(b"LIBRARY/a.xml", false), ("LIBRARY/a.xml".to_owned(), NameEncoding::Ascii));
        assert_eq!(decode_name(b"x", true).1, NameEncoding::Utf8Flagged);
        let hebrew = "ספר.xml";
        assert_eq!(decode_name(hebrew.as_bytes(), true), (hebrew.to_owned(), NameEncoding::Utf8Flagged));
        assert_eq!(decode_name(hebrew.as_bytes(), false), (hebrew.to_owned(), NameEncoding::Utf8));
        // The sample FLA's "UNI" + HEBREW POINT HOLAM name is real UTF-8.
        assert_eq!(decode_name("UNI\u{5b9}_FULL.xml".as_bytes(), true).0, "UNI\u{5b9}_FULL.xml");
    }

    #[test]
    fn windows_1255_hebrew_is_preferred() {
        // "ספר" in Windows-1255.
        let (text, enc) = decode_name(&[0xF1, 0xF4, 0xF8, b'.', b'x', b'm', b'l'], false);
        assert_eq!(text, "ספר.xml");
        assert_eq!(enc, NameEncoding::Windows1255);
    }

    #[test]
    fn cp862_hebrew_is_detected() {
        // "שלום" in CP862: shin 0x99, lamed 0x8C, vav 0x85, final mem 0x8D.
        let (text, enc) = decode_name(&[0x99, 0x8C, 0x85, 0x8D, b'.', b'p', b'n', b'g'], false);
        assert_eq!(text, "שלום.png");
        assert_eq!(enc, NameEncoding::Cp862);
    }

    #[test]
    fn western_names_stay_western() {
        // "Mädchen.xml" in Windows-1252: a lone 0xE4 would also be a Hebrew he, but gluing it to Latin letters loses.
        let (text, enc) = decode_name(b"M\xE4dchen.xml", false);
        assert_eq!(text, "Mädchen.xml");
        assert_eq!(enc, NameEncoding::Windows1252);
        // "Café" with CP437's 0x82 for é (also Hebrew bet in CP862).
        let (text, enc) = decode_name(b"Caf\x82.png", false);
        assert_eq!(text, "Café.png");
        assert_eq!(enc, NameEncoding::Cp437);
    }

    #[test]
    fn flagged_but_invalid_utf8_falls_back_to_legacy() {
        let (text, enc) = decode_name(&[0xF9, 0xEC, 0xE5, 0xED], true);
        assert_eq!(text, "שלום");
        assert_eq!(enc, NameEncoding::Windows1255);
    }

    #[test]
    fn code_page_tables_have_the_right_size_and_corners() {
        assert_eq!(CP437_HIGH.chars().count(), 128);
        assert_eq!(cp437_char(0x80), 'Ç');
        assert_eq!(cp437_char(0xFF), '\u{a0}');
        assert_eq!(cp437_char(0xE1), 'ß');
        assert_eq!(cp862_char(0x80), 'א');
        assert_eq!(cp862_char(0x9A), 'ת');
        assert_eq!(cp862_char(0x9B), '¢');
        assert_eq!(decode_cp437(b"abc\x82"), "abcé");
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        for b in 0..=255_u8 {
            let _ = decode_name(&[b, b, b'a'], false);
            let _ = decode_name(&[b'a', b], true);
        }
    }
}
