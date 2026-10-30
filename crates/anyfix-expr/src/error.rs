use crate::span::{LineColumn, Span, line_column};

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

impl Error {
    /// Where the error starts in `source`, as a line and column.
    pub fn position(&self, source: &str) -> LineColumn {
        line_column(source, self.span.start)
    }

    /// `file:line:column: message`, then the source line with `^` under the span.
    pub fn render(&self, source: &str, file: &str) -> String {
        let at = self.position(source);
        let line = source.lines().nth(at.line - 1).unwrap_or("");
        let width = source
            .get(self.span.start..self.span.end)
            .map_or(1, |text| text.lines().next().unwrap_or("").chars().count())
            .max(1);
        format!(
            "{file}:{at}: {}\n  {line}\n  {}{}",
            self.message,
            " ".repeat(at.column - 1),
            "^".repeat(width)
        )
    }
}
