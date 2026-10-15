//! The JSON-lines wire format shared by [`crate::ControlServer`] and [`crate::BridgeClient`].

use std::io::{self, BufRead, Read, Write};

use serde_json::{Map, Value, json};

use crate::protocol::{ControlError, Event, Progress, Reply};

/// What reading one line produced.
#[derive(Debug)]
pub enum Line {
    /// End of input.
    Eof,
    /// One line of text without its newline.
    Text(String),
    /// The line was longer than the limit (the rest of the stream is unusable).
    TooLong,
    /// The line was not valid UTF-8.
    NotUtf8,
}

/// Reads one `\n`-terminated line of at most `max` bytes.
///
/// # Errors
/// I/O errors from the reader.
pub fn read_line<R: BufRead>(reader: &mut R, max: u64) -> io::Result<Line> {
    let mut buf = Vec::new();
    let read = Read::take(&mut *reader, max.saturating_add(1)).read_until(b'\n', &mut buf)?;
    if read == 0 {
        return Ok(Line::Eof);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    } else if u64::try_from(buf.len()).unwrap_or(u64::MAX) > max {
        return Ok(Line::TooLong);
    }
    Ok(String::from_utf8(buf).map_or(Line::NotUtf8, Line::Text))
}

/// Writes `value` as one line and flushes.
///
/// # Errors
/// I/O errors from the writer.
pub fn write_line<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let mut text = value.to_string();
    text.push('\n');
    writer.write_all(text.as_bytes())?;
    writer.flush()
}

/// The wire form of an event for call `id`.
#[must_use]
pub fn event_line(id: u64, event: &Event) -> Value {
    match event {
        Event::Progress(p) => json!({ "id": id, "progress": { "fraction": p.fraction, "message": p.message } }),
        Event::Done(Ok(reply)) => {
            let mut line = Map::new();
            line.insert("id".into(), json!(id));
            line.insert("ok".into(), Value::Bool(true));
            line.insert("result".into(), reply.value.clone());
            if !reply.images.is_empty() {
                line.insert("images".into(), serde_json::to_value(&reply.images).unwrap_or(Value::Null));
            }
            Value::Object(line)
        }
        Event::Done(Err(error)) => json!({ "id": id, "ok": false, "error": error }),
    }
}

/// Parses a server line back into `(call id, event)`. Returns `None` for lines that are not call events.
#[must_use]
pub fn parse_event_line(line: &Value) -> Option<(u64, Event)> {
    let id = line.get("id")?.as_u64()?;
    if let Some(progress) = line.get("progress") {
        let progress: Progress = serde_json::from_value(progress.clone()).ok()?;
        return Some((id, Event::Progress(progress)));
    }
    let ok = line.get("ok")?.as_bool()?;
    let outcome = if ok {
        let images = line.get("images").map(|v| serde_json::from_value(v.clone())).transpose().ok()?.unwrap_or_default();
        Ok(Reply { value: line.get("result").cloned().unwrap_or(Value::Null), images })
    } else {
        Err(serde_json::from_value::<ControlError>(line.get("error")?.clone()).ok()?)
    };
    Some((id, Event::Done(outcome)))
}

/// Constant-time comparison for tokens.
#[must_use]
pub fn tokens_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = u8::from(a.len() != b.len());
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ErrorKind;
    use std::io::Cursor;

    #[test]
    fn lines_are_bounded() {
        let mut ok = Cursor::new(b"hello\r\nworld\n".to_vec());
        assert!(matches!(read_line(&mut ok, 100).unwrap(), Line::Text(t) if t == "hello"));
        assert!(matches!(read_line(&mut ok, 100).unwrap(), Line::Text(t) if t == "world"));
        assert!(matches!(read_line(&mut ok, 100).unwrap(), Line::Eof));
        let mut long = Cursor::new(vec![b'a'; 50]);
        assert!(matches!(read_line(&mut long, 10).unwrap(), Line::TooLong));
        let mut bad = Cursor::new(vec![0xff, 0xfe, b'\n']);
        assert!(matches!(read_line(&mut bad, 10).unwrap(), Line::NotUtf8));
    }

    #[test]
    fn events_round_trip() {
        let ok = Event::Done(Ok(Reply::value(json!({"a": 1})).with_png_rgba(1, 1, &[0, 0, 0, 255]).unwrap()));
        let err = Event::Done(Err(ControlError::new(ErrorKind::Denied, "no")));
        let prog = Event::Progress(Progress { fraction: 0.5, message: "half".into() });
        for event in [ok, err, prog] {
            let line = event_line(7, &event);
            assert_eq!(parse_event_line(&line), Some((7, event)));
        }
    }

    #[test]
    fn token_comparison() {
        assert!(tokens_match("abc", "abc"));
        assert!(!tokens_match("abc", "abd"));
        assert!(!tokens_match("abc", "abcd"));
        assert!(!tokens_match("", "x"));
    }
}
