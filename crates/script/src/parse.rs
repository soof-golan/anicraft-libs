//! A recursive-descent parser for the supported subset.
//!
//! Supported: `var` declarations, expression statements, `if`/`else`, blocks, function declarations and function
//! literals, `return`, calls, member access, the arithmetic, comparison and logical operators, assignment (plain and
//! compound), `++`/`--`, and type annotations (parsed and ignored). `import` lines are skipped.
//!
//! Everything else (loops, `switch`, `try`, classes, `new`, array and object literals, the conditional operator,
//! bracket access) is outside the subset: the statement is kept as [`Stmt::Unsupported`], a [`Diagnostic`] says
//! which, and parsing carries on after it.

use crate::ast::{BinOp, Diagnostic, Expr, Func, Script, Stmt, UnOp};
use crate::lex::{Token, TokenKind, tokenize};

/// The deepest nesting of blocks, calls and parentheses the parser follows.
const MAX_DEPTH: u32 = 64;
/// The longest run of operators or member accesses in one expression, so dropping the tree never recurses deeply.
const MAX_CHAIN: u32 = 256;

struct ParseError {
    at: usize,
    message: String,
}

type PResult<T> = Result<T, ParseError>;

/// Parses a script. Never fails: what cannot be parsed is reported in [`Script::diagnostics`].
#[must_use]
pub fn parse(src: &str) -> Script {
    let tokens: Vec<Token> = tokenize(src).into_iter().filter(|t| !t.is_trivia()).collect();
    let mut p = Parser { src, tokens, pos: 0, depth: 0, diagnostics: Vec::new() };
    let stmts = p.statements(false);
    Script { stmts, diagnostics: p.diagnostics }
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    depth: u32,
    diagnostics: Vec<Diagnostic>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<Token> {
        self.tokens.get(self.pos.saturating_add(n)).copied()
    }

    fn text(&self, t: Token) -> &str {
        t.text(self.src)
    }

    fn is_punct(&self, s: &str) -> bool {
        self.peek().is_some_and(|t| t.kind == TokenKind::Punct && self.text(t) == s)
    }

    fn is_word(&self, s: &str) -> bool {
        self.peek().is_some_and(|t| matches!(t.kind, TokenKind::Keyword | TokenKind::Ident | TokenKind::Builtin) && self.text(t) == s)
    }

    fn eat_punct(&mut self, s: &str) -> bool {
        let hit = self.is_punct(s);
        if hit {
            self.pos = self.pos.saturating_add(1);
        }
        hit
    }

    fn eat_word(&mut self, s: &str) -> bool {
        let hit = self.is_word(s);
        if hit {
            self.pos = self.pos.saturating_add(1);
        }
        hit
    }

    fn here(&self) -> usize {
        self.peek().map_or(self.src.len(), |t| t.start)
    }

    fn err<T>(&self, message: impl Into<String>) -> PResult<T> {
        Err(ParseError { at: self.here(), message: message.into() })
    }

    fn expect_punct(&mut self, s: &str) -> PResult<()> {
        if self.eat_punct(s) { Ok(()) } else { self.err(format!("expected `{s}`")) }
    }

    fn line_of(&self, offset: usize) -> u32 {
        let before = self.src.get(..offset).unwrap_or("");
        u32::try_from(before.bytes().filter(|b| *b == b'\n').count()).unwrap_or(u32::MAX).saturating_add(1)
    }

    fn name(&mut self) -> PResult<String> {
        match self.peek() {
            Some(t)
                if matches!(t.kind, TokenKind::Ident | TokenKind::Builtin)
                    || (t.kind == TokenKind::Keyword && matches!(self.text(t), "get" | "set" | "on")) =>
            {
                self.pos = self.pos.saturating_add(1);
                Ok(self.text(t).to_owned())
            }
            _ => self.err("expected a name"),
        }
    }

    /// Statements up to the end of input, or up to the `}` that closes a block when `in_block`.
    fn statements(&mut self, in_block: bool) -> Vec<Stmt> {
        let mut out = Vec::new();
        while let Some(t) = self.peek() {
            if in_block && t.kind == TokenKind::Punct && self.text(t) == "}" {
                break;
            }
            let start = self.pos;
            match self.statement() {
                Ok(Some(s)) => out.push(s),
                Ok(None) => {}
                Err(e) => {
                    let line = self.line_of(e.at);
                    self.diagnostics.push(Diagnostic { line, message: e.message });
                    self.pos = start;
                    let text = self.skip_statement(in_block);
                    out.push(Stmt::Unsupported { line, text });
                }
            }
            if self.pos == start {
                self.pos = self.pos.saturating_add(1);
            }
        }
        out
    }

    /// Skips one statement the parser could not read and returns its source text.
    fn skip_statement(&mut self, in_block: bool) -> String {
        let first = self.here();
        let mut braces = 0_u32;
        let mut parens = 0_u32;
        while let Some(t) = self.peek() {
            let text = self.text(t);
            if t.kind == TokenKind::Punct {
                match text {
                    "(" | "[" => parens = parens.saturating_add(1),
                    ")" | "]" => parens = parens.saturating_sub(1),
                    "{" => braces = braces.saturating_add(1),
                    "}" => {
                        if braces == 0 {
                            if in_block {
                                break;
                            }
                        } else {
                            braces = braces.saturating_sub(1);
                            if braces == 0 && parens == 0 {
                                self.pos = self.pos.saturating_add(1);
                                // `else`, `catch` and `finally` continue the statement.
                                if self.is_word("else") || self.is_word("catch") || self.is_word("finally") {
                                    continue;
                                }
                                break;
                            }
                        }
                    }
                    ";" if braces == 0 && parens == 0 => {
                        self.pos = self.pos.saturating_add(1);
                        break;
                    }
                    _ => {}
                }
            }
            self.pos = self.pos.saturating_add(1);
        }
        let last = self.tokens.get(self.pos.saturating_sub(1)).map_or(first, |t| t.end);
        self.src.get(first..last.max(first)).unwrap_or("").trim().to_owned()
    }

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect_punct("{")?;
        self.depth = self.depth.saturating_add(1);
        if self.depth > MAX_DEPTH {
            self.depth = self.depth.saturating_sub(1);
            return self.err("blocks are nested too deeply");
        }
        let body = self.statements(true);
        self.depth = self.depth.saturating_sub(1);
        self.expect_punct("}")?;
        Ok(body)
    }

    fn body_of(&mut self) -> PResult<Vec<Stmt>> {
        if self.is_punct("{") { self.block() } else { Ok(self.statement()?.into_iter().collect()) }
    }

    fn statement(&mut self) -> PResult<Option<Stmt>> {
        let Some(t) = self.peek() else { return Ok(None) };
        let word = if matches!(t.kind, TokenKind::Keyword | TokenKind::Ident | TokenKind::Builtin) { self.text(t).to_owned() } else { String::new() };
        if t.kind == TokenKind::Punct {
            match self.text(t) {
                ";" => {
                    self.pos = self.pos.saturating_add(1);
                    return Ok(None);
                }
                "{" => return Ok(Some(Stmt::Block(self.block()?))),
                _ => {}
            }
        }
        match word.as_str() {
            "import" => {
                while let Some(t) = self.peek() {
                    self.pos = self.pos.saturating_add(1);
                    if t.kind == TokenKind::Punct && self.text(t) == ";" {
                        break;
                    }
                }
                Ok(None)
            }
            "var" | "const" => {
                self.pos = self.pos.saturating_add(1);
                let name = self.name()?;
                self.skip_type()?;
                let init = if self.eat_punct("=") { Some(self.expression()?) } else { None };
                if self.is_punct(",") {
                    return self.err("several declarations in one `var` are outside the supported subset");
                }
                self.end_of_statement();
                Ok(Some(Stmt::Var { name, init }))
            }
            "function" if self.peek_at(1).is_some_and(|n| n.kind != TokenKind::Punct) => {
                self.pos = self.pos.saturating_add(1);
                let func = self.function_rest(true)?;
                Ok(Some(Stmt::Function(func)))
            }
            "if" => {
                self.pos = self.pos.saturating_add(1);
                self.expect_punct("(")?;
                let cond = self.expression()?;
                self.expect_punct(")")?;
                let then = self.body_of()?;
                let otherwise = if self.eat_word("else") { self.body_of()? } else { Vec::new() };
                Ok(Some(Stmt::If { cond, then, otherwise }))
            }
            "return" => {
                self.pos = self.pos.saturating_add(1);
                let value = if self.is_punct(";") || self.is_punct("}") || self.peek().is_none() { None } else { Some(self.expression()?) };
                self.end_of_statement();
                Ok(Some(Stmt::Return(value)))
            }
            "for" | "while" | "do" | "switch" | "try" | "throw" | "with" | "break" | "continue" => {
                self.err(format!("`{word}` is outside the supported subset (no loops, switch or exceptions)"))
            }
            "package" | "class" | "interface" | "public" | "private" | "internal" | "protected" | "static" | "override" => {
                self.err(format!("`{word}`: classes and packages are outside the supported subset"))
            }
            _ => {
                let e = self.expression()?;
                self.end_of_statement();
                Ok(Some(Stmt::Expr(e)))
            }
        }
    }

    fn end_of_statement(&mut self) {
        self.eat_punct(";");
    }

    /// After `function` (and the name, for declarations): parameters, return type and body.
    fn function_rest(&mut self, named: bool) -> PResult<Func> {
        let name = if named || self.peek().is_some_and(|t| t.kind == TokenKind::Ident) { Some(self.name()?) } else { None };
        self.expect_punct("(")?;
        let mut params = Vec::new();
        while !self.is_punct(")") {
            if self.eat_punct("...") {
                return self.err("rest parameters are outside the supported subset");
            }
            params.push(self.name()?);
            self.skip_type()?;
            if self.eat_punct("=") {
                return self.err("default parameter values are outside the supported subset");
            }
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct(")")?;
        self.skip_type()?;
        let body = self.block()?;
        Ok(Func { name, params, body })
    }

    /// `: Type`, `: *` or `: Vector.<T>`, ignored.
    fn skip_type(&mut self) -> PResult<()> {
        if !self.eat_punct(":") {
            return Ok(());
        }
        if self.eat_punct("*") || self.eat_word("void") || self.eat_word("null") {
            return Ok(());
        }
        self.name()?;
        loop {
            if self.eat_punct(".<") {
                let mut depth = 1_u32;
                while depth > 0 {
                    let Some(t) = self.peek() else { return self.err("unterminated type") };
                    self.pos = self.pos.saturating_add(1);
                    match self.text(t) {
                        "<" | ".<" => depth = depth.saturating_add(1),
                        ">" => depth = depth.saturating_sub(1),
                        ">>" => depth = depth.saturating_sub(2),
                        _ => {}
                    }
                }
            } else if self.is_punct(".") && self.peek_at(1).is_some_and(|t| matches!(t.kind, TokenKind::Ident | TokenKind::Builtin)) {
                self.pos = self.pos.saturating_add(1);
                self.name()?;
            } else {
                return Ok(());
            }
        }
    }

    fn expression(&mut self) -> PResult<Expr> {
        self.depth = self.depth.saturating_add(1);
        if self.depth > MAX_DEPTH {
            self.depth = self.depth.saturating_sub(1);
            return self.err("expression is nested too deeply");
        }
        let e = self.assignment();
        self.depth = self.depth.saturating_sub(1);
        e
    }

    fn assignment(&mut self) -> PResult<Expr> {
        let left = self.binary(0)?;
        if self.is_punct("?") {
            return self.err("the conditional operator `?:` is outside the supported subset");
        }
        let op = self.peek().filter(|t| t.kind == TokenKind::Punct).map(|t| self.text(t).to_owned());
        let compound = match op.as_deref() {
            Some("=") => Some(None),
            Some("+=") => Some(Some(BinOp::Add)),
            Some("-=") => Some(Some(BinOp::Sub)),
            Some("*=") => Some(Some(BinOp::Mul)),
            Some("/=") => Some(Some(BinOp::Div)),
            Some("%=") => Some(Some(BinOp::Mod)),
            _ => None,
        };
        if let Some(op) = compound {
            if !matches!(left, Expr::Ident(_) | Expr::Member(..)) {
                return self.err("only names and properties can be assigned to");
            }
            self.pos = self.pos.saturating_add(1);
            let right = self.assignment()?;
            return Ok(Expr::Assign(Box::new(left), op, Box::new(right)));
        }
        Ok(left)
    }

    fn binary_op(&self) -> Option<(BinOp, u8)> {
        let t = self.peek()?;
        if t.kind != TokenKind::Punct {
            return None;
        }
        Some(match self.text(t) {
            "||" => (BinOp::Or, 1),
            "&&" => (BinOp::And, 2),
            "==" => (BinOp::Eq, 3),
            "!=" => (BinOp::Ne, 3),
            "===" => (BinOp::StrictEq, 3),
            "!==" => (BinOp::StrictNe, 3),
            "<" => (BinOp::Lt, 4),
            "<=" => (BinOp::Le, 4),
            ">" => (BinOp::Gt, 4),
            ">=" => (BinOp::Ge, 4),
            "+" => (BinOp::Add, 5),
            "-" => (BinOp::Sub, 5),
            "*" => (BinOp::Mul, 6),
            "/" => (BinOp::Div, 6),
            "%" => (BinOp::Mod, 6),
            _ => return None,
        })
    }

    fn binary(&mut self, min: u8) -> PResult<Expr> {
        let mut left = self.unary()?;
        let mut chain = 0_u32;
        while let Some((op, prec)) = self.binary_op() {
            if prec < min {
                break;
            }
            chain = chain.saturating_add(1);
            if chain > MAX_CHAIN {
                return self.err("operator chain is too long");
            }
            self.pos = self.pos.saturating_add(1);
            let right = self.binary(prec.saturating_add(1))?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> PResult<Expr> {
        self.depth = self.depth.saturating_add(1);
        let e = if self.depth > MAX_DEPTH { self.err("expression is nested too deeply") } else { self.unary_inner() };
        self.depth = self.depth.saturating_sub(1);
        e
    }

    fn unary_inner(&mut self) -> PResult<Expr> {
        if self.eat_punct("!") {
            return Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)));
        }
        if self.eat_punct("-") {
            return Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?)));
        }
        if self.eat_punct("+") {
            return self.unary();
        }
        for (s, delta) in [("++", 1.0), ("--", -1.0)] {
            if self.eat_punct(s) {
                let target = self.unary()?;
                return Ok(Expr::Update { target: Box::new(target), delta, prefix: true });
            }
        }
        let e = self.postfix()?;
        for (s, delta) in [("++", 1.0), ("--", -1.0)] {
            if self.eat_punct(s) {
                return Ok(Expr::Update { target: Box::new(e), delta, prefix: false });
            }
        }
        Ok(e)
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        let mut chain = 0_u32;
        loop {
            chain = chain.saturating_add(1);
            if chain > MAX_CHAIN {
                return self.err("member chain is too long");
            }
            if self.eat_punct(".") {
                e = Expr::Member(Box::new(e), self.name()?);
            } else if self.is_punct("(") {
                self.pos = self.pos.saturating_add(1);
                let mut args = Vec::new();
                while !self.is_punct(")") {
                    args.push(self.expression()?);
                    if !self.eat_punct(",") {
                        break;
                    }
                }
                self.expect_punct(")")?;
                e = Expr::Call(Box::new(e), args);
            } else if self.is_punct("[") {
                return self.err("bracket access `a[b]` is outside the supported subset");
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let Some(t) = self.peek() else { return self.err("unexpected end of script") };
        let text = self.text(t).to_owned();
        match t.kind {
            TokenKind::Number => {
                self.pos = self.pos.saturating_add(1);
                Ok(Expr::Num(parse_number(&text)))
            }
            TokenKind::String => {
                self.pos = self.pos.saturating_add(1);
                Ok(Expr::Str(unescape(&text)))
            }
            TokenKind::Keyword => match text.as_str() {
                "true" | "false" => {
                    self.pos = self.pos.saturating_add(1);
                    Ok(Expr::Bool(text == "true"))
                }
                "null" | "undefined" => {
                    self.pos = self.pos.saturating_add(1);
                    Ok(Expr::Null)
                }
                "this" => {
                    self.pos = self.pos.saturating_add(1);
                    Ok(Expr::This)
                }
                "function" => {
                    self.pos = self.pos.saturating_add(1);
                    Ok(Expr::Function(Box::new(self.function_rest(false)?)))
                }
                "get" | "set" | "on" => {
                    self.pos = self.pos.saturating_add(1);
                    Ok(Expr::Ident(text))
                }
                "new" => self.err("`new` is outside the supported subset"),
                other => self.err(format!("unexpected `{other}`")),
            },
            TokenKind::Ident | TokenKind::Builtin => {
                self.pos = self.pos.saturating_add(1);
                Ok(Expr::Ident(text))
            }
            TokenKind::Punct if text == "(" => {
                self.pos = self.pos.saturating_add(1);
                let e = self.expression()?;
                self.expect_punct(")")?;
                Ok(e)
            }
            TokenKind::Punct if text == "{" || text == "[" => self.err("object and array literals are outside the supported subset"),
            _ => self.err(format!("unexpected `{text}`")),
        }
    }
}

