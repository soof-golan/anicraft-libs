//! Dark and light palettes, installed into egui, plus right-to-left safe text helpers.

use std::borrow::Cow;

use egui::{Color32, Context, CornerRadius, Stroke, Visuals};
use serde::{Deserialize, Serialize};
use unicode_bidi::BidiInfo;

/// Which palette is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// The colour tokens every widget in this crate draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Window and panel background.
    pub panel: Color32,
    /// Slightly raised surfaces (headers, rulers, tab strips).
    pub surface: Color32,
    /// Recessed areas (timeline grid, pasteboard, text fields).
    pub recessed: Color32,
    /// Default stage pasteboard (outside the document rectangle).
    pub pasteboard: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub border: Color32,
    pub grid_line: Color32,
    /// Every fifth frame in the timeline grid.
    pub grid_line_strong: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub playhead: Color32,
    pub keyframe: Color32,
    pub span: Color32,
    pub span_selected: Color32,
    pub classic_tween: Color32,
    pub shape_tween: Color32,
    pub motion_tween: Color32,
    pub onion: Color32,
    pub guide: Color32,
    pub warning: Color32,
}

impl Palette {
    /// The dark palette.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            panel: Color32::from_rgb(0x2b, 0x2b, 0x2d),
            surface: Color32::from_rgb(0x38, 0x38, 0x3b),
            recessed: Color32::from_rgb(0x1f, 0x1f, 0x21),
            pasteboard: Color32::from_rgb(0x3d, 0x3d, 0x40),
            text: Color32::from_rgb(0xe4, 0xe4, 0xe6),
            text_weak: Color32::from_rgb(0x9a, 0x9a, 0xa0),
            border: Color32::from_rgb(0x4a, 0x4a, 0x4f),
            grid_line: Color32::from_rgb(0x34, 0x34, 0x37),
            grid_line_strong: Color32::from_rgb(0x44, 0x44, 0x49),
            accent: Color32::from_rgb(0x4c, 0x9a, 0xff),
            accent_soft: Color32::from_rgba_premultiplied(0x1a, 0x36, 0x58, 0xa0),
            playhead: Color32::from_rgb(0xe5, 0x39, 0x35),
            keyframe: Color32::from_rgb(0xf2, 0xf2, 0xf2),
            span: Color32::from_rgb(0x4d, 0x4d, 0x52),
            span_selected: Color32::from_rgb(0x2f, 0x6f, 0xc0),
            classic_tween: Color32::from_rgb(0x8c, 0x6b, 0xd1),
            shape_tween: Color32::from_rgb(0x5a, 0xb0, 0x5a),
            motion_tween: Color32::from_rgb(0x4c, 0x9a, 0xd8),
            onion: Color32::from_rgb(0xff, 0xb3, 0x4d),
            guide: Color32::from_rgb(0x39, 0xd3, 0xc9),
            warning: Color32::from_rgb(0xe0, 0xa0, 0x30),
        }
    }

    /// The light palette.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            panel: Color32::from_rgb(0xe9, 0xe9, 0xeb),
            surface: Color32::from_rgb(0xf5, 0xf5, 0xf6),
            recessed: Color32::from_rgb(0xff, 0xff, 0xff),
            pasteboard: Color32::from_rgb(0xc9, 0xc9, 0xcd),
            text: Color32::from_rgb(0x1d, 0x1d, 0x20),
            text_weak: Color32::from_rgb(0x60, 0x60, 0x66),
            border: Color32::from_rgb(0xb6, 0xb6, 0xbc),
            grid_line: Color32::from_rgb(0xe2, 0xe2, 0xe5),
            grid_line_strong: Color32::from_rgb(0xd0, 0xd0, 0xd5),
            accent: Color32::from_rgb(0x1a, 0x73, 0xe8),
            accent_soft: Color32::from_rgba_premultiplied(0x9c, 0xc3, 0xf5, 0xa0),
            playhead: Color32::from_rgb(0xd3, 0x2f, 0x2f),
            keyframe: Color32::from_rgb(0x20, 0x20, 0x24),
            span: Color32::from_rgb(0xcf, 0xcf, 0xd5),
            span_selected: Color32::from_rgb(0x8f, 0xb8, 0xf0),
            classic_tween: Color32::from_rgb(0x8a, 0x63, 0xd2),
            shape_tween: Color32::from_rgb(0x3f, 0x9a, 0x3f),
            motion_tween: Color32::from_rgb(0x2b, 0x86, 0xc5),
            onion: Color32::from_rgb(0xe0, 0x8a, 0x1e),
            guide: Color32::from_rgb(0x00, 0xa8, 0x9d),
            warning: Color32::from_rgb(0xb8, 0x7a, 0x00),
        }
    }
}

impl Theme {
    /// The palette for this theme.
    #[must_use]
    pub const fn palette(self) -> Palette {
        match self {
            Self::Dark => Palette::dark(),
            Self::Light => Palette::light(),
        }
    }

