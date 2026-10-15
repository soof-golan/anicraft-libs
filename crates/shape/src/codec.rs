//! The text syntaxes of XFL `<Edge>` elements.
//!
//! `edges` is a path in twips: `!x y` moves, `|x y` (or `/x y`) draws a line, `[cx cy x y` a quadratic curve and
//! `S<digits>` is a style marker that carries nothing we keep. Animate writes every segment with its own `!`, but a
//! single `!` followed by several `|`/`[` commands is equally valid, and so is repeating the numbers of the previous
//! command without its letter.
//!
//! `cubics` carries the true cubic curve next to the quadratic approximation that `edges` has to make do with:
//! `!x y(;c1x,c1y c2x,c2y x,y q<x y>Q<cx cy>q<x y>...);` where the `q`/`p` (on-curve) and `Q` (control) list repeats the quadratic pieces the
//! cubic replaces (rounded to whole twips), which is how a cubic is matched back to the edge it belongs to.

use std::fmt::Write as _;

use anim_core::Point;
use anim_core::kurbo::CubicBez;

use crate::error::{ParseError, ParseErrorKind};
use crate::number::{format_number, parse_number, quantize};
use crate::segment::Segment;

/// Longest attribute text accepted, in bytes.
pub const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;

/// Most segments accepted from one attribute.
pub const MAX_SEGMENTS: usize = 1_000_000;

/// Accuracy, in twips, of the quadratic approximation written next to a cubic curve.
pub const APPROX_ACCURACY: f64 = 0.25;

/// A cubic curve from a `cubics` attribute plus the quadratic segments it stands in for.
#[derive(Debug, Clone, PartialEq)]
pub struct CubicRun {
    /// Always a [`Segment::Cubic`].
    pub cubic: Segment,
    /// The quadratic/line pieces listed after the control points (possibly empty).
    pub approx: Vec<Segment>,
}

struct Cursor<'a> {
    text: &'a str,
    pos: usize,
    /// Treat `,` and `;` as whitespace (the `cubics` syntax).
    loose: bool,
}

impl<'a> Cursor<'a> {
    const fn new(text: &'a str, loose: bool) -> Result<Self, ParseError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(ParseError { offset: 0, kind: ParseErrorKind::TooLong(MAX_TEXT_BYTES) });
        }
        Ok(Self { text, pos: 0, loose })
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    const fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    const fn error(&self, kind: ParseErrorKind) -> ParseError {
        ParseError { offset: self.pos, kind }
    }

    fn unexpected(&self) -> ParseError {
        let kind = self.text.get(self.pos..).and_then(|rest| rest.chars().next()).map_or(ParseErrorKind::UnexpectedEnd, ParseErrorKind::UnexpectedChar);
        self.error(kind)
    }

    fn skip_separators(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() || (self.loose && (b == b',' || b == b';')) {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn skip_digits(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.bump();
        }
    }

    fn at_number(&self) -> bool {
        matches!(self.peek(), Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'#'))
    }

    fn take_while(&mut self, pred: impl Fn(u8) -> bool) {
        while self.peek().is_some_and(&pred) {
            self.bump();
        }
    }

    fn number(&mut self) -> Result<f64, ParseError> {
        self.skip_separators();
        let start = self.pos;
        if self.peek() == Some(b'#') {
            self.bump();
            self.take_while(|b| b.is_ascii_hexdigit());
            if self.peek() == Some(b'.') {
                self.bump();
                self.take_while(|b| b.is_ascii_hexdigit());
            }
        } else {
            if matches!(self.peek(), Some(b'-' | b'+')) {
                self.bump();
            }
            self.take_while(|b| b.is_ascii_digit());
            if self.peek() == Some(b'.') {
                self.bump();
                self.take_while(|b| b.is_ascii_digit());
            }
        }
        if self.pos == start {
            return Err(self.unexpected());
        }
        let token = self.text.get(start..self.pos).ok_or_else(|| self.error(ParseErrorKind::BadNumber))?;
        parse_number(token).map_err(|kind| ParseError { offset: start, kind })
    }

    fn point(&mut self) -> Result<Point, ParseError> {
        let x = self.number()?;
        let y = self.number()?;
        Ok(Point::new(x, y))
    }
}

const fn check_count(cur: &Cursor<'_>, count: usize) -> Result<(), ParseError> {
    if count >= MAX_SEGMENTS { Err(cur.error(ParseErrorKind::TooManySegments(MAX_SEGMENTS))) } else { Ok(()) }
}

