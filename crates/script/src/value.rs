//! Values of the interpreter and the conversions `ActionScript` does between them.

use std::fmt::Write as _;

use crate::ast::Func;

/// A value.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Undefined,
    Num(f64),
    Str(String),
    Bool(bool),
    /// A movie clip or button, by the chain of instance names from the main timeline (empty is the main timeline).
    Clip(Vec<String>),
    /// A function; it is called with the global scope.
    Func(Box<Func>),
    /// An event object handed to a listener.
    Event {
        kind: String,
        target: Vec<String>,
    },
    /// A class whose constants the interpreter knows (`MouseEvent`, `Event`).
    Class(String),
}

impl Value {
    /// `ActionScript` truthiness.
    #[must_use]
    pub fn truthy(&self) -> bool {
        match self {
            Self::Undefined => false,
            Self::Num(n) => *n != 0.0 && !n.is_nan(),
            Self::Str(s) => !s.is_empty(),
            Self::Bool(b) => *b,
            Self::Clip(_) | Self::Func(_) | Self::Event { .. } | Self::Class(_) => true,
        }
    }

    /// The number a value converts to (NaN when it has none).
    #[must_use]
    pub fn to_num(&self) -> f64 {
        match self {
            Self::Num(n) => *n,
            Self::Bool(b) => f64::from(u8::from(*b)),
            Self::Str(s) => {
                let t = s.trim();
                if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) }
            }
            _ => f64::NAN,
        }
    }

    /// The text `trace` prints for a value.
    #[must_use]
    pub fn to_text(&self) -> String {
        match self {
            Self::Undefined => "undefined".to_owned(),
            Self::Num(n) => number_text(*n),
            Self::Str(s) => s.clone(),
            Self::Bool(b) => b.to_string(),
            Self::Clip(path) if path.is_empty() => "[object MainTimeline]".to_owned(),
            Self::Clip(path) => format!("[object MovieClip {}]", path.join(".")),
            Self::Func(_) => "function Function() {}".to_owned(),
            Self::Event { kind, .. } => format!("[Event type=\"{kind}\"]"),
            Self::Class(name) => format!("[class {name}]"),
        }
    }
}

/// Numbers print without a fraction when they have none.
#[must_use]
pub fn number_text(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".to_owned() } else { "-Infinity".to_owned() };
    }
    let mut out = String::new();
    if n.fract() == 0.0 && n.abs() < 1e15 {
        let _ = write!(out, "{n:.0}");
    } else {
        let _ = write!(out, "{n}");
    }
    out
}

/// `==`: values of one type compare directly; otherwise both sides are converted to numbers.
#[must_use]
pub fn loose_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) | (Value::Class(x), Value::Class(y)) => x == y,
        (Value::Undefined, Value::Undefined) => true,
        (Value::Clip(x), Value::Clip(y)) => x == y,
        (Value::Num(_) | Value::Bool(_) | Value::Str(_), Value::Num(_) | Value::Bool(_) | Value::Str(_)) => {
            let (x, y) = (a.to_num(), b.to_num());
            (x - y).abs() < f64::EPSILON || (x.is_infinite() && y.is_infinite() && x.is_sign_positive() == y.is_sign_positive())
        }
        _ => false,
    }
}

/// `===`: the same type and the same value.
#[must_use]
pub fn strict_equals(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b) && loose_equals(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_like_flash() {
        assert_eq!(number_text(3.0), "3");
        assert_eq!(number_text(2.5), "2.5");
        assert_eq!(number_text(f64::NAN), "NaN");
    }

    #[test]
    fn equality() {
        assert!(loose_equals(&Value::Num(1.0), &Value::Str("1".into())));
        assert!(!strict_equals(&Value::Num(1.0), &Value::Str("1".into())));
        assert!(!loose_equals(&Value::Undefined, &Value::Num(0.0)));
    }
}
