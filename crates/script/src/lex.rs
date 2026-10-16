//! The tokenizer.
//!
//! It covers the whole source (whitespace and comments included), so the same tokens drive both the parser and syntax
//! highlighting, and it never fails: anything it does not understand becomes an [`TokenKind::Error`] token one
//! character long.

use serde::{Deserialize, Serialize};

/// What a token is, as far as highlighting cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenKind {
    Whitespace,
    Comment,
    /// A reserved word (`var`, `function`, `if`, ...).
    Keyword,
    /// A name the runtime gives meaning to (`stop`, `gotoAndPlay`, `MouseEvent`, ...).
    Builtin,
    Ident,
    Number,
    String,
    /// An operator or punctuation mark.
    Punct,
    /// A character that starts no token, or a string or comment that is not closed.
    Error,
}

/// A token: a kind and a byte range of the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    /// The token's text.
    #[must_use]
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        src.get(self.start..self.end).unwrap_or("")
    }

    /// True for whitespace and comments, which the parser skips.
    #[must_use]
    pub const fn is_trivia(&self) -> bool {
        matches!(self.kind, TokenKind::Whitespace | TokenKind::Comment)
    }
}

/// Words that are reserved in `ActionScript` 2 or 3 and appear in frame scripts.
pub const KEYWORDS: &[&str] = &[
    "var",
    "function",
    "if",
    "else",
    "return",
    "new",
    "this",
    "true",
    "false",
    "null",
    "undefined",
    "for",
    "while",
    "do",
    "switch",
    "case",
    "default",
    "break",
    "continue",
    "try",
    "catch",
    "finally",
    "throw",
    "import",
    "package",
    "class",
    "extends",
    "implements",
    "interface",
    "public",
    "private",
    "protected",
    "static",
    "const",
    "override",
    "in",
    "is",
    "as",
    "typeof",
    "delete",
    "void",
    "with",
    "get",
    "set",
    "on",
    "onClipEvent",
];

/// Names the interpreter implements, highlighted differently from user names.
pub const BUILTINS: &[&str] = &[
    "stop",
    "play",
    "gotoAndPlay",
    "gotoAndStop",
    "nextFrame",
    "prevFrame",
    "nextScene",
    "prevScene",
    "trace",
    "addEventListener",
    "removeEventListener",
    "MouseEvent",
    "Event",
    "MovieClip",
    "root",
    "_root",
    "parent",
    "_parent",
    "currentFrame",
    "_currentframe",
    "totalFrames",
    "_totalframes",
    "currentLabel",
    "Number",
    "String",
    "int",
    "uint",
    "Boolean",
    "SimpleButton",
    "stage",
];

