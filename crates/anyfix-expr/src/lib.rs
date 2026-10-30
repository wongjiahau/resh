//! Anyfix expressions: a keyword-free notation that only describes grouping.
//! It gives nothing a meaning; that is the consumer's job. See `README.md`
//! for the specification.
//!
//! [`parse`] returns a lossless [`SyntaxTree`] (printing it back reproduces
//! the input byte for byte) and every [`Error`] found. [`SyntaxTree::nodes`]
//! derives the abstract [`Node`] view from it.

mod error;
mod lexer;
mod node;
mod parser;
mod span;
mod syntax;

pub use error::{Error, ErrorCode};
pub use node::{Node, NodeKind};
pub use parser::{MAX_DEPTH, Parsed, parse};
pub use span::{LineColumn, Span, line_column};
pub use syntax::{
    BinaryExpression, Element, Expression, Group, GroupKind, Item, Run, Sequence, SyntaxTree,
    Token, TokenKind, Trivia, TriviaKind,
};
