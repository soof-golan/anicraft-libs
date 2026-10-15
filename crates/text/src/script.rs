//! Scripts that need a font beyond the Latin default, with the families to try for each.
//!
//! The first font of a run is whatever the author picked; characters it lacks fall back per script. The tables here
//! name well-known families on Windows, macOS and Linux in preference order. The engine only uses the ones that are
//! installed *and* really cover the script's sample text; when none does it scans the installed families for one that
//! does (see `fonts.rs`), so unusual systems still get a fallback and nothing here is a hard requirement.

/// A script with its own fallback chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Script {
    Hebrew,
    Arabic,
    /// Han ideographs, kana and CJK punctuation.
    Han,
    Hangul,
    Thai,
    Lao,
    Khmer,
    Myanmar,
    Devanagari,
    Bengali,
    Gurmukhi,
    Gujarati,
    Tamil,
    Telugu,
    Kannada,
    Malayalam,
    Sinhala,
    Armenian,
    Georgian,
    Ethiopic,
}

impl Script {
    /// Every script, in a stable order.
    pub const ALL: [Self; 20] = [
        Self::Hebrew,
        Self::Arabic,
        Self::Han,
        Self::Hangul,
        Self::Thai,
        Self::Lao,
        Self::Khmer,
        Self::Myanmar,
        Self::Devanagari,
        Self::Bengali,
        Self::Gurmukhi,
        Self::Gujarati,
        Self::Tamil,
        Self::Telugu,
        Self::Kannada,
        Self::Malayalam,
        Self::Sinhala,
        Self::Armenian,
        Self::Georgian,
        Self::Ethiopic,
    ];

    /// The script of `c`, or `None` for Latin, Greek, Cyrillic, digits, punctuation and anything unlisted.
    #[must_use]
    pub fn of(c: char) -> Option<Self> {
        let u = u32::from(c);
        if u < 0x0530 {
            return None;
        }
        RANGES.iter().find(|(lo, hi, _)| (*lo..=*hi).contains(&u)).map(|(_, _, s)| *s)
    }

    /// Whether this script right-to-left.
    #[must_use]
    pub const fn is_rtl(self) -> bool {
        matches!(self, Self::Hebrew | Self::Arabic)
    }

