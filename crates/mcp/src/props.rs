//! Property paths into JSON-shaped documents.
//!
//! Applications whose document (or a view of it) serialises to JSON can implement `get_property` / `set_property` with
//! these helpers: [`parse_path`] understands JSON-pointer style (`/layers/0/name`) and dotted style
//! (`layers[0].name`, `layers.0.name`), and [`get`] / [`set`] walk a [`serde_json::Value`] without panicking on hostile
//! paths or indexes.

use serde_json::Value;

use crate::protocol::{ControlError, ErrorKind};

/// Longest accepted path text, in bytes.
pub const MAX_PATH_BYTES: usize = 1024;
/// Most segments in a path.
pub const MAX_SEGMENTS: usize = 64;

/// Splits a property path into segments.
///
/// # Errors
/// [`ErrorKind::InvalidParams`] for an empty, over-long or malformed path.
pub fn parse_path(path: &str) -> Result<Vec<String>, ControlError> {
    let bad = |why: &str| ControlError::invalid(format!("property path {path:?}: {why}"));
    if path.len() > MAX_PATH_BYTES {
        return Err(bad("too long"));
    }
    let segments: Vec<String> = if let Some(rest) = path.strip_prefix('/') {
        if rest.is_empty() { Vec::new() } else { rest.split('/').map(|s| s.replace("~1", "/").replace("~0", "~")).collect() }
    } else {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut chars = path.chars();
        while let Some(ch) = chars.next() {
            match ch {
                '.' => {
                    if !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                    }
                }
                '[' => {
                    if !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                    }
                    let mut inner = String::new();
                    let mut closed = false;
                    for c in chars.by_ref() {
                        if c == ']' {
                            closed = true;
                            break;
                        }
                        inner.push(c);
                    }
                    if !closed {
                        return Err(bad("unclosed `[`"));
                    }
                    out.push(inner.trim_matches(|c| c == '"' || c == '\'').to_owned());
                }
                other => current.push(other),
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
        out
    };
    if segments.len() > MAX_SEGMENTS {
        return Err(bad("too many segments"));
    }
    Ok(segments)
}

/// The value at `path` (the empty path is the whole document).
///
/// # Errors
/// [`ErrorKind::NotFound`] when a segment does not exist.
pub fn get<'a>(root: &'a Value, path: &[String]) -> Result<&'a Value, ControlError> {
    let mut current = root;
    for (depth, segment) in path.iter().enumerate() {
        current = match current {
            Value::Object(map) => map.get(segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        }
        .ok_or_else(|| not_found(path, depth))?;
    }
    Ok(current)
}

/// Replaces the value at `path`. Object members are created when missing; an array index may be one past the end to
/// append.
///
/// # Errors
/// [`ErrorKind::NotFound`] when a parent does not exist, [`ErrorKind::InvalidParams`] for the empty path.
pub fn set(root: &mut Value, path: &[String], value: Value) -> Result<(), ControlError> {
    let Some((last, parents)) = path.split_last() else {
        return Err(ControlError::invalid("cannot replace the whole document through a property path"));
    };
    let mut current = root;
    for (depth, segment) in parents.iter().enumerate() {
        current = match current {
            Value::Object(map) => map.get_mut(segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get_mut(i)),
            _ => None,
        }
        .ok_or_else(|| not_found(path, depth))?;
    }
    match current {
        Value::Object(map) => {
            map.insert(last.clone(), value);
            Ok(())
        }
        Value::Array(items) => {
            let index = last.parse::<usize>().map_err(|_| ControlError::invalid(format!("{last:?} is not an array index")))?;
            if let Some(slot) = items.get_mut(index) {
                *slot = value;
                Ok(())
            } else if index == items.len() {
                items.push(value);
                Ok(())
            } else {
                Err(ControlError::new(ErrorKind::NotFound, format!("index {index} is past the end ({} items)", items.len())))
            }
        }
        _ => Err(not_found(path, parents.len())),
    }
}

fn not_found(path: &[String], depth: usize) -> ControlError {
    let shown: Vec<&str> = path.iter().take(depth.saturating_add(1)).map(String::as_str).collect();
    ControlError::new(ErrorKind::NotFound, format!("no property at /{}", shown.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn both_syntaxes_parse_alike() {
        let expected = vec!["layers".to_owned(), "0".to_owned(), "name".to_owned()];
        assert_eq!(parse_path("/layers/0/name").unwrap(), expected);
        assert_eq!(parse_path("layers[0].name").unwrap(), expected);
        assert_eq!(parse_path("layers.0.name").unwrap(), expected);
        assert_eq!(parse_path("/a~1b/c~0d").unwrap(), vec!["a/b".to_owned(), "c~d".to_owned()]);
        assert_eq!(parse_path("m[\"k.x\"]").unwrap(), vec!["m".to_owned(), "k.x".to_owned()]);
        assert!(parse_path("/").unwrap().is_empty());
    }

    #[test]
    fn malformed_and_oversize_paths_are_rejected() {
        assert!(parse_path("a[0").is_err());
        assert!(parse_path(&"a.".repeat(MAX_SEGMENTS + 1)).is_err());
        assert!(parse_path(&"x".repeat(MAX_PATH_BYTES + 1)).is_err());
    }

    #[test]
    fn get_and_set_walk_objects_and_arrays() {
        let mut doc = json!({"layers": [{"name": "a"}, {"name": "b"}], "fps": 24});
        let name = parse_path("layers[1].name").unwrap();
        assert_eq!(get(&doc, &name).unwrap(), &json!("b"));
        set(&mut doc, &name, json!("z")).unwrap();
        assert_eq!(doc["layers"][1]["name"], "z");
        set(&mut doc, &parse_path("layers[2]").unwrap(), json!({"name": "c"})).unwrap();
        assert_eq!(doc["layers"].as_array().unwrap().len(), 3);
        set(&mut doc, &parse_path("meta").unwrap(), json!(1)).unwrap();
        assert_eq!(doc["meta"], 1);
    }

    #[test]
    fn missing_parents_and_hostile_indexes_fail_cleanly() {
        let mut doc = json!({"layers": []});
        assert_eq!(get(&doc, &parse_path("layers[5]").unwrap()).unwrap_err().kind, ErrorKind::NotFound);
        assert_eq!(set(&mut doc, &parse_path("layers[5]").unwrap(), json!(1)).unwrap_err().kind, ErrorKind::NotFound);
        assert_eq!(set(&mut doc, &parse_path("nope.deeper").unwrap(), json!(1)).unwrap_err().kind, ErrorKind::NotFound);
        assert!(get(&doc, &parse_path("layers[99999999999999999999999]").unwrap()).is_err());
        assert_eq!(set(&mut doc, &[], json!(1)).unwrap_err().kind, ErrorKind::InvalidParams);
    }
}
