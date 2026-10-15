//! Fill and stroke styles of an edge shape (XFL `<FillStyle>` and `<StrokeStyle>`).
//!
//! Gradient and bitmap matrices are kept exactly as XFL stores them. For gradients the matrix maps gradient space
//! (a square from -819.2 to 819.2) into the shape's pixel space, i.e. the edges divided by 20. For
//! bitmap fills the matrix is in the same units as SWF stores it (a bitmap shown at 100% has a scale of 20 and a
//! translation in pixels); [`BITMAP_MATRIX_SCALE`] converts it.

use anim_core::{Affine, Rgba8};
use anim_display::{Cap, GradientStop, Interpolation, Join, Spread, StrokeScale};
use serde::{Deserialize, Serialize};

/// Linear scale that turns a stored bitmap-fill matrix into one that maps image pixels to shape pixels.
pub const BITMAP_MATRIX_SCALE: f64 = 1.0 / 20.0;

/// How XFL spells the options that map onto display-list enums.
pub trait XflName: Sized + Copy {
    /// Parses the attribute value; `None` for anything unknown.
    fn from_xfl(name: &str) -> Option<Self>;
    /// The attribute value Animate writes.
    fn xfl_name(self) -> &'static str;
}

impl XflName for Cap {
    fn from_xfl(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Self::None),
            "round" => Some(Self::Round),
            "square" => Some(Self::Square),
            _ => None,
        }
    }

    fn xfl_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Round => "round",
            Self::Square => "square",
        }
    }
}

impl XflName for Join {
    fn from_xfl(name: &str) -> Option<Self> {
        match name {
            "miter" => Some(Self::Miter),
            "round" => Some(Self::Round),
            "bevel" => Some(Self::Bevel),
            _ => None,
        }
    }

    fn xfl_name(self) -> &'static str {
        match self {
            Self::Miter => "miter",
            Self::Round => "round",
            Self::Bevel => "bevel",
        }
    }
}

impl XflName for StrokeScale {
    fn from_xfl(name: &str) -> Option<Self> {
        match name {
            "normal" => Some(Self::Normal),
            "horizontal" => Some(Self::Horizontal),
            "vertical" => Some(Self::Vertical),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    fn xfl_name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
            Self::None => "none",
        }
    }
}

impl XflName for Spread {
    fn from_xfl(name: &str) -> Option<Self> {
        match name {
            "extend" => Some(Self::Pad),
            "reflect" => Some(Self::Reflect),
            "repeat" => Some(Self::Repeat),
            _ => None,
        }
    }

    fn xfl_name(self) -> &'static str {
        match self {
            Self::Pad => "extend",
            Self::Reflect => "reflect",
            Self::Repeat => "repeat",
        }
    }
}

impl XflName for Interpolation {
    fn from_xfl(name: &str) -> Option<Self> {
        match name {
            "RGB" => Some(Self::Rgb),
            "linearRGB" => Some(Self::LinearRgb),
            _ => None,
        }
    }

    fn xfl_name(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::LinearRgb => "linearRGB",
        }
    }
}

/// A gradient fill (linear or radial; the variant of [`FillStyle`] says which).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientFill {
    /// Maps gradient space (-819.2 ..= 819.2) into shape pixel space.
    pub matrix: Affine,
    /// Sorted by offset.
    pub stops: Vec<GradientStop>,
    pub spread: Spread,
    pub interpolation: Interpolation,
    /// Radial only: focal point along the x axis, -1.0 ..= 1.0 (`focalPointRatio`).
    pub focal: f64,
}

impl Default for GradientFill {
    fn default() -> Self {
        Self { matrix: Affine::IDENTITY, stops: Vec::new(), spread: Spread::Pad, interpolation: Interpolation::Rgb, focal: 0.0 }
    }
}

/// A bitmap fill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BitmapFill {
    /// Library path of the bitmap item (`bitmapPath`).
    pub path: String,
    /// The matrix as stored in XFL; see the module documentation for its units.
    pub matrix: Affine,
    /// `bitmapIsClipped`: the bitmap is drawn once instead of tiled.
    pub clipped: bool,
    /// Smooth (bilinear) sampling instead of nearest neighbour.
    pub smoothing: bool,
}

/// What a region of a shape is filled with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FillStyle {
    Solid(Rgba8),
    LinearGradient(GradientFill),
    RadialGradient(GradientFill),
    Bitmap(BitmapFill),
}

impl Default for FillStyle {
    /// Opaque black, what an empty `<SolidColor/>` means.
    fn default() -> Self {
        Self::Solid(Rgba8::BLACK)
    }
}

impl FillStyle {
    /// The colour of a solid fill.
    #[must_use]
    pub const fn solid_color(&self) -> Option<Rgba8> {
        match self {
            Self::Solid(c) => Some(*c),
            _ => None,
        }
    }
}

/// A solid (continuous) stroke.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolidStroke {
    /// Width in pixels; 0 is a hairline.
    pub weight: f64,
    /// What the stroke is painted with (usually a solid colour).
    pub fill: FillStyle,
    pub caps: Cap,
    pub joints: Join,
    pub miter_limit: f64,
    pub scale_mode: StrokeScale,
    pub pixel_hinting: bool,
}

