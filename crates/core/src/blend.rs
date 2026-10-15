//! Blend modes: the Flash/Animate set plus the Photoshop modes that layered puppet artwork uses.

use serde::{Deserialize, Serialize};

/// How a layer or symbol instance combines with what is below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    /// Flash "layer": composite the children in isolation first, then blend normally.
    Layer,
    /// Photoshop group "pass through": children blend straight into the backdrop.
    PassThrough,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    /// Flash "add": channel sum.
    Add,
    /// Flash "subtract": backdrop minus source.
    Subtract,
    /// Flash "invert": inverts the backdrop where the source is opaque.
    Invert,
    /// Flash "alpha": applies the source alpha to the parent layer group.
    Alpha,
    /// Flash "erase": removes the parent layer group where the source is opaque.
    Erase,
}

impl BlendMode {
    /// The XFL `blendMode` attribute value, or `None` for `Normal` (which XFL omits).
    #[must_use]
    pub const fn xfl_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Normal | Self::PassThrough => return None,
            Self::Layer => "layer",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Overlay => "overlay",
            Self::Darken => "darken",
            Self::Lighten => "lighten",
            Self::HardLight => "hardlight",
            Self::Difference => "difference",
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::Invert => "invert",
            Self::Alpha => "alpha",
            Self::Erase => "erase",
            Self::ColorDodge => "colordodge",
            Self::ColorBurn => "colorburn",
            Self::SoftLight => "softlight",
            Self::Exclusion => "exclusion",
            Self::Hue => "hue",
            Self::Saturation => "saturation",
            Self::Color => "color",
            Self::Luminosity => "luminosity",
        })
    }

    /// Parses an XFL `blendMode` attribute value (unknown values fall back to `Normal`, never an error).
    #[must_use]
    pub fn from_xfl_name(name: &str) -> Self {
        match name {
            "layer" => Self::Layer,
            "multiply" => Self::Multiply,
            "screen" => Self::Screen,
            "overlay" => Self::Overlay,
            "darken" => Self::Darken,
            "lighten" => Self::Lighten,
            "hardlight" => Self::HardLight,
            "difference" => Self::Difference,
            "add" => Self::Add,
            "subtract" => Self::Subtract,
            "invert" => Self::Invert,
            "alpha" => Self::Alpha,
            "erase" => Self::Erase,
            "colordodge" => Self::ColorDodge,
            "colorburn" => Self::ColorBurn,
            "softlight" => Self::SoftLight,
            "exclusion" => Self::Exclusion,
            "hue" => Self::Hue,
            "saturation" => Self::Saturation,
            "color" => Self::Color,
            "luminosity" => Self::Luminosity,
            _ => Self::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xfl_names_round_trip() {
        for mode in [BlendMode::Layer, BlendMode::Multiply, BlendMode::HardLight, BlendMode::Erase, BlendMode::Luminosity] {
            let name = mode.xfl_name().unwrap();
            assert_eq!(BlendMode::from_xfl_name(name), mode);
        }
        assert_eq!(BlendMode::Normal.xfl_name(), None);
        assert_eq!(BlendMode::from_xfl_name("bogus"), BlendMode::Normal);
    }
}
