//! Font names as documents store them: PostScript names, full names, names of fonts this machine lacks.

mod common;

use anim_text::{Paragraph, RichText, Run, TextEngine};
use common::{glyph_ids, run, system_engine as engine};

fn text_in(family: &str) -> RichText {
    RichText::new(vec![Paragraph::new(vec![Run { family: family.to_owned(), ..run("Resolved fonts") }])])
}

#[test]
fn installed_family_names_resolve_whatever_the_case_or_spacing() {
    let Some(mut engine) = engine() else { return };
    let Some(family) = engine.families().into_iter().find(|f| f.contains(' ') && f.is_ascii()) else { return };
    for spelling in [family.clone(), family.to_lowercase(), family.replace(' ', ""), family.to_uppercase()] {
        let resolved = engine.resolve_font_name(&spelling);
        assert_eq!(resolved.family, family, "{spelling:?}");
        assert!(resolved.exact && !resolved.bold && !resolved.italic);
    }
}

#[test]
fn postscript_and_full_names_give_family_and_style() {
    let Some(mut engine) = engine() else { return };
    if !engine.has_family("DejaVu Sans") {
        return;
    }
    let bold = engine.resolve_font_name("DejaVuSans-Bold");
    assert_eq!((bold.family.as_str(), bold.bold, bold.italic), ("DejaVu Sans", true, false));
    let both = engine.resolve_font_name("DejaVuSans-BoldOblique");
    assert_eq!((both.family.as_str(), both.bold, both.italic), ("DejaVu Sans", true, true));
    let full = engine.resolve_font_name("DejaVu Sans Bold");
    assert_eq!((full.family.as_str(), full.bold), ("DejaVu Sans", true));
    // No hyphen and no space: only the name tables inside the fonts can say what this is.
    let glued = engine.resolve_font_name("DejaVuSansBold");
    assert_eq!((glued.family.as_str(), glued.bold, glued.exact), ("DejaVu Sans", true, true));
}

#[test]
fn well_known_missing_fonts_get_metric_compatible_substitutes() {
    let Some(mut engine) = engine() else { return };
    let cases = [
        ("Arial-BoldMT", "Arial", "Liberation Sans", true, false),
        ("TimesNewRomanPS-ItalicMT", "Times New Roman", "Liberation Serif", false, true),
        ("CourierNewPSMT", "Courier New", "Liberation Mono", false, false),
    ];
    for (name, original, substitute, bold, italic) in cases {
        if engine.has_family(original) || !engine.has_family(substitute) {
            continue;
        }
        let resolved = engine.resolve_font_name(name);
        assert_eq!((resolved.family.as_str(), resolved.bold, resolved.italic, resolved.exact), (substitute, bold, italic, false), "{name}");
    }
}

#[test]
fn names_nobody_has_come_back_as_guesses() {
    let Some(mut engine) = engine() else { return };
    let guess = engine.resolve_font_name("Zzyzx-BoldItalicMT");
    assert_eq!((guess.family.as_str(), guess.bold, guess.italic, guess.exact), ("Zzyzx", true, true, false));
    let empty = engine.resolve_font_name("   ");
    assert_eq!(empty.family, "");
    let long = engine.resolve_font_name(&"x".repeat(10_000));
    assert!(long.family.len() <= 200);
}

#[test]
fn layout_resolves_names_that_are_not_installed_families() {
    let Some(mut engine) = engine() else { return };
    if !engine.has_family("DejaVu Sans") {
        return;
    }
    let by_ps_name = engine.layout(&text_in("DejaVuSans-Bold"), None);
    let explicit = engine.layout(&RichText::new(vec![Paragraph::new(vec![Run { family: "DejaVu Sans".into(), bold: true, ..run("Resolved fonts") }])]), None);
    assert_eq!(by_ps_name.runs[0].font.id, explicit.runs[0].font.id, "the bold DejaVu Sans face either way");
    assert_eq!(glyph_ids(&by_ps_name), glyph_ids(&explicit));
    assert!((by_ps_name.bounds.width() - explicit.bounds.width()).abs() < 0.01);
    let plain = engine.layout(&text_in("DejaVu Sans"), None);
    assert!(by_ps_name.bounds.width() > plain.bounds.width(), "bold is wider");
}

#[test]
fn a_registered_font_is_found_by_its_names() {
    let Some(path) = ["/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf"]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
    else {
        return;
    };
    let mut engine = TextEngine::without_system_fonts();
    let names = engine.register_font(std::fs::read(path).unwrap());
    assert_eq!(names, vec!["DejaVu Sans".to_owned()]);
    let resolved = engine.resolve_font_name("DejaVuSans-Bold");
    assert_eq!((resolved.family.as_str(), resolved.bold), ("DejaVu Sans", true));
    let glued = engine.resolve_font_name("DejaVuSansBold");
    assert_eq!((glued.family.as_str(), glued.exact), ("DejaVu Sans", true));
}