fn parse_number(text: &str) -> f64 {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(0.0, |v| f64::from(u32::try_from(v).unwrap_or(u32::MAX)));
    }
    text.parse().unwrap_or(0.0)
}

fn unescape(quoted: &str) -> String {
    let inner = quoted.get(1..quoted.len().saturating_sub(1)).unwrap_or("");
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_timeline_subset() {
        let s = parse("stop();\nthis.gotoAndPlay(\"end\");\nvar n:int = 3;\nif (n > 2) { nextFrame(); } else { prevFrame(); }");
        assert!(s.diagnostics.is_empty(), "{:?}", s.diagnostics);
        assert_eq!(s.stmts.len(), 4);
        assert!(matches!(s.stmts.get(2), Some(Stmt::Var { name, init: Some(Expr::Num(v)) }) if name == "n" && (*v - 3.0).abs() < f64::EPSILON));
    }

    #[test]
    fn parses_handlers() {
        let s = parse(
            "function onGo(e:MouseEvent):void { gotoAndStop(\"end\"); }\nbtn.addEventListener(MouseEvent.CLICK, onGo);\nbtn2.onRelease = function() { play(); };",
        );
        assert!(s.diagnostics.is_empty(), "{:?}", s.diagnostics);
        assert!(matches!(s.stmts.first(), Some(Stmt::Function(f)) if f.name.as_deref() == Some("onGo") && f.params == ["e"]));
        assert!(matches!(s.stmts.get(2), Some(Stmt::Expr(Expr::Assign(..)))));
    }

    #[test]
    fn unsupported_statements_are_reported_and_skipped() {
        let s = parse("stop();\nfor (var i:int = 0; i < 3; i++) { trace(i); }\nplay();\nvar a = new Foo();\nnextFrame();");
        assert_eq!(s.diagnostics.len(), 2, "{:?}", s.diagnostics);
        assert_eq!(s.diagnostics.first().map(|d| d.line), Some(2));
        assert_eq!(s.stmts.iter().filter(|s| !matches!(s, Stmt::Unsupported { .. })).count(), 3);
    }

    #[test]
    fn hostile_input_does_not_panic() {
        for src in ["(((((", "}}}}", "function", "if (", "\"abc", "a.", "x = ", &"(".repeat(5000), &"{".repeat(5000), "var", "else"] {
            let _ = parse(src);
        }
    }
}
