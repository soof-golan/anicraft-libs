//! Reading font names the way documents store them.
//!
//! Animate (XFL), Character Animator and PSD files name fonts by PostScript name (`Arial-BoldMT`,
//! `MyriadPro-Regular`, `TimesNewRomanPSMT`), by full name (`Arial Bold`) or by family (`Arial`), and often name fonts
//! the machine does not have. These pure helpers take such names apart; `fonts.rs` matches the pieces against what is
//! installed.

/// Lower-cases `s` and keeps only letters and digits, so `"DejaVu Sans"`, `"DejaVuSans"` and `"dejavu-sans"` compare
/// equal.
#[must_use]
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// A font name split into the family-ish base and the style it asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub base: String,
    pub bold: bool,
    pub italic: bool,
}

/// Words that name a style rather than a family when they end a space-separated font name.
const STYLE_WORDS: [&str; 13] =
    ["regular", "normal", "book", "bold", "italic", "oblique", "bolditalic", "boldoblique", "semibold", "demibold", "black", "heavy", "medium"];

/// Splits `name` into base and style: `"Arial-BoldMT"` is (`"Arial"`, bold), `"Times New Roman Bold Italic"` is
/// (`"Times New Roman"`, bold, italic), `"ArialMT"` is (`"Arial"`).
#[must_use]
pub fn parse_style(name: &str) -> Parsed {
    let name = name.trim();
    let (base, style) = if let Some((base, style)) = name.rsplit_once('-') {
        (base.to_owned(), style.to_owned())
    } else {
        let mut words: Vec<&str> = name.split_whitespace().collect();
        let mut style = String::new();
        while words.len() > 1 && words.last().is_some_and(|w| STYLE_WORDS.contains(&w.to_lowercase().as_str())) {
            if let Some(word) = words.pop() {
                style.push_str(word);
            }
        }
        (words.join(" "), style)
    };
    let style = style.to_lowercase();
    let bold = ["bold", "black", "heavy", "demi"].iter().any(|w| style.contains(w));
    let italic = style.contains("italic") || style.contains("oblique") || style == "it" || style.ends_with("boldit");
    Parsed { base: strip_foundry_suffix(&base).to_owned(), bold, italic }
}

/// Drops the `MT`, `PS` or `PSMT` that PostScript names append to a family (`ArialMT`, `TimesNewRomanPSMT`), but not
/// when it is a word of its own (`Gill Sans MT`).
fn strip_foundry_suffix(base: &str) -> &str {
    for suffix in ["PSMT", "MT", "PS"] {
        if let Some(rest) = base.strip_suffix(suffix)
            && rest.chars().next_back().is_some_and(|c| c.is_lowercase() || c.is_ascii_digit())
        {
            return rest;
        }
    }
    base
}

/// Installed families that can stand in for a commonly used font that is not installed, best first, keyed by
/// [`norm`]alised name. Mostly the metric-compatible substitutes (Liberation, Arimo, Tinos, Cousine, Carlito, Caladea).
#[must_use]
pub fn substitutes(normalised: &str) -> &'static [&'static str] {
    const SANS: &[&str] = &["Liberation Sans", "Arimo", "Nimbus Sans", "Helvetica", "Arial", "FreeSans", "DejaVu Sans"];
    const SERIF: &[&str] = &["Liberation Serif", "Tinos", "Nimbus Roman", "Times", "Times New Roman", "FreeSerif", "DejaVu Serif"];
    const MONO: &[&str] = &["Liberation Mono", "Cousine", "Nimbus Mono PS", "Courier", "Courier New", "FreeMono", "DejaVu Sans Mono"];
    match normalised {
        "arial" | "helvetica" | "helveticaneue" | "arialnarrow" | "arialunicodems" => SANS,
        "timesnewroman" | "times" => SERIF,
        "couriernew" | "courier" | "lucidaconsole" | "consolas" | "monaco" | "menlo" => MONO,
        "verdana" | "tahoma" | "trebuchetms" | "lucidagrande" | "segoeui" | "geneva" => &["DejaVu Sans", "Bitstream Vera Sans", "Liberation Sans"],
        "georgia" | "palatino" | "palatinolinotype" | "bookantiqua" => &["Gelasio", "DejaVu Serif", "Liberation Serif"],
        "calibri" => &["Carlito", "Lato", "Liberation Sans", "DejaVu Sans"],
        "cambria" => &["Caladea", "Liberation Serif", "DejaVu Serif"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(name: &str) -> (String, bool, bool) {
        let p = parse_style(name);
        (p.base, p.bold, p.italic)
    }

    #[test]
    fn norm_ignores_case_spaces_and_punctuation() {
        assert_eq!(norm("DejaVu Sans"), "dejavusans");
        assert_eq!(norm("dejavu-sans"), "dejavusans");
        assert_eq!(norm("  "), "");
        assert_eq!(norm("Ünï Sans"), "ünïsans");
    }

    #[test]
    fn postscript_names_split_into_family_and_style() {
        assert_eq!(parsed("Arial-BoldMT"), ("Arial".into(), true, false));
        assert_eq!(parsed("Arial-BoldItalicMT"), ("Arial".into(), true, true));
        assert_eq!(parsed("ArialMT"), ("Arial".into(), false, false));
        assert_eq!(parsed("TimesNewRomanPSMT"), ("TimesNewRoman".into(), false, false));
        assert_eq!(parsed("TimesNewRomanPS-ItalicMT"), ("TimesNewRoman".into(), false, true));
        assert_eq!(parsed("Helvetica-Oblique"), ("Helvetica".into(), false, true));
        assert_eq!(parsed("MyriadPro-BoldIt"), ("MyriadPro".into(), true, true));
        assert_eq!(parsed("MyriadPro-Regular"), ("MyriadPro".into(), false, false));
        assert_eq!(parsed("SegoeUI-Semibold"), ("SegoeUI".into(), true, false));
        assert_eq!(parsed("DejaVuSans-Bold"), ("DejaVuSans".into(), true, false));
    }

    #[test]
    fn full_names_and_family_names_split_too() {
        assert_eq!(parsed("Arial Bold"), ("Arial".into(), true, false));
        assert_eq!(parsed("Times New Roman Bold Italic"), ("Times New Roman".into(), true, true));
        assert_eq!(parsed("Gill Sans MT"), ("Gill Sans MT".into(), false, false), "MT is a word of the name here");
        assert_eq!(parsed("Open Sans"), ("Open Sans".into(), false, false));
        assert_eq!(parsed("Regular"), ("Regular".into(), false, false), "a lone style word is kept as the name");
        assert_eq!(parsed(""), (String::new(), false, false));
    }

    #[test]
    fn well_known_fonts_have_substitutes() {
        assert_eq!(substitutes("arial").first(), Some(&"Liberation Sans"));
        assert_eq!(substitutes("timesnewroman").first(), Some(&"Liberation Serif"));
        assert_eq!(substitutes("couriernew").first(), Some(&"Liberation Mono"));
        assert_eq!(substitutes("unheardof").len(), 0);
    }
}
