use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub span: Span,
    pub msg: String,
}

impl Diag {
    pub fn new(span: Span, msg: impl Into<String>) -> Self {
        Diag {
            span,
            msg: msg.into(),
        }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.msg)
    }
}

pub type Res<T> = Result<T, Diag>;
