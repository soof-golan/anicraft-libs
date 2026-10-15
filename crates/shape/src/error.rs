//! Errors for the XFL edge text codecs and shape assembly.

/// Why an `edges` or `cubics` attribute could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseErrorKind {
    #[error("unexpected character {0:?}")]
    UnexpectedChar(char),
    #[error("unexpected end of text")]
    UnexpectedEnd,
    #[error("malformed number")]
    BadNumber,
    #[error("number is out of range")]
    NumberRange,
    #[error("a draw command came before any move command")]
    MissingMoveTo,
    #[error("text is longer than the limit of {0} bytes")]
    TooLong(usize),
    #[error("more than {0} segments")]
    TooManySegments(usize),
}

/// A parse failure with the byte offset where it was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("at byte {offset}: {kind}")]
pub struct ParseError {
    pub offset: usize,
    pub kind: ParseErrorKind,
}

/// Errors building an [`EdgeShape`](crate::EdgeShape) from XFL attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ShapeError {
    /// The `edges` attribute of the `index`-th `<Edge>` element is malformed.
    #[error("edge {index}, `edges` attribute: {source}")]
    Edges { index: usize, source: ParseError },
    /// The `cubics` attribute of the `index`-th `<Edge>` element is malformed.
    #[error("edge {index}, `cubics` attribute: {source}")]
    Cubics { index: usize, source: ParseError },
}