struct EdgeParser<'a> {
    cur: Cursor<'a>,
    pen: Option<Point>,
    last: Option<u8>,
    segments: Vec<Segment>,
}

impl EdgeParser<'_> {
    fn run(mut self) -> Result<Vec<Segment>, ParseError> {
        loop {
            self.cur.skip_separators();
            let Some(c) = self.cur.peek() else { return Ok(self.segments) };
            match c {
                b'!' | b'|' | b'/' | b'[' => {
                    self.cur.bump();
                    self.command(c)?;
                }
                b'S' => {
                    self.cur.bump();
                    self.cur.skip_digits();
                }
                _ if self.cur.at_number() => {
                    let repeat = self.last.filter(|last| *last != b'!').ok_or_else(|| self.cur.unexpected())?;
                    self.command(repeat)?;
                }
                _ => return Err(self.cur.unexpected()),
            }
        }
    }

    fn command(&mut self, c: u8) -> Result<(), ParseError> {
        self.last = Some(c);
        if c == b'!' {
            self.pen = Some(self.cur.point()?);
            return Ok(());
        }
        let segment = if c == b'[' {
            let ctrl = self.cur.point()?;
            let to = self.cur.point()?;
            let from = self.pen_or_error()?;
            Segment::Quad { from, ctrl, to }
        } else {
            let to = self.cur.point()?;
            let from = self.pen_or_error()?;
            Segment::Line { from, to }
        };
        check_count(&self.cur, self.segments.len())?;
        self.pen = Some(segment.end());
        self.segments.push(segment);
        Ok(())
    }

    fn pen_or_error(&self) -> Result<Point, ParseError> {
        self.pen.ok_or_else(|| self.cur.error(ParseErrorKind::MissingMoveTo))
    }
}

/// Parses an `edges` attribute into segments (twips).
///
/// # Errors
/// A [`ParseError`] for unknown commands, malformed numbers, a draw command before any `!`, or input beyond the
/// size limits.
pub fn parse_edges(text: &str) -> Result<Vec<Segment>, ParseError> {
    EdgeParser { cur: Cursor::new(text, false)?, pen: None, last: None, segments: Vec::new() }.run()
}

struct CubicParser<'a> {
    cur: Cursor<'a>,
    pen: Option<Point>,
    runs: Vec<CubicRun>,
    total: usize,
}

impl CubicParser<'_> {
    fn run(mut self) -> Result<Vec<CubicRun>, ParseError> {
        loop {
            self.cur.skip_separators();
            let Some(c) = self.cur.peek() else { return Ok(self.runs) };
            match c {
                b'!' => {
                    self.cur.bump();
                    self.pen = Some(self.cur.point()?);
                }
                b'(' => {
                    self.cur.bump();
                    self.group()?;
                }
                _ => return Err(self.cur.unexpected()),
            }
        }
    }

    fn group(&mut self) -> Result<(), ParseError> {
        let from = self.pen.ok_or_else(|| self.cur.error(ParseErrorKind::MissingMoveTo))?;
        let ctrl1 = self.cur.point()?;
        let ctrl2 = self.cur.point()?;
        let to = self.cur.point()?;
        let approx = self.approximation()?;
        self.total = self.total.saturating_add(approx.len()).saturating_add(1);
        check_count(&self.cur, self.total)?;
        self.runs.push(CubicRun { cubic: Segment::Cubic { from, ctrl1, ctrl2, to }, approx });
        self.pen = Some(to);
        Ok(())
    }

    /// Reads the `q`/`Q` list up to and including the closing parenthesis.
    fn approximation(&mut self) -> Result<Vec<Segment>, ParseError> {
        let mut out = Vec::new();
        let mut pen: Option<Point> = None;
        let mut ctrl: Option<Point> = None;
        loop {
            self.cur.skip_separators();
            match self.cur.peek() {
                Some(b')') => {
                    self.cur.bump();
                    return Ok(out);
                }
                Some(b'q' | b'p') => {
                    self.cur.bump();
                    // `q` and `p` (Animate writes either, seemingly a corner/smooth flag) introduce one or more
                    // on-curve points.
                    let first = self.cur.point()?;
                    approx_point(&mut out, &mut pen, &mut ctrl, first);
                    loop {
                        self.cur.skip_separators();
                        if !self.cur.at_number() {
                            break;
                        }
                        let next = self.cur.point()?;
                        approx_point(&mut out, &mut pen, &mut ctrl, next);
                        check_count(&self.cur, out.len())?;
                    }
                }
                Some(b'Q') => {
                    self.cur.bump();
                    ctrl = Some(self.cur.point()?);
                }
                _ => return Err(self.cur.unexpected()),
            }
        }
    }
}

