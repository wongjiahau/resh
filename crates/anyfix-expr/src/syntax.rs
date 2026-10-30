//! The concrete, lossless tree: every token with its trivia, so printing it
//! back reproduces the source byte for byte.

use crate::span::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    Number,
    /// A string literal; `value` is its decoded text.
    String {
        value: String,
    },
    Comma,
    Colon,
    Dot,
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    /// The end of input. It carries the document's trailing trivia.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriviaKind {
    Whitespace,
    /// From `#` to the end of the line, not including the newline.
    Comment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub text: String,
    pub span: Span,
}

/// A token with its source text. Leading trivia is whatever comes before it,
/// back to the previous token's trailing trivia; trailing trivia is the
/// whitespace and comment after it on the same line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub span: Span,
    pub leading: Vec<Trivia>,
    pub trailing: Vec<Trivia>,
}

impl Token {
    fn write(&self, out: &mut String) {
        self.leading
            .iter()
            .for_each(|trivia| out.push_str(&trivia.text));
        out.push_str(&self.text);
        self.trailing
            .iter()
            .for_each(|trivia| out.push_str(&trivia.text));
    }
}

/// A whole document: its top-level sequence, then the end token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxTree {
    pub sequence: Sequence,
    pub end: Token,
}

impl SyntaxTree {
    /// The source text again, byte for byte.
    pub fn to_source(&self) -> String {
        let mut out = String::new();
        self.sequence.write(&mut out);
        self.end.write(&mut out);
        out
    }
}

/// Comma-separated elements, at the top level or inside a bracket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequence {
    pub elements: Vec<Element>,
}

impl Sequence {
    fn write(&self, out: &mut String) {
        self.elements.iter().for_each(|element| {
            element.expression.write(out);
            if let Some(comma) = &element.comma {
                comma.write(out);
            }
        });
    }
}

/// One element and the comma after it, if any. An element whose expression is
/// an empty run is an error (`a,,b`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub expression: Expression,
    pub comma: Option<Token>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expression {
    Run(Run),
    /// `left . right`, left-associative.
    Dot(Box<BinaryExpression>),
    /// `left : right`, right-associative.
    Colon(Box<BinaryExpression>),
}

impl Expression {
    fn write(&self, out: &mut String) {
        match self {
            Expression::Run(run) => run.items.iter().for_each(|item| item.write(out)),
            Expression::Dot(binary) | Expression::Colon(binary) => {
                binary.left.write(out);
                binary.operator.write(out);
                binary.right.write(out);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryExpression {
    pub left: Expression,
    pub operator: Token,
    pub right: Expression,
}

/// Items side by side. `position` is where the run starts; for a run with no
/// items (an error), it is where one was expected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub items: Vec<Item>,
    pub position: usize,
}

impl Run {
    /// Whether the run holds nothing but skipped tokens.
    pub fn is_empty(&self) -> bool {
        self.items
            .iter()
            .all(|item| matches!(item, Item::Skipped(_)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A word, number or string.
    Atom(Token),
    Group(Group),
    /// A token kept only for the round trip, after an error: a stray closer,
    /// or the contents of a too-deep bracket.
    Skipped(Token),
}

impl Item {
    fn write(&self, out: &mut String) {
        match self {
            Item::Atom(token) | Item::Skipped(token) => token.write(out),
            Item::Group(group) => {
                group.open.write(out);
                group.sequence.write(out);
                if let Some(close) = &group.close {
                    close.write(out);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    Parens,
    Brackets,
    Braces,
}

impl GroupKind {
    pub fn open(self) -> char {
        match self {
            GroupKind::Parens => '(',
            GroupKind::Brackets => '[',
            GroupKind::Braces => '{',
        }
    }

    pub fn close(self) -> char {
        match self {
            GroupKind::Parens => ')',
            GroupKind::Brackets => ']',
            GroupKind::Braces => '}',
        }
    }
}

/// A bracketed sequence. `close` is missing when the bracket was never closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub kind: GroupKind,
    pub open: Token,
    pub sequence: Sequence,
    pub close: Option<Token>,
}