    /// Characters a font must map to count as covering the script (common letters, not exotica).
    #[must_use]
    pub const fn sample(self) -> &'static str {
        match self {
            Self::Hebrew => "אבגדהוזחטיכלמנסעפצקרשת",
            Self::Arabic => "ابتثجحخدذرزسشصضطظعغفقكلمنهوي",
            Self::Han => "中文字日本語あいうアイウ",
            Self::Hangul => "한국어글자",
            Self::Thai => "กขคงจฉชซญ",
            Self::Lao => "ກຂຄງຈສຊ",
            Self::Khmer => "កខគឃងចឆ",
            Self::Myanmar => "ကခဂဃငစဆ",
            Self::Devanagari => "अआइईउऊकखगघ",
            Self::Bengali => "অআইঈউঊকখগঘ",
            Self::Gurmukhi => "ਅਆਇਈਉਊਕਖਗਘ",
            Self::Gujarati => "અઆઇઈઉઊકખગઘ",
            Self::Tamil => "அஆஇஈஉஊகஙச",
            Self::Telugu => "అఆఇఈఉఊకఖగఘ",
            Self::Kannada => "ಅಆಇಈಉಊಕಖಗಘ",
            Self::Malayalam => "അആഇഈഉഊകഖഗഘ",
            Self::Sinhala => "අආඉඊඋඌකඛග",
            Self::Armenian => "ԱԲԳԴԵԶԷԸԹ",
            Self::Georgian => "აბგდევზთი",
            Self::Ethiopic => "ሀሁሂሃሄህሆለሉ",
        }
    }

    /// Families to try first, best first (Windows, macOS and Linux names mixed; uninstalled ones are skipped).
    #[must_use]
    pub const fn preferred_families(self) -> &'static [&'static str] {
        match self {
            Self::Hebrew => {
                &["Noto Sans Hebrew", "Arial Hebrew", "Segoe UI", "Arial", "Tahoma", "David", "Noto Serif Hebrew", "Liberation Sans", "DejaVu Sans"]
            }
            Self::Arabic => &[
                "Noto Sans Arabic",
                "Noto Naskh Arabic",
                "Geeza Pro",
                "Segoe UI",
                "Tahoma",
                "Arial",
                "Dubai",
                "Noto Kufi Arabic",
                "DejaVu Sans",
                "Noto Sans Arabic UI",
            ],
            Self::Han => &[
                "Noto Sans CJK SC",
                "Noto Sans CJK JP",
                "PingFang SC",
                "Hiragino Sans",
                "Microsoft YaHei",
                "Yu Gothic",
                "Meiryo",
                "SimSun",
                "WenQuanYi Micro Hei",
                "Source Han Sans",
                "Droid Sans Fallback",
            ],
            Self::Hangul => &["Noto Sans CJK KR", "Apple SD Gothic Neo", "Malgun Gothic", "NanumGothic", "Noto Sans KR", "Source Han Sans K"],
            Self::Thai => &["Noto Sans Thai", "Thonburi", "Leelawadee UI", "Tahoma", "Sukhumvit Set", "Loma", "Garuda"],
            Self::Lao => &["Noto Sans Lao", "Lao Sangam MN", "Leelawadee UI", "Phetsarath OT"],
            Self::Khmer => &["Noto Sans Khmer", "Khmer Sangam MN", "Leelawadee UI", "Khmer OS"],
            Self::Myanmar => &["Noto Sans Myanmar", "Myanmar Sangam MN", "Myanmar Text", "Padauk"],
            Self::Devanagari => &["Noto Sans Devanagari", "Kohinoor Devanagari", "Devanagari Sangam MN", "Nirmala UI", "Lohit Devanagari"],
            Self::Bengali => &["Noto Sans Bengali", "Kohinoor Bangla", "Bangla Sangam MN", "Nirmala UI", "Lohit Bengali"],
            Self::Gurmukhi => &["Noto Sans Gurmukhi", "Gurmukhi Sangam MN", "Nirmala UI", "Lohit Gurmukhi"],
            Self::Gujarati => &["Noto Sans Gujarati", "Kohinoor Gujarati", "Gujarati Sangam MN", "Nirmala UI", "Lohit Gujarati"],
            Self::Tamil => &["Noto Sans Tamil", "Tamil Sangam MN", "Nirmala UI", "Lohit Tamil"],
            Self::Telugu => &["Noto Sans Telugu", "Kohinoor Telugu", "Telugu Sangam MN", "Nirmala UI", "Lohit Telugu"],
            Self::Kannada => &["Noto Sans Kannada", "Kannada Sangam MN", "Nirmala UI", "Lohit Kannada"],
            Self::Malayalam => &["Noto Sans Malayalam", "Malayalam Sangam MN", "Nirmala UI", "Lohit Malayalam"],
            Self::Sinhala => &["Noto Sans Sinhala", "Sinhala Sangam MN", "Nirmala UI"],
            Self::Armenian => &["Noto Sans Armenian", "Mshtakan", "Sylfaen", "DejaVu Sans"],
            Self::Georgian => &["Noto Sans Georgian", "Sylfaen", "DejaVu Sans"],
            Self::Ethiopic => &["Noto Sans Ethiopic", "Kefa", "Nyala", "Abyssinica SIL"],
        }
    }

    /// The BCP 47 language tag fonts and shapers use by default for this script, where one language dominates.
    #[must_use]
    pub const fn default_language(self) -> Option<&'static str> {
        match self {
            Self::Hebrew => Some("he"),
            Self::Arabic => Some("ar"),
            Self::Hangul => Some("ko"),
            Self::Thai => Some("th"),
            _ => None,
        }
    }
}