    /// The other theme.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    /// egui visuals matching this theme.
    #[must_use]
    pub fn visuals(self) -> Visuals {
        let p = self.palette();
        let mut v = match self {
            Self::Dark => Visuals::dark(),
            Self::Light => Visuals::light(),
        };
        v.panel_fill = p.panel;
        v.window_fill = p.surface;
        v.extreme_bg_color = p.recessed;
        v.faint_bg_color = p.surface;
        v.override_text_color = Some(p.text);
        v.selection.bg_fill = p.accent_soft;
        v.selection.stroke = Stroke::new(1.0, p.accent);
        v.hyperlink_color = p.accent;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
        v.widgets.noninteractive.bg_fill = p.panel;
        v.widgets.inactive.bg_fill = p.surface;
        v.widgets.inactive.weak_bg_fill = p.surface;
        v.widgets.hovered.weak_bg_fill = p.grid_line_strong;
        v.widgets.hovered.bg_fill = p.grid_line_strong;
        v.widgets.active.bg_fill = p.accent_soft;
        v.widgets.active.weak_bg_fill = p.accent_soft;
        let radius = CornerRadius::same(3);
        v.widgets.noninteractive.corner_radius = radius;
        v.widgets.inactive.corner_radius = radius;
        v.widgets.hovered.corner_radius = radius;
        v.widgets.active.corner_radius = radius;
        v
    }

    /// Installs this theme's visuals into the egui context.
    pub fn apply(self, ctx: &Context) {
        ctx.set_visuals(self.visuals());
    }
}

/// The palette of the theme currently active in `ctx` (dark when the visuals are dark).
#[must_use]
pub fn current_palette(ctx: &Context) -> Palette {
    if ctx.global_style().visuals.dark_mode { Palette::dark() } else { Palette::light() }
}

/// Whether `text` contains any strongly right-to-left character (Hebrew, Arabic, Syriac, Thaana, ...).
#[must_use]
pub fn is_rtl(text: &str) -> bool {
    text.chars().any(|c| unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::R || unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::AL)
}

/// Returns `text` in visual order so egui (which lays glyphs out left to right) shows right-to-left runs correctly.
///
/// Text without right-to-left characters is returned unchanged and unallocated. Mixed text keeps its left-to-right
/// runs (numbers, Latin words) intact inside the right-to-left paragraph.
#[must_use]
pub fn visual(text: &str) -> Cow<'_, str> {
    if !is_rtl(text) {
        return Cow::Borrowed(text);
    }
    let info = BidiInfo::new(text, None);
    let mut out = String::with_capacity(text.len());
    for para in &info.paragraphs {
        // The paragraph range includes its trailing separator, which reorder_line keeps in place.
        out.push_str(&info.reorder_line(para, para.range.clone()));
    }
    Cow::Owned(out)
}

/// Candidate system fonts that cover Hebrew and Arabic, per platform. First readable file of each script wins.
const FALLBACK_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/System/Library/Fonts/ArialHB.ttc",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/SFHebrew.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
];

/// Appends readable system fonts as fallbacks to egui's proportional and monospace families.
///
/// Candidates are `DejaVu` Sans, Noto Hebrew/Arabic, Arial and Segoe UI, whichever exist; with one of them Hebrew
/// and Arabic labels render. Returns how many were added.
#[must_use = "zero means no system font covers Hebrew/Arabic: warn the user or bundle a font"]
pub fn install_fallback_fonts(ctx: &Context) -> usize {
    let mut fonts = egui::FontDefinitions::default();
    let mut added = 0usize;
    for path in FALLBACK_CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = format!("anim-ui-fallback-{path}");
        fonts.font_data.insert(name.clone(), egui::FontData::from_owned(bytes).into());
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
        added = added.saturating_add(1);
    }
    if added > 0 {
        ctx.set_fonts(fonts);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_text_is_untouched() {
        assert!(matches!(visual("Layer 1"), Cow::Borrowed("Layer 1")));
        assert!(!is_rtl("Layer 1"));
    }

    #[test]
    fn hebrew_is_reversed_for_display() {
        let shown = visual("שלום");
        assert_eq!(shown, "םולש");
        assert!(is_rtl("שלום"));
    }

    #[test]
    fn embedded_numbers_keep_order() {
        let shown = visual("שכבה 12");
        assert!(shown.starts_with("12"), "{shown}");
        assert!(shown.ends_with("הבכש"), "{shown}");
    }

    #[test]
    fn arabic_counts_as_rtl() {
        assert!(is_rtl("مرحبا"));
    }

    #[test]
    fn palettes_differ_and_toggle() {
        assert_ne!(Theme::Dark.palette(), Theme::Light.palette());
        assert_eq!(Theme::Dark.toggled(), Theme::Light);
        assert!(Theme::Dark.visuals().dark_mode);
        assert!(!Theme::Light.visuals().dark_mode);
    }
}