fn approx_point(out: &mut Vec<Segment>, pen: &mut Option<Point>, ctrl: &mut Option<Point>, to: Point) {
    if let Some(from) = *pen {
        out.push(ctrl.take().map_or(Segment::Line { from, to }, |ctrl| Segment::Quad { from, ctrl, to }));
    }
    *pen = Some(to);
}

/// Parses a `cubics` attribute into cubic curves with the quadratic pieces they replace.
///
/// # Errors
/// A [`ParseError`] for malformed text, a curve before any `!`, or input beyond the size limits.
pub fn parse_cubics(text: &str) -> Result<Vec<CubicRun>, ParseError> {
    CubicParser { cur: Cursor::new(text, true)?, pen: None, runs: Vec::new(), total: 0 }.run()
}

/// Quadratic pieces approximating a cubic within [`APPROX_ACCURACY`] twips, first and last points exact.
#[must_use]
pub fn cubic_approximation(from: Point, ctrl1: Point, ctrl2: Point, to: Point) -> Vec<Segment> {
    let cubic = CubicBez::new(from, ctrl1, ctrl2, to);
    let mut quads: Vec<Segment> = cubic.to_quads(APPROX_ACCURACY).map(|(_, _, q)| Segment::Quad { from: q.p0, ctrl: q.p1, to: q.p2 }).collect();
    if let Some(Segment::Quad { from: first, .. }) = quads.first_mut() {
        *first = from;
    }
    if let Some(Segment::Quad { to: last, .. }) = quads.last_mut() {
        *last = to;
    }
    // Keep pieces joined exactly after the endpoint fix-ups above.
    let ends: Vec<Point> = quads.iter().map(Segment::end).collect();
    for (segment, prev_end) in quads.iter_mut().skip(1).zip(ends) {
        if let Segment::Quad { from, .. } = segment {
            *from = prev_end;
        }
    }
    quads
}

fn push_point(out: &mut String, p: Point) {
    out.push_str(&format_number(p.x));
    out.push(' ');
    out.push_str(&format_number(p.y));
}

fn push_quads(out: &mut String, segment: &Segment) {
    match *segment {
        Segment::Line { from, to } => {
            out.push('!');
            push_point(out, from);
            out.push('|');
            push_point(out, to);
        }
        Segment::Quad { from, ctrl, to } => {
            out.push('!');
            push_point(out, from);
            out.push('[');
            push_point(out, ctrl);
            out.push(' ');
            push_point(out, to);
        }
        Segment::Cubic { from, ctrl1, ctrl2, to } => {
            for piece in cubic_approximation(from, ctrl1, ctrl2, to) {
                push_quads(out, &piece);
            }
        }
    }
}

/// Encodes segments as an `edges` attribute in canonical decimal twips.
///
/// Every segment gets its own `!`, like Animate writes them. Cubic segments are written as their quadratic
/// approximation; see [`encode_cubic`] for the exact curve.
#[must_use]
pub fn encode_edges(segments: &[Segment]) -> String {
    let mut out = String::new();
    for segment in segments {
        push_quads(&mut out, segment);
    }
    out
}

/// Encodes a cubic segment as a `cubics` attribute, with the same quadratic approximation [`encode_edges`] writes
/// so the two match when read back. Returns `None` for lines and quadratic curves.
#[must_use]
pub fn encode_cubic(segment: &Segment) -> Option<String> {
    let Segment::Cubic { from, ctrl1, ctrl2, to } = *segment else { return None };
    let mut out = String::from("!");
    push_point(&mut out, from);
    out.push_str("(;");
    for (i, p) in [ctrl1, ctrl2, to].into_iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let _ = write!(out, "{},{}", format_number(p.x), format_number(p.y));
    }
    let approx = cubic_approximation(from, ctrl1, ctrl2, to);
    if let Some(first) = approx.first() {
        out.push('q');
        push_point(&mut out, first.start());
    }
    for piece in &approx {
        if let Segment::Quad { ctrl, to, .. } = *piece {
            out.push('Q');
            push_point(&mut out, ctrl);
            out.push('q');
            push_point(&mut out, to);
        }
    }
    out.push_str(");");
    Some(out)
}

