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
    /// The bare word `_`, left over when no `.` or `:` consumed it. It is a
    /// placeholder in the scope of the nearest enclosing brackets.
    Underscore,
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

/// `.` and `:` do not survive into the abstract view. Each hands its left
/// value to the run on its other side (see [`apply`]):
///
/// - `x . y (z)` is `(x) y (z)` and `x . y _` is `y (x)`;
/// - `f : x` is `f (x)` and `_ f : x` is `(x) f`.
fn expression_node(expression: &Expression) -> Node {
    match expression {
        Expression::Run(run) => run_node(run),
        Expression::Dot(binary) => {
            let value = expression_node(&binary.left);
            let receiver = expression_node(&binary.right);
            apply(value, receiver, Side::Front)
        }
        Expression::Colon(binary) => {
            let receiver = expression_node(&binary.left);
            let value = expression_node(&binary.right);
            apply(value, receiver, Side::Back)
        }
    }
}

enum Side {
    Front,
    Back,
}

/// Puts `value` in a slot of `receiver`: in place of its first top-level `_`,
/// else at the `side`. A `_` inside brackets belongs to those brackets.
fn apply(value: Node, receiver: Node, side: Side) -> Node {
    let span = cover(value.span, receiver.span);
    let mut items = match receiver.kind {
        NodeKind::Run(items) => items,
        NodeKind::Missing => Vec::new(),
        _ => vec![receiver],
    };
    let slot = Node {
        span: value.span,
        kind: NodeKind::Parens(vec![value]),
    };
    match items
        .iter()
        .position(|item| item.kind == NodeKind::Underscore)
    {
        Some(index) => items[index] = slot,
        None => match side {
            Side::Front => items.insert(0, slot),
            Side::Back => items.push(slot),
        },
    }
    if items.len() == 1 {
        let only = items.remove(0);
        return Node {
            kind: only.kind,
            span,
        };
    }
    Node {
        kind: NodeKind::Run(items),
        span,
    }
}

fn cover(a: Span, b: Span) -> Span {
    Span::new(a.start.min(b.start), a.end.max(b.end))
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
                TokenKind::Word if token.text == "_" => NodeKind::Underscore,
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
