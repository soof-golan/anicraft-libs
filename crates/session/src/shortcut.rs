//! Keyboard shortcuts as plain data: parsed from text such as `"Cmd+Shift+Z"`, compared and displayed.
//!
//! `Cmd` means the platform command key: Control on Windows and Linux, Command on macOS. The UI layer
//! translates its own key events into a [`Shortcut`] and looks it up in the registry.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A key chord: modifiers plus one key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Shortcut {
    /// The platform command modifier (Control or Command).
    pub command: bool,
    /// Shift.
    pub shift: bool,
    /// Alt or Option.
    pub alt: bool,
    /// Normalised key name: one uppercase character (`"Z"`, `"1"`, `"["`) or a capitalised name
    /// (`"Delete"`, `"Space"`, `"F5"`).
    pub key: String,
}

/// Why a shortcut string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShortcutError {
    /// The text had no key, for example `"Cmd+"` or an empty string.
    #[error("shortcut {0:?} has no key")]
    NoKey(String),
    /// A word before the key is not a modifier name.
    #[error("{0:?} is not a modifier")]
    UnknownModifier(String),
    /// A modifier appeared twice.
    #[error("shortcut {0:?} repeats a modifier")]
    RepeatedModifier(String),
}

impl Shortcut {
    /// A chord with no modifiers.
    #[must_use]
    pub fn key(key: &str) -> Self {
        Self { command: false, shift: false, alt: false, key: normalise_key(key) }
    }

    /// Adds the command modifier.
    #[must_use]
    pub const fn with_command(mut self) -> Self {
        self.command = true;
        self
    }

    /// Adds the shift modifier.
    #[must_use]
    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Adds the alt modifier.
    #[must_use]
    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }
}

fn normalise_key(raw: &str) -> String {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let rest = chars.as_str();
    if rest.is_empty() {
        return first.to_uppercase().collect();
    }
    let mut out: String = first.to_uppercase().collect();
    out.push_str(&rest.to_lowercase());
    out
}

impl FromStr for Shortcut {
    type Err = ShortcutError;

    /// Parses `"Cmd+Shift+Z"`. Modifier names are case-insensitive: `cmd`, `ctrl`, `control`, `command`,
    /// `mod` (all the command key), `shift`, `alt`, `option`, `opt`. A literal plus key is spelled `Plus`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut out = Self { command: false, shift: false, alt: false, key: String::new() };
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = parts.pop().unwrap_or_default();
        if key.is_empty() {
            return Err(ShortcutError::NoKey(text.to_owned()));
        }
        for part in parts {
            let flag = match part.to_ascii_lowercase().as_str() {
                "cmd" | "ctrl" | "control" | "command" | "mod" => &mut out.command,
                "shift" => &mut out.shift,
                "alt" | "option" | "opt" => &mut out.alt,
                _ => return Err(ShortcutError::UnknownModifier(part.to_owned())),
            };
            if *flag {
                return Err(ShortcutError::RepeatedModifier(text.to_owned()));
            }
            *flag = true;
        }
        out.key = if key.eq_ignore_ascii_case("plus") { "+".to_owned() } else { normalise_key(key) };
        Ok(out)
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.command {
            f.write_str("Cmd+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.key == "+" { f.write_str("Plus") } else { f.write_str(&self.key) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays() {
        let s: Shortcut = "cmd+shift+z".parse().unwrap();
        assert_eq!(s, Shortcut::key("z").with_command().with_shift());
        assert_eq!(s.to_string(), "Cmd+Shift+Z");
        assert_eq!("Ctrl+Plus".parse::<Shortcut>().unwrap().key, "+");
        assert_eq!("Delete".parse::<Shortcut>().unwrap().key, "Delete");
        assert_eq!("f5".parse::<Shortcut>().unwrap().to_string(), "F5");
    }

    #[test]
    fn rejects_bad_text() {
        assert!("".parse::<Shortcut>().is_err());
        assert!("Cmd+".parse::<Shortcut>().is_err());
        assert!("Cmd+Ctrl+Z".parse::<Shortcut>().is_err());
        assert!("Foo+Z".parse::<Shortcut>().is_err());
    }

    #[test]
    fn display_round_trips() {
        for text in ["Cmd+Alt+Shift+K", "Space", "Cmd+Plus", "Alt+F12"] {
            let s: Shortcut = text.parse().unwrap();
            assert_eq!(s.to_string().parse::<Shortcut>().unwrap(), s);
        }
    }
}