/// Rounds every coordinate to the 1/256 twip grid files store, the exact value [`encode_edges`] would write.
#[must_use]
pub fn quantize_segment(segment: Segment) -> Segment {
    segment.map_points(|p| Point::new(quantize(p.x), quantize(p.y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn parses_animate_style_edges() {
        let segs = parse_edges("!848 67S1[835 165 1052 184!1052 184|1175 174").unwrap();
        assert_eq!(
            segs,
            vec![
                Segment::Quad { from: p(848.0, 67.0), ctrl: p(835.0, 165.0), to: p(1052.0, 184.0) },
                Segment::Line { from: p(1052.0, 184.0), to: p(1175.0, 174.0) },
            ]
        );
    }

    #[test]
    fn slash_is_a_line_and_hex_numbers_work() {
        let segs = parse_edges("!127 141/73 14!73 14[#27.9 -20.5625 10 -45").unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs.first(), Some(&Segment::Line { from: p(127.0, 141.0), to: p(73.0, 14.0) }));
        assert_eq!(segs.get(1), Some(&Segment::Quad { from: p(73.0, 14.0), ctrl: p(39.5625, -20.5625), to: p(10.0, -45.0) }));
    }

    #[test]
    fn tolerates_newlines_inside_the_text() {
        let a = parse_edges("\r\n!2742 9112[2814 9182 2838 9185").unwrap();
        let b = parse_edges("!2742\r\n 9112[2814 9182 2838\n 9185").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn implicit_continuation_and_repetition() {
        let a = parse_edges("!0 0|10 0|10 10|0 10|0 0").unwrap();
        let b = parse_edges("!0 0|10 0 10 10 0 10 0 0").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 4);
    }

    #[test]
    fn rejects_malformed_text() {
        assert!(matches!(parse_edges("|1 2"), Err(ParseError { kind: ParseErrorKind::MissingMoveTo, .. })));
        assert!(matches!(parse_edges("!1"), Err(ParseError { kind: ParseErrorKind::UnexpectedEnd, .. })));
        assert!(matches!(parse_edges("!1 2 x"), Err(ParseError { kind: ParseErrorKind::UnexpectedChar('x'), .. })));
        assert!(matches!(parse_edges("!1 2 ["), Err(ParseError { kind: ParseErrorKind::UnexpectedEnd, .. })));
        assert!(matches!(parse_edges("!1 2|5"), Err(ParseError { kind: ParseErrorKind::UnexpectedEnd, .. })));
        assert!(matches!(parse_edges("5 5"), Err(ParseError { kind: ParseErrorKind::UnexpectedChar('5'), .. })));
        assert!(parse_edges("!1 2|ü").is_err());
        assert_eq!(parse_edges("").unwrap(), vec![]);
        assert_eq!(parse_edges("  \r\n ").unwrap(), vec![]);
    }

    #[test]
    fn hostile_sizes_are_refused() {
        let many = format!("!0 0{}", "|1 1".repeat(MAX_SEGMENTS + 1));
        assert!(matches!(parse_edges(&many), Err(ParseError { kind: ParseErrorKind::TooManySegments(_), .. })));
        let long = " ".repeat(MAX_TEXT_BYTES + 1);
        assert!(matches!(parse_edges(&long), Err(ParseError { kind: ParseErrorKind::TooLong(_), .. })));
        assert!(matches!(parse_cubics(&long), Err(ParseError { kind: ParseErrorKind::TooLong(_), .. })));
        // Absurd magnitudes and number lengths.
        assert!(parse_edges("!99999999999999 0|1 1").is_err());
        assert!(parse_edges(&format!("!{} 0|1 1", "9".repeat(5000))).is_err());
        // Deeply nested garbage is just garbage.
        assert!(parse_cubics(&"(".repeat(100_000)).is_err());
    }

    #[test]
    fn encode_is_canonical_and_round_trips() {
        let text = "!848 67[835 165 1052 184!1052 184|1175 174!5 5[#27.9 -20.5625 10 -45";
        let segs = parse_edges(text).unwrap();
        let encoded = encode_edges(&segs);
        assert_eq!(encoded, "!848 67[835 165 1052 184!1052 184|1175 174!5 5[39.5625 -20.5625 10 -45");
        assert_eq!(parse_edges(&encoded).unwrap(), segs);
    }

    #[test]
    fn parses_the_cubics_syntax_of_a_line_and_a_curve() {
        let runs = parse_cubics("!848 67(;829,206 1302,223 1177,116q848 67Q835 165q1052 184Q1134 190q1175 174Q1220 153q1177 116);").unwrap();
        assert_eq!(runs.len(), 1);
        let run = runs.first().unwrap();
        assert_eq!(run.cubic, Segment::Cubic { from: p(848.0, 67.0), ctrl1: p(829.0, 206.0), ctrl2: p(1302.0, 223.0), to: p(1177.0, 116.0) });
        assert_eq!(run.approx.len(), 3);
        assert_eq!(run.approx.get(1), Some(&Segment::Quad { from: p(1052.0, 184.0), ctrl: p(1134.0, 190.0), to: p(1175.0, 174.0) }));

        let line = parse_cubics("!3560 8615(;3560,8615 3640,8710 3640,8710q3560 8615 3640 8710);").unwrap();
        assert_eq!(line.first().unwrap().approx, vec![Segment::Line { from: p(3560.0, 8615.0), to: p(3640.0, 8710.0) }]);
    }

    #[test]
    fn p_and_q_both_mark_on_curve_points() {
        let quad = parse_cubics("!148 204(;151,172 157,133 168,94q148 204Q151 149p168 94);").unwrap();
        assert_eq!(quad.first().unwrap().approx, vec![Segment::Quad { from: p(148.0, 204.0), ctrl: p(151.0, 149.0), to: p(168.0, 94.0) }]);
        let line = parse_cubics("!208 1001(;208,1001 0,37 0,37p208 1001q0 37);").unwrap();
        assert_eq!(line.first().unwrap().approx, vec![Segment::Line { from: p(208.0, 1001.0), to: p(0.0, 37.0) }]);
        let mixed = parse_cubics("!-840 -637(;-688,-716 -489,-726 -300,-657q-840 -637 -756 -671Q-535 -743q-300 -657);").unwrap();
        assert_eq!(
            mixed.first().unwrap().approx,
            vec![
                Segment::Line { from: p(-840.0, -637.0), to: p(-756.0, -671.0) },
                Segment::Quad { from: p(-756.0, -671.0), ctrl: p(-535.0, -743.0), to: p(-300.0, -657.0) },
            ]
        );
    }

    #[test]
    fn cubics_errors() {
        assert!(matches!(parse_cubics("(;1,2 3,4 5,6);"), Err(ParseError { kind: ParseErrorKind::MissingMoveTo, .. })));
        assert!(parse_cubics("!0 0(;1,2 3,4 5,6").is_err());
        assert!(parse_cubics("!0 0(;1,2 3,4);").is_err());
        assert!(parse_cubics("!0 0 x").is_err());
    }

    #[test]
    fn cubic_encode_round_trips_and_matches_its_edges_text() {
        let cubic = Segment::Cubic { from: p(0.0, 0.0), ctrl1: p(0.0, 400.0), ctrl2: p(400.0, 400.0), to: p(400.0, 0.0) };
        let text = encode_cubic(&cubic).unwrap();
        let runs = parse_cubics(&text).unwrap();
        assert_eq!(runs.len(), 1);
        let run = runs.first().unwrap();
        assert_eq!(run.cubic, cubic);
        assert!(run.approx.len() > 1);
        // The edges attribute carries exactly the same quadratic pieces.
        let edges = parse_edges(&encode_edges(&[cubic])).unwrap();
        assert_eq!(edges.len(), run.approx.len());
        for (a, b) in edges.iter().zip(&run.approx) {
            assert!(a.matches(b, 1e-9));
        }
        assert_eq!(encode_cubic(&Segment::Line { from: p(0.0, 0.0), to: p(1.0, 1.0) }), None);
    }

    #[test]
    fn approximation_is_close_and_joined() {
        let (a, b, c, d) = (p(0.0, 0.0), p(0.0, 1000.0), p(1000.0, 1000.0), p(1000.0, 0.0));
        let quads = cubic_approximation(a, b, c, d);
        assert!(quads.len() >= 2);
        assert_eq!(quads.first().unwrap().start(), a);
        assert_eq!(quads.last().unwrap().end(), d);
        for pair in quads.windows(2) {
            assert_eq!(pair.first().unwrap().end(), pair.get(1).unwrap().start());
        }
    }
}
