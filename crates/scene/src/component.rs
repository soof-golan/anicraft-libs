//! Component instances (`DOMComponentInstance`): a symbol instance that carries parameters.
//!
//! Animate stores the parameters as an XML document in the instance's `parametersAsXML` child. The model keeps that
//! text verbatim and reads and edits the parameters inside it in place, so a file whose parameter format differs from
//! what the reader expects still survives a round trip unchanged. A parameter is any element of that text that has a
//! `value` attribute and an `id` (or `name`) attribute; its `type` attribute, when present, says how it is edited.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// The parameters of a component instance.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ComponentInstance {
    /// The `parametersAsXML` text, verbatim.
    pub parameters_xml: String,
}

/// One parameter of a component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentParam {
    pub id: String,
    pub value: String,
    /// The `type` attribute (`String`, `Boolean`, `Number`, `List`, ...), when the file states one.
    pub kind: Option<String>,
}

/// An attribute found in a tag: its name, and the byte range of its (still escaped) value.
struct Attr {
    name: String,
    value: std::ops::Range<usize>,
}

impl ComponentInstance {
    /// A component with the given parameters text.
    #[must_use]
    pub fn new(parameters_xml: impl Into<String>) -> Self {
        Self { parameters_xml: parameters_xml.into() }
    }

    /// A component whose parameters are `(id, value, type)` triples, written the way Animate lists them.
    #[must_use]
    pub fn from_params(params: &[(&str, &str, &str)]) -> Self {
        let mut xml = String::from("<Properties>");
        for (id, value, kind) in params {
            let _ = write!(xml, "<Property id=\"{}\" value=\"{}\" type=\"{}\"/>", escape(id), escape(value), escape(kind));
        }
        xml.push_str("</Properties>");
        Self { parameters_xml: xml }
    }

    /// The parameters, in file order.
    #[must_use]
    pub fn params(&self) -> Vec<ComponentParam> {
        let mut out = Vec::new();
        for tag in tags(&self.parameters_xml) {
            let get = |n: &str| tag.iter().find(|a| a.name == n).and_then(|a| self.parameters_xml.get(a.value.clone())).map(unescape);
            if let (Some(value), Some(id)) = (get("value"), get("id").or_else(|| get("name"))) {
                out.push(ComponentParam { id, value, kind: get("type") });
            }
        }
        out
    }

    /// The value of parameter `id`.
    #[must_use]
    pub fn param(&self, id: &str) -> Option<String> {
        self.params().into_iter().find(|p| p.id == id).map(|p| p.value)
    }

    /// Sets the value of parameter `id`; false when there is no such parameter.
    pub fn set_param(&mut self, id: &str, value: &str) -> bool {
        let range = tags(&self.parameters_xml).into_iter().find_map(|tag| {
            let get = |n: &str| tag.iter().find(|a| a.name == n).map(|a| a.value.clone());
            let name = get("id").or_else(|| get("name"))?;
            let this = self.parameters_xml.get(name).map(unescape)?;
            (this == id).then(|| get("value")).flatten()
        });
        let Some(range) = range else { return false };
        self.parameters_xml.replace_range(range, &escape(value));
        true
    }
}

/// Escapes text for an attribute value in double quotes.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The attributes of every start tag and empty-element tag of `xml`.
fn tags(xml: &str) -> Vec<Vec<Attr>> {
    let bytes = xml.as_bytes();
    let mut out = Vec::new();
    let mut i = 0_usize;
    while let Some(rel) = bytes.get(i..).and_then(|rest| rest.iter().position(|b| *b == b'<')) {
        let start = i.saturating_add(rel);
        let after = start.saturating_add(1);
        match bytes.get(after) {
            Some(b'/' | b'?' | b'!') | None => {
                i = after;
                continue;
            }
            _ => {}
        }
        let mut j = after;
        // The element name.
        while bytes.get(j).is_some_and(|b| !b.is_ascii_whitespace() && *b != b'>' && *b != b'/') {
            j = j.saturating_add(1);
        }
        let mut attrs = Vec::new();
        loop {
            while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                j = j.saturating_add(1);
            }
            match bytes.get(j) {
                None => break,
                Some(b'>') => {
                    j = j.saturating_add(1);
                    break;
                }
                Some(b'/') => j = j.saturating_add(1),
                Some(_) => {
                    let name_start = j;
                    while bytes.get(j).is_some_and(|b| !b.is_ascii_whitespace() && !matches!(*b, b'=' | b'>' | b'/')) {
                        j = j.saturating_add(1);
                    }
                    let name = xml.get(name_start..j).unwrap_or("").to_owned();
                    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                        j = j.saturating_add(1);
                    }
                    if bytes.get(j) != Some(&b'=') {
                        if j == name_start {
                            j = j.saturating_add(1);
                        }
                        continue;
                    }
                    j = j.saturating_add(1);
                    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                        j = j.saturating_add(1);
                    }
                    let Some(&quote) = bytes.get(j).filter(|q| matches!(**q, b'"' | b'\'')) else { continue };
                    j = j.saturating_add(1);
                    let value_start = j;
                    while bytes.get(j).is_some_and(|b| *b != quote) {
                        j = j.saturating_add(1);
                    }
                    attrs.push(Attr { name, value: value_start..j.min(bytes.len()) });
                    j = j.saturating_add(1);
                }
            }
        }
        if !attrs.is_empty() {
            out.push(attrs);
        }
        i = j.max(after);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<Properties><Property id="label" value="Click &amp; go" type="String"/><Property id="enabled" value="true" type="Boolean"/><Property name="emphasized" value='false'/></Properties>"#;

    #[test]
    fn reads_parameters_in_order() {
        let c = ComponentInstance::new(XML);
        let p = c.params();
        assert_eq!(p.len(), 3);
        assert_eq!(p.first().map(|p| (p.id.as_str(), p.value.as_str(), p.kind.as_deref())), Some(("label", "Click & go", Some("String"))));
        assert_eq!(c.param("emphasized").as_deref(), Some("false"));
    }

    #[test]
    fn edits_in_place_and_keeps_the_rest_verbatim() {
        let mut c = ComponentInstance::new(XML);
        assert!(c.set_param("label", "Say \"hi\" <now>"));
        assert_eq!(c.param("label").as_deref(), Some("Say \"hi\" <now>"));
        assert_eq!(c.param("enabled").as_deref(), Some("true"));
        assert!(c.parameters_xml.ends_with(r#"<Property name="emphasized" value='false'/></Properties>"#));
        assert!(!c.set_param("missing", "x"));
    }

    #[test]
    fn from_params_round_trips() {
        let c = ComponentInstance::from_params(&[("label", "A&B", "String"), ("n", "3", "Number")]);
        assert_eq!(c.param("label").as_deref(), Some("A&B"));
        assert_eq!(c.param("n").as_deref(), Some("3"));
    }

    #[test]
    fn hostile_text_does_not_panic() {
        for s in ["<", "<a", "<a b", "<a b=", "<a b=\"", "<a b='x", "</", "<!--", "<?x", "<a =\"1\"/>", "<a ===/>"] {
            let _ = ComponentInstance::new(s).params();
        }
    }
}
