//! The abstract view consumers read, derived from the concrete tree.

use crate::span::Span;
use crate::syntax::{Expression, Group, GroupKind, Item, Run, Sequence, SyntaxTree, TokenKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Word(String),
    /// The number's source text; its meaning is the consumer's.
    Number(String),
    /// The decoded string value.
    String(String),
    Parens(Vec<Node>),
    Brackets(Vec<Node>),
    Braces(Vec<Node>),
    /// Two or more items side by side; a single item is never a `Run`.
    Run(Vec<Node>),
    /// `left . right`.
    Dot {
        left: Box<Node>,
        right: Box<Node>,
    },
    /// `left : right`.
    Colon {
        left: Box<Node>,
        right: Box<Node>,
    },
    /// Where an operand or element is missing. Appears only when there are errors.
    Missing,
}

impl SyntaxTree {
    /// The document's top-level sequence as abstract nodes, with spans.
    pub fn nodes(&self) -> Vec<Node> {
        sequence_nodes(&self.sequence)
    }
}

fn sequence_nodes(sequence: &Sequence) -> Vec<Node> {
    sequence
        .elements
        .iter()
        .filter(|element| !holds_only_skipped(&element.expression))
        .map(|element| expression_node(&element.expression))
        .collect()
}

/// An element made only of stray closers adds nothing to the abstract view.
fn holds_only_skipped(expression: &Expression) -> bool {
    matches!(expression, Expression::Run(run) if !run.items.is_empty() && run.is_empty())
}

fn expression_node(expression: &Expression) -> Node {
    match expression {
        Expression::Run(run) => run_node(run),
        Expression::Dot(binary) | Expression::Colon(binary) => {
            let left = Box::new(expression_node(&binary.left));
            let right = Box::new(expression_node(&binary.right));
            let span = left.span.to(right.span);
            let kind = match expression {
                Expression::Dot(_) => NodeKind::Dot { left, right },
                _ => NodeKind::Colon { left, right },
            };
            Node { kind, span }
        }
    }
}

fn run_node(run: &Run) -> Node {
    let mut items: Vec<Node> = run.items.iter().filter_map(item_node).collect();
    match items.len() {
        0 => Node {
            kind: NodeKind::Missing,
            span: Span::empty(run.position),
        },
        1 => items.remove(0),
        _ => Node {
            span: items[0].span.to(items[items.len() - 1].span),
            kind: NodeKind::Run(items),
        },
    }
}

fn item_node(item: &Item) -> Option<Node> {
    match item {
        Item::Skipped(_) => None,
        Item::Atom(token) => {
            let kind = match &token.kind {
                TokenKind::Word => NodeKind::Word(token.text.clone()),
                TokenKind::Number => NodeKind::Number(token.text.clone()),
                TokenKind::String { value } => NodeKind::String(value.clone()),
                other => unreachable!("an atom is a word, number or string, not {other:?}"),
            };
            Some(Node {
                kind,
                span: token.span,
            })
        }
        Item::Group(group) => Some(group_node(group)),
    }
}

fn group_node(group: &Group) -> Node {
    let children = sequence_nodes(&group.sequence);
    let end = match &group.close {
        Some(close) => close.span.end,
        None => children
            .last()
            .map_or(group.open.span.end, |last| last.span.end),
    };
    let kind = match group.kind {
        GroupKind::Parens => NodeKind::Parens(children),
        GroupKind::Brackets => NodeKind::Brackets(children),
        GroupKind::Braces => NodeKind::Braces(children),
    };
    Node {
        kind,
        span: Span::new(group.open.span.start, end),
    }
}