/// Splits `src` into tokens that together cover every byte of it.
#[must_use]
pub fn tokenize(src: &str) -> Vec<Token> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|(_, c)| *c);
    let offset = |i: usize| chars.get(i).map_or(src.len(), |(o, _)| *o);
    let mut out = Vec::new();
    let mut i = 0_usize;
    while let Some(c) = at(i) {
        let start = i;
        let kind;
        if c.is_whitespace() {
            while at(i).is_some_and(char::is_whitespace) {
                i = i.saturating_add(1);
            }
            kind = TokenKind::Whitespace;
        } else if c == '/' && at(i.saturating_add(1)) == Some('/') {
            while at(i).is_some_and(|c| c != '\n') {
                i = i.saturating_add(1);
            }
            kind = TokenKind::Comment;
        } else if c == '/' && at(i.saturating_add(1)) == Some('*') {
            i = i.saturating_add(2);
            let mut closed = false;
            while let Some(c) = at(i) {
                i = i.saturating_add(1);
                if c == '*' && at(i) == Some('/') {
                    i = i.saturating_add(1);
                    closed = true;
                    break;
                }
            }
            kind = if closed { TokenKind::Comment } else { TokenKind::Error };
        } else if c == '"' || c == '\'' {
            i = i.saturating_add(1);
            let mut closed = false;
            while let Some(d) = at(i) {
                i = i.saturating_add(1);
                if d == '\\' {
                    i = i.saturating_add(1);
                } else if d == c {
                    closed = true;
                    break;
                } else if d == '\n' {
                    break;
                }
            }
            i = i.min(chars.len());
            kind = if closed { TokenKind::String } else { TokenKind::Error };
        } else if c.is_ascii_digit() || (c == '.' && at(i.saturating_add(1)).is_some_and(|d| d.is_ascii_digit())) {
            if c == '0' && matches!(at(i.saturating_add(1)), Some('x' | 'X')) {
                i = i.saturating_add(2);
                while at(i).is_some_and(|d| d.is_ascii_hexdigit()) {
                    i = i.saturating_add(1);
                }
            } else {
                while at(i).is_some_and(|d| d.is_ascii_digit() || d == '.') {
                    i = i.saturating_add(1);
                }
                if matches!(at(i), Some('e' | 'E')) {
                    let sign = matches!(at(i.saturating_add(1)), Some('+' | '-'));
                    let digit_at = i.saturating_add(if sign { 2 } else { 1 });
                    if at(digit_at).is_some_and(|d| d.is_ascii_digit()) {
                        i = digit_at;
                        while at(i).is_some_and(|d| d.is_ascii_digit()) {
                            i = i.saturating_add(1);
                        }
                    }
                }
            }
            kind = TokenKind::Number;
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            while at(i).is_some_and(|d| d.is_alphanumeric() || d == '_' || d == '$') {
                i = i.saturating_add(1);
            }
            let word = src.get(offset(start)..offset(i)).unwrap_or("");
            kind = if KEYWORDS.contains(&word) {
                TokenKind::Keyword
            } else if BUILTINS.contains(&word) {
                TokenKind::Builtin
            } else {
                TokenKind::Ident
            };
        } else if let Some(len) = punct_len(&chars, i) {
            i = i.saturating_add(len);
            kind = TokenKind::Punct;
        } else {
            i = i.saturating_add(1);
            kind = TokenKind::Error;
        }
        out.push(Token { kind, start: offset(start), end: offset(i) });
    }
    out
}

const PUNCT3: &[&str] = &["===", "!==", ">>>", "<<=", ">>=", "..."];
const PUNCT2: &[&str] = &["==", "!=", "<=", ">=", "&&", "||", "++", "--", "+=", "-=", "*=", "/=", "%=", "<<", ">>", "::", "=>", ".<"];
const PUNCT1: &str = "{}()[];,.:?+-*/%=<>!&|^~@";

fn punct_len(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let take = |n: usize| -> String { chars.iter().skip(i).take(n).map(|(_, c)| *c).collect() };
    if PUNCT3.contains(&take(3).as_str()) {
        return Some(3);
    }
    if PUNCT2.contains(&take(2).as_str()) {
        return Some(2);
    }
    let one = chars.get(i).map(|(_, c)| *c)?;
    PUNCT1.contains(one).then_some(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, &str)> {
        tokenize(src).into_iter().filter(|t| t.kind != TokenKind::Whitespace).map(|t| (t.kind, t.text(src))).collect()
    }

    #[test]
    fn covers_every_byte() {
        let src = "stop(); // done\nvar x:int = 0x1F + 2.5e3; /* c */ trace(\"שלום\");";
        let tokens = tokenize(src);
        let mut pos = 0;
        for t in &tokens {
            assert_eq!(t.start, pos);
            pos = t.end;
        }
        assert_eq!(pos, src.len());
    }

    #[test]
    fn classifies_words() {
        let k = kinds("var n = gotoAndPlay;");
        assert_eq!(k.first(), Some(&(TokenKind::Keyword, "var")));
        assert_eq!(k.get(1), Some(&(TokenKind::Ident, "n")));
        assert_eq!(k.get(3), Some(&(TokenKind::Builtin, "gotoAndPlay")));
    }

    #[test]
    fn unterminated_string_and_comment_are_errors_not_panics() {
        assert_eq!(kinds("\"abc").first().map(|k| k.0), Some(TokenKind::Error));
        assert_eq!(kinds("/* abc").first().map(|k| k.0), Some(TokenKind::Error));
        assert_eq!(tokenize("\"\\").len(), 1);
    }

    #[test]
    fn operators_are_longest_match() {
        let k = kinds("a === b >>> 2 ++");
        assert!(k.contains(&(TokenKind::Punct, "===")));
        assert!(k.contains(&(TokenKind::Punct, ">>>")));
        assert!(k.contains(&(TokenKind::Punct, "++")));
    }
}
