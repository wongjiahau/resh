/// A byte range `start..end` into the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// A zero-width span at `offset`.
    pub fn empty(offset: usize) -> Self {
        Span::new(offset, offset)
    }

    pub fn to(self, other: Span) -> Span {
        Span::new(self.start, other.end)
    }
}

/// A 1-based line and column; the column counts characters, not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineColumn {
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for LineColumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Where byte `offset` of `source` sits as a line and column. An offset past
/// the end, or inside a character, is clamped back to a character boundary.
pub fn line_column(source: &str, offset: usize) -> LineColumn {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    LineColumn {
        line: before.matches('\n').count() + 1,
        column: before[line_start..].chars().count() + 1,
    }
}
