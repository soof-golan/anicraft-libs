//! System font discovery and per-script fallback.

mod common;

use anim_text::{Paragraph, RichText, Run, Script, TextEngine};
use common::{ARABIC, HEBREW, engine, engine_for, glyph_ids, run};

fn text_in(family: &str, text: &str) -> RichText {
    RichText::new(vec![Paragraph::new(vec![Run { family: family.to_owned(), ..run(text) }])])
}

#[test]
fn system_fonts_are_discovered_without_bundling_any() {
    let Some(mut engine) = engine() else { return };
    let families = engine.families();
    assert!(!families.is_empty());
    assert!(families.windows(2).all(|w| w[0].to_lowercase() <= w[1].to_lowercase()), "sorted");
    assert!(families.iter().all(|f| !f.starts_with('.')), "private families are hidden");
    let first = families[0].clone();
    assert!(engine.has_family(&first));
    assert!(!engine.has_family("Definitely Not A Font"));
}

#[test]
fn an_unknown_family_falls_back_to_a_working_default() {
    let Some(mut engine) = engine() else { return };
    let layout = engine.layout(&text_in("Definitely Not A Font", "Fallback works"), None);
    assert!(layout.runs[0].glyphs.iter().all(|g| g.id != 0));
    let list = engine.layout(&text_in("Definitely Not A Font, Also Missing", "Fallback works"), None);
    assert_eq!(glyph_ids(&layout), glyph_ids(&list));
}

#[test]
fn a_family_list_picks_the_first_installed_family() {
    let Some(mut engine) = engine() else { return };
    let Some(family) = engine.families().into_iter().find(|f| engine_has_latin(f)) else { return };
    let listed = engine.layout(&text_in(&format!("Missing One, \"{family}\""), "Same font"), None);
    let direct = engine.layout(&text_in(&family, "Same font"), None);
    assert_eq!(listed.runs[0].font.id, direct.runs[0].font.id);
    assert_eq!(glyph_ids(&listed), glyph_ids(&direct));
}

fn engine_has_latin(family: &str) -> bool {
    !family.contains("Symbol") && !family.contains("Dingbats") && !family.contains("Math") && !family.starts_with("Kacst")
}

#[test]
fn hebrew_text_in_a_latin_only_family_is_drawn_with_a_fallback_font() {
    let Some(mut engine) = engine_for(&[Script::Hebrew]) else { return };
    // Pick a family that is installed but has no Hebrew: any family the Hebrew chain does not contain and that fails
    // the coverage test would do; Latin-only families are common, so look for one.
    let chain = engine.fallback_chain(Script::Hebrew);
    let families = engine.families();
    let Some(latin_only) =
        families.iter().find(|f| !chain.contains(f) && !f.contains("Symbol") && !f.contains("Math") && !f.contains("Dingbats") && !f.starts_with("Kacst"))
    else {
        return;
    };
    let layout = engine.layout(&text_in(latin_only, &format!("Latin {HEBREW}")), None);
    let hebrew = layout.runs.iter().find(|r| r.rtl).expect("a right-to-left run");
    assert!(hebrew.glyphs.iter().all(|g| g.id != 0), "no .notdef boxes in Hebrew text set in {latin_only}");
}

#[test]
fn arabic_fallback_gives_real_glyphs_for_every_letter() {
    let Some(mut engine) = engine_for(&[Script::Arabic]) else { return };
    let layout = engine.layout(&text_in("Definitely Not A Font", ARABIC), None);
    assert!(layout.runs.iter().all(|r| r.glyphs.iter().all(|g| g.id != 0)), "{:?}", layout.runs.iter().map(|r| r.glyphs.len()).collect::<Vec<_>>());
}

#[test]
fn one_run_can_mix_scripts_from_different_fonts() {
    let Some(mut engine) = engine_for(&[Script::Hebrew, Script::Arabic]) else { return };
    let layout = engine.layout(&text_in("", &format!("Latin {HEBREW} {ARABIC} end")), Some(2000.0));
    assert!(layout.runs.iter().all(|r| r.glyphs.iter().all(|g| g.id != 0)), "every character has a glyph");
    assert!(layout.runs.iter().filter(|r| r.rtl).count() >= 2);
}

#[test]
fn cjk_text_uses_a_cjk_font_when_one_is_installed() {
    let Some(mut engine) = engine_for(&[Script::Han]) else { return };
    let layout = engine.layout(&text_in("", "日本語 中文 Latin"), None);
    assert!(layout.runs.iter().all(|r| r.glyphs.iter().all(|g| g.id != 0)));
    let chain = engine.fallback_chain(Script::Han);
    assert!(!chain.is_empty() && chain.len() <= 6);
    // Kana prefer the Japanese forms when a Japanese face is available.
    engine.set_language(Some("ja"));
    let ja = engine.layout(&text_in("", "あいう"), None);
    assert!(ja.runs[0].glyphs.iter().all(|g| g.id != 0));
}

#[test]
fn fallback_chains_only_contain_fonts_that_cover_the_script() {
    let Some(mut engine) = engine() else { return };
    for script in Script::ALL {
        let chain = engine.fallback_chain(script);
        assert!(chain.len() <= 6, "{script:?}: {chain:?}");
        let covers = engine.covers_script(script);
        assert_eq!(covers, !chain.is_empty());
        for family in chain {
            assert!(engine.has_family(&family), "{script:?}: {family}");
        }
    }
}

#[test]
fn an_engine_without_system_fonts_degrades_instead_of_failing() {
    let mut engine = TextEngine::without_system_fonts();
    assert!(engine.families().is_empty());
    assert!(!engine.covers_script(Script::Hebrew));
    let layout = engine.layout(&common::one("No fonts at all"), Some(100.0));
    assert!(!layout.lines.is_empty());
    assert!(layout.caret_rect(layout.caret_at(0), 1.0).is_some());
    assert!(engine.to_display(&layout).iter().all(|n| matches!(n, anim_display::Node::Fill(_) | anim_display::Node::Stroke(_))));
}

#[test]
fn registering_garbage_font_data_is_harmless() {
    let mut engine = TextEngine::without_system_fonts();
    assert!(engine.register_font(Vec::new()).is_empty());
    assert!(engine.register_font(b"not a font at all, just text".to_vec()).is_empty());
    assert!(engine.register_font(vec![0; 4096]).is_empty());
    assert!(engine.families().is_empty());
}

#[test]
fn registered_font_data_becomes_available_by_name() {
    // Borrow a real font file from the system to register it as user data; skip when none can be found.
    let Some(path) = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/Library/Fonts/Arial.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ]
    .iter()
    .find(|p| std::path::Path::new(p).exists()) else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let mut engine = TextEngine::without_system_fonts();
    let names = engine.register_font(bytes);
    assert!(!names.is_empty());
    assert!(engine.has_family(&names[0]));
    let layout = engine.layout(&text_in(&names[0], "Registered"), None);
    assert!(layout.runs[0].glyphs.iter().all(|g| g.id != 0));
    // With no generic sans serif on this engine, the first registered family stands in for it.
    let anonymous = engine.layout(&text_in("", "Registered"), None);
    assert_eq!(glyph_ids(&layout), glyph_ids(&anonymous));
}