/// Whether text in `c`'s script is cursive: letters join, so adding space between characters would break the word.
#[must_use]
pub fn is_cursive(c: char) -> bool {
    let u = u32::from(c);
    matches!(u, 0x0600..=0x06FF | 0x0700..=0x074F | 0x0750..=0x077F | 0x07C0..=0x07FF | 0x0870..=0x08FF | 0x1800..=0x18AF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

/// The scripts used in `text`, in order of first appearance (at most `limit`).
#[must_use]
pub fn scripts_in(text: &str, limit: usize) -> Vec<Script> {
    let mut found: Vec<Script> = Vec::new();
    for c in text.chars() {
        if let Some(s) = Script::of(c)
            && !found.contains(&s)
        {
            found.push(s);
            if found.len() >= limit {
                break;
            }
        }
    }
    found
}

/// Inclusive code point ranges per script.
const RANGES: &[(u32, u32, Script)] = &[
    (0x0530, 0x058F, Script::Armenian),
    (0x0590, 0x05FF, Script::Hebrew),
    (0x0600, 0x06FF, Script::Arabic),
    (0x0750, 0x077F, Script::Arabic),
    (0x0870, 0x08FF, Script::Arabic),
    (0x0900, 0x097F, Script::Devanagari),
    (0x0980, 0x09FF, Script::Bengali),
    (0x0A00, 0x0A7F, Script::Gurmukhi),
    (0x0A80, 0x0AFF, Script::Gujarati),
    (0x0B80, 0x0BFF, Script::Tamil),
    (0x0C00, 0x0C7F, Script::Telugu),
    (0x0C80, 0x0CFF, Script::Kannada),
    (0x0D00, 0x0D7F, Script::Malayalam),
    (0x0D80, 0x0DFF, Script::Sinhala),
    (0x0E00, 0x0E7F, Script::Thai),
    (0x0E80, 0x0EFF, Script::Lao),
    (0x1000, 0x109F, Script::Myanmar),
    (0x10A0, 0x10FF, Script::Georgian),
    (0x1100, 0x11FF, Script::Hangul),
    (0x1200, 0x139F, Script::Ethiopic),
    (0x1780, 0x17FF, Script::Khmer),
    (0x1C90, 0x1CBF, Script::Georgian),
    (0x2E80, 0x2FDF, Script::Han),
    (0x3000, 0x303F, Script::Han),
    (0x3040, 0x30FF, Script::Han),
    (0x3100, 0x312F, Script::Han),
    (0x3130, 0x318F, Script::Hangul),
    (0x3190, 0x33FF, Script::Han),
    (0x3400, 0x4DBF, Script::Han),
    (0x4E00, 0x9FFF, Script::Han),
    (0xA960, 0xA97F, Script::Hangul),
    (0xAC00, 0xD7FF, Script::Hangul),
    (0xF900, 0xFAFF, Script::Han),
    (0xFB1D, 0xFB4F, Script::Hebrew),
    (0xFB50, 0xFDFF, Script::Arabic),
    (0xFE30, 0xFE4F, Script::Han),
    (0xFE70, 0xFEFF, Script::Arabic),
    (0xFF00, 0xFFEF, Script::Han),
    (0x20000, 0x2FA1F, Script::Han),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_characters() {
        assert_eq!(Script::of('a'), None);
        assert_eq!(Script::of('я'), None);
        assert_eq!(Script::of('ש'), Some(Script::Hebrew));
        assert_eq!(Script::of('ب'), Some(Script::Arabic));
        assert_eq!(Script::of('ﻻ'), Some(Script::Arabic));
        assert_eq!(Script::of('中'), Some(Script::Han));
        assert_eq!(Script::of('あ'), Some(Script::Han));
        assert_eq!(Script::of('한'), Some(Script::Hangul));
        assert_eq!(Script::of('ก'), Some(Script::Thai));
    }

    #[test]
    fn samples_belong_to_their_script() {
        for script in Script::ALL {
            assert_ne!(script.preferred_families().len(), 0);
            for c in script.sample().chars() {
                assert_eq!(Script::of(c), Some(script), "{c:?} of {script:?}");
            }
        }
    }

    #[test]
    fn scripts_in_keeps_first_appearance_order_and_limit() {
        assert_eq!(scripts_in("abc שלום سلام 中", 8), vec![Script::Hebrew, Script::Arabic, Script::Han]);
        assert_eq!(scripts_in("שلام中", 2).len(), 2);
        assert_eq!(scripts_in("plain", 8), Vec::<Script>::new());
    }

    #[test]
    fn arabic_is_cursive_and_hebrew_is_not() {
        assert!(is_cursive('ب') && !is_cursive('ש') && !is_cursive('a'));
        assert!(Script::Hebrew.is_rtl() && Script::Arabic.is_rtl() && !Script::Han.is_rtl());
    }
}