impl Default for SolidStroke {
    fn default() -> Self {
        Self {
            weight: 1.0,
            fill: FillStyle::default(),
            caps: Cap::Round,
            joints: Join::Round,
            miter_limit: 3.0,
            scale_mode: StrokeScale::Normal,
            pixel_hinting: false,
        }
    }
}

/// The patterned stroke kinds Animate has besides the solid one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StrokeKind {
    Dashed,
    Dotted,
    Ragged,
    Stipple,
    Hatched,
}

impl StrokeKind {
    /// Parses the XFL element name (`DashedStroke`, ...).
    #[must_use]
    pub fn from_element(name: &str) -> Option<Self> {
        match name {
            "DashedStroke" => Some(Self::Dashed),
            "DottedStroke" => Some(Self::Dotted),
            "RaggedStroke" => Some(Self::Ragged),
            "StippleStroke" => Some(Self::Stipple),
            "HatchedStroke" => Some(Self::Hatched),
            _ => None,
        }
    }

    /// The XFL element name.
    #[must_use]
    pub const fn element_name(self) -> &'static str {
        match self {
            Self::Dashed => "DashedStroke",
            Self::Dotted => "DottedStroke",
            Self::Ragged => "RaggedStroke",
            Self::Stipple => "StippleStroke",
            Self::Hatched => "HatchedStroke",
        }
    }
}

/// A patterned stroke. It renders as its solid base; the pattern attributes are kept verbatim so files round-trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OtherStroke {
    pub kind: StrokeKind,
    /// The attributes every stroke has (weight, scale mode, fill...).
    pub base: SolidStroke,
    /// Kind-specific attributes (`dash1`, `dotSpace`, `waveHeight`, ...) as name/value pairs, in file order.
    pub attrs: Vec<(String, String)>,
}

/// A stroke style of an edge shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StrokeStyle {
    Solid(SolidStroke),
    /// Dashed, dotted, ragged, stipple and hatched strokes; rendered as solid.
    Other(OtherStroke),
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self::Solid(SolidStroke::default())
    }
}

impl StrokeStyle {
    /// The solid stroke that is drawn for this style.
    #[must_use]
    pub const fn solid(&self) -> &SolidStroke {
        match self {
            Self::Solid(s) => s,
            Self::Other(o) => &o.base,
        }
    }

    /// Mutable access to the solid stroke that is drawn for this style.
    pub const fn solid_mut(&mut self) -> &mut SolidStroke {
        match self {
            Self::Solid(s) => s,
            Self::Other(o) => &mut o.base,
        }
    }

    /// Width in pixels.
    #[must_use]
    pub const fn weight(&self) -> f64 {
        self.solid().weight
    }

    /// A solid stroke of `weight` pixels in `color` with the XFL default caps, joints and scale mode.
    #[must_use]
    pub fn plain(weight: f64, color: Rgba8) -> Self {
        Self::Solid(SolidStroke { weight, fill: FillStyle::Solid(color), ..SolidStroke::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xfl_names_round_trip() {
        for name in ["none", "round", "square"] {
            assert_eq!(Cap::from_xfl(name).unwrap().xfl_name(), name);
        }
        for name in ["miter", "round", "bevel"] {
            assert_eq!(Join::from_xfl(name).unwrap().xfl_name(), name);
        }
        for name in ["normal", "horizontal", "vertical", "none"] {
            assert_eq!(StrokeScale::from_xfl(name).unwrap().xfl_name(), name);
        }
        for name in ["extend", "reflect", "repeat"] {
            assert_eq!(Spread::from_xfl(name).unwrap().xfl_name(), name);
        }
        for name in ["RGB", "linearRGB"] {
            assert_eq!(Interpolation::from_xfl(name).unwrap().xfl_name(), name);
        }
        assert_eq!(Cap::from_xfl("bogus"), None);
    }

    #[test]
    fn stroke_kinds_round_trip() {
        for kind in [StrokeKind::Dashed, StrokeKind::Dotted, StrokeKind::Ragged, StrokeKind::Stipple, StrokeKind::Hatched] {
            assert_eq!(StrokeKind::from_element(kind.element_name()), Some(kind));
        }
        assert_eq!(StrokeKind::from_element("SolidStroke"), None);
    }

    #[test]
    fn other_strokes_expose_their_solid_base() {
        let mut other = StrokeStyle::Other(OtherStroke {
            kind: StrokeKind::Dashed,
            base: SolidStroke { weight: 2.5, ..SolidStroke::default() },
            attrs: vec![("dash1".into(), "4".into())],
        });
        assert!((other.weight() - 2.5).abs() < 1e-12);
        other.solid_mut().weight = 3.0;
        assert!((other.solid().weight - 3.0).abs() < 1e-12);
    }

    #[test]
    fn styles_serialize() {
        let fill = FillStyle::LinearGradient(GradientFill {
            stops: vec![GradientStop { offset: 0.0, color: Rgba8::WHITE }, GradientStop { offset: 1.0, color: Rgba8::BLACK }],
            ..GradientFill::default()
        });
        let json = serde_json::to_string(&fill).unwrap();
        assert_eq!(serde_json::from_str::<FillStyle>(&json).unwrap(), fill);
        let stroke = StrokeStyle::plain(2.0, Rgba8::opaque(1, 2, 3));
        let json = serde_json::to_string(&stroke).unwrap();
        assert_eq!(serde_json::from_str::<StrokeStyle>(&json).unwrap(), stroke);
    }
}
