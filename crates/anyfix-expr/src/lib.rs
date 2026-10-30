//! Anyfix expressions: a keyword-free notation that only describes grouping.
//! It gives nothing a meaning; that is the consumer's job. See `README.md`
//! for the specification.
//!
//! ```
//! let parsed = anyfix_expr::parse("search { max depth : 3 }");
//! assert!(parsed.errors.is_empty());
//! let nodes = parsed.syntax.nodes();
//! let anyfix_expr::NodeKind::Call(call) = &nodes[0].kind else { unreachable!() };
//! assert_eq!(call.name_text(), "search _");
//! assert_eq!(parsed.syntax.to_source(), "search { max depth : 3 }");
//! ```
//!
//! [`parse`] returns a lossless [`SyntaxTree`] (printing it back reproduces
//! the input byte for byte) and every [`Error`] found. [`SyntaxTree::nodes`]
//! derives the abstract [`Node`] view from it, where every run is a [`Call`].

mod error;
mod lexer;
mod node;
mod parser;
mod span;
mod syntax;

pub use error::{Error, ErrorCode};
pub use node::{Call, NamePart, Node, NodeKind};
pub use parser::{MAX_DEPTH, Parsed, parse};
pub use span::{LineColumn, Span, line_column};
pub use syntax::{
    BinaryExpression, Element, Expression, Group, GroupKind, Item, Run, Sequence, SyntaxTree,
    Token, TokenKind, Trivia, TriviaKind,
};
