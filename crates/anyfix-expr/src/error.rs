use crate::span::Span;

/// A stable code for each kind of syntax error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// An opening bracket with no closer, e.g. `f (a`.
    UnclosedBracket,
    /// A closer of the wrong kind, e.g. `(a]`.
    MismatchedBracket,
    /// A closer with no open bracket, e.g. `a)`.
    UnexpectedCloser,
    /// An operator without a run on one side, e.g. `a..b`, `. a`, `a :`.
    EmptyOperand,
    /// Nothing between two commas, or before the first one, e.g. `a,,b`, `[, a]`.
    EmptyElement,
    /// A string with no closing `"`.
    UnterminatedString,
    /// An unknown escape, or a bad `\u{…}`, e.g. `"\q"`, `"\u{110000}"`.
    InvalidEscape,
    /// Brackets nested deeper than [`MAX_DEPTH`](crate::MAX_DEPTH).
    TooDeep,
}

/// One syntax error: what was found, what was expected, and the fix, in one line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub span: Span,
    pub message: String,
}

impl Error {
    pub(crate) fn new(code: ErrorCode, span: Span, message: impl Into<String>) -> Self {
        Error {
            code,
            span,
            message: message.into(),
        }
    }
}
