//! Unknown XML preserved verbatim so `anicraft-xfl` can write back what it did not understand.

use serde::{Deserialize, Serialize};

/// Attributes and child elements a reader did not model, kept in file order.
///
/// Every model struct that mirrors an XFL element carries one. Attribute values are the raw (unescaped) strings;
/// child elements are raw XML text, one string per element, which the writer emits as is.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Extra {
    /// Unknown attributes `(name, value)` in file order.
    pub attrs: Vec<(String, String)>,
    /// Unknown child elements as raw XML, in file order.
    pub children: Vec<String>,
}

impl Extra {
    /// An `Extra` with nothing in it.
    #[must_use]
    pub const fn new() -> Self {
        Self { attrs: Vec::new(), children: Vec::new() }
    }

    /// True when there is nothing to write back.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.attrs.is_empty() && self.children.is_empty()
    }

    /// Value of the first attribute called `name`.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Sets (or appends) an attribute, keeping its original position when it exists.
    pub fn set_attr(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(slot) = self.attrs.iter_mut().find(|(k, _)| k == name) {
            slot.1 = value;
        } else {
            self.attrs.push((name.to_owned(), value));
        }
    }

    /// Removes every attribute called `name`, returning the first value.
    pub fn remove_attr(&mut self, name: &str) -> Option<String> {
        let found = self.attr(name).map(str::to_owned);
        self.attrs.retain(|(k, _)| k != name);
        found
    }
}

/// An element the model has no type for, kept as raw XML (an unknown element kind inside a frame, a filter we do not
/// map, ...).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RawXml {
    /// Element name, for example `BevelFilter`.
    pub tag: String,
    /// The complete element text including its children.
    pub xml: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attrs_keep_order_and_update_in_place() {
        let mut e = Extra::new();
        assert!(e.is_empty());
        e.set_attr("a", "1");
        e.set_attr("b", "2");
        e.set_attr("a", "3");
        assert_eq!(e.attrs, vec![("a".into(), "3".into()), ("b".into(), "2".into())]);
        assert_eq!(e.attr("b"), Some("2"));
        assert_eq!(e.remove_attr("a"), Some("3".into()));
        assert_eq!(e.attr("a"), None);
        assert!(!e.is_empty());
    }
}
