//! Viseme sets and the mapping between them.

use serde::{Deserialize, Serialize};

/// The Character Animator viseme set (the mouth shapes a puppet's `Mouth` group is tagged with).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Viseme {
    /// Closed, resting mouth (silence).
    #[default]
    Neutral,
    /// Wide open vowel, as in "father".
    Ah,
    /// Tongue-tip stop: D, T, N, and short bursts.
    D,
    /// Spread lips vowel, as in "see", "bed".
    Ee,
    /// Lower lip against upper teeth: F, V.
    F,
    /// Tongue raised: L.
    L,
    /// Lips closed: M, B, P.
    M,
    /// Rounded open vowel, as in "go".
    Oh,
    /// R sound.
    R,
    /// Sibilant: S, Z, SH, CH.
    S,
    /// Relaxed mid vowel, as in "cut", "about".
    Uh,
    /// Pursed lips: W, OO.
    WOo,
}

impl Viseme {
    /// All twelve visemes, in Character Animator's order.
    pub const ALL: [Self; 12] = [Self::Neutral, Self::Ah, Self::D, Self::Ee, Self::F, Self::L, Self::M, Self::Oh, Self::R, Self::S, Self::Uh, Self::WOo];

    /// The name of the layer a Character Animator puppet uses for this viseme (`W-Oo` has a hyphen there).
    #[must_use]
    pub const fn ca_layer_name(self) -> &'static str {
        match self {
            Self::Neutral => "Neutral",
            Self::Ah => "Ah",
            Self::D => "D",
            Self::Ee => "Ee",
            Self::F => "F",
            Self::L => "L",
            Self::M => "M",
            Self::Oh => "Oh",
            Self::R => "R",
            Self::S => "S",
            Self::Uh => "Uh",
            Self::WOo => "W-Oo",
        }
    }

    /// Parses a layer or tag name, ignoring case, spaces, hyphens and underscores (`"w-oo"`, `"WOo"`, `"W Oo"`).
    #[must_use]
    pub fn from_ca_layer_name(name: &str) -> Option<Self> {
        let key: String = name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
        Self::ALL.into_iter().find(|v| {
            let own: String = v.ca_layer_name().chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
            own == key
        })
    }

    /// The closest mouth shape in Animate's lip-sync set (lossy: several visemes share a shape).
    #[must_use]
    pub const fn to_animate(self) -> AnimateViseme {
        match self {
            Self::Neutral => AnimateViseme::Rest,
            Self::Ah => AnimateViseme::Ai,
            Self::Ee => AnimateViseme::E,
            Self::Oh => AnimateViseme::O,
            Self::Uh => AnimateViseme::U,
            Self::WOo => AnimateViseme::Wq,
            Self::F => AnimateViseme::Fv,
            Self::L => AnimateViseme::L,
            Self::M => AnimateViseme::Mbp,
            Self::D | Self::R | Self::S => AnimateViseme::Etc,
        }
    }

    /// Whether the mouth is open for a vowel (as opposed to a consonant or rest).
    #[must_use]
    pub const fn is_vowel(self) -> bool {
        matches!(self, Self::Ah | Self::Ee | Self::Oh | Self::Uh | Self::WOo)
    }
}

/// Animate's lip-sync mouth shapes (the ten-shape convention of Preston Blair / Papagayo that Animate's lip-sync
/// panel follows).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AnimateViseme {
    /// Mouth at rest.
    #[default]
    Rest,
    /// "A", "I": open mouth.
    Ai,
    /// "E": spread lips.
    E,
    /// "O": rounded open.
    O,
    /// "U": relaxed or slightly rounded.
    U,
    /// F, V: lip against teeth.
    Fv,
    /// L, TH: tongue visible.
    L,
    /// M, B, P: closed lips.
    Mbp,
    /// W, Q: pursed lips.
    Wq,
    /// Everything else (D, T, R, S, K, ...).
    Etc,
}

impl AnimateViseme {
    /// All ten shapes.
    pub const ALL: [Self; 10] = [Self::Rest, Self::Ai, Self::E, Self::O, Self::U, Self::Fv, Self::L, Self::Mbp, Self::Wq, Self::Etc];

    /// The conventional label (`"AI"`, `"MBP"`, `"rest"`, ...).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rest => "rest",
            Self::Ai => "AI",
            Self::E => "E",
            Self::O => "O",
            Self::U => "U",
            Self::Fv => "FV",
            Self::L => "L",
            Self::Mbp => "MBP",
            Self::Wq => "WQ",
            Self::Etc => "etc",
        }
    }

    /// The Character Animator viseme to use for this shape (the inverse of [`Viseme::to_animate`] on its image).
    #[must_use]
    pub const fn to_character_animator(self) -> Viseme {
        match self {
            Self::Rest => Viseme::Neutral,
            Self::Ai => Viseme::Ah,
            Self::E => Viseme::Ee,
            Self::O => Viseme::Oh,
            Self::U => Viseme::Uh,
            Self::Fv => Viseme::F,
            Self::L => Viseme::L,
            Self::Mbp => Viseme::M,
            Self::Wq => Viseme::WOo,
            Self::Etc => Viseme::D,
        }
    }
}

/// Optional facial expressions a puppet may carry alongside visemes.
///
/// Character Animator tags `Smile` and `Surprised` on mouth layers. The analyser never produces them; they exist so
/// one track type can carry both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Expression {
    /// A smiling mouth.
    Smile,
    /// A surprised (open, round) mouth.
    Surprised,
}

impl Expression {
    /// The tag name used on puppet layers.
    #[must_use]
    pub const fn ca_layer_name(self) -> &'static str {
        match self {
            Self::Smile => "Smile",
            Self::Surprised => "Surprised",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for v in Viseme::ALL {
            assert_eq!(Viseme::from_ca_layer_name(v.ca_layer_name()), Some(v));
        }
        assert_eq!(Viseme::from_ca_layer_name("w oo"), Some(Viseme::WOo));
        assert_eq!(Viseme::from_ca_layer_name("WOO"), Some(Viseme::WOo));
        assert_eq!(Viseme::from_ca_layer_name("nope"), None);
    }

    #[test]
    fn animate_mapping_is_total_and_consistent() {
        for v in Viseme::ALL {
            let a = v.to_animate();
            // Round trip lands on a viseme that maps to the same Animate shape.
            assert_eq!(a.to_character_animator().to_animate(), a);
        }
        for a in AnimateViseme::ALL {
            assert_eq!(a.to_character_animator().to_animate(), a);
        }
        assert_eq!(Viseme::Neutral.to_animate(), AnimateViseme::Rest);
        assert_eq!(Viseme::M.to_animate().label(), "MBP");
    }
}
