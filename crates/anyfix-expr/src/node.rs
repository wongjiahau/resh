//! The abstract view consumers read, derived from the concrete tree.

use crate::lexer::is_symbol_word;
use crate::span::Span;
use crate::syntax::{Expression, Group, GroupKind, Item, Run, Sequence, SyntaxTree, TokenKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// The number's source text; its meaning is the consumer's.
    Number(String),
    /// The decoded string value.
    String(String),
    /// A word, or a run of items side by side, read as a call.
    Call(Call),
    /// `()`, `(a, b)` or `(a,)`. Parens around one element with no comma only
    /// group, so `(x)` is `x`.
    Parens(Vec<Node>),
    Brackets(Vec<Node>),
    Braces(Vec<Node>),
    /// The bare word `_`, left over when no `.` or `:` consumed it. It is a
    /// placeholder in the scope of the nearest enclosing brackets, so `(_)`
    /// stays `Parens[_]`.
    Underscore,
    /// Where an operand or element is missing. Appears only when there are errors.
    Missing,
}

/// A run read as a call: its words form the name, and every other item fills
/// a slot. Symbol words bind looser than the rest, so the items between them
/// become one argument each: `my weight <= max (all)` is the call
/// `_ <= _` of `my weight` and `max _`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub name: Vec<NamePart>,
    /// One per `NamePart::Slot`, in order.
    pub arguments: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamePart {
    Word { text: String, span: Span },
    Slot,
}

impl Call {
    /// The name with `_` for each slot, parts joined by spaces:
    /// `list file in _`, `_ <= _ < _`. A lone `_` is always a slot, since a
    /// bare `_` is never a word, so the text is unambiguous.
    pub fn name_text(&self) -> String {
        self.name
            .iter()
            .map(|part| match part {
                NamePart::Word { text, .. } => text.as_str(),
                NamePart::Slot => "_",
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
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

/// A run's items before they become a node. `.` and `:` fill a slot at this
/// level, where a bare `_` is still a top-level item of the run.
struct Pieces {
    items: Vec<Piece>,
    span: Span,
}

enum Piece {
    Word { text: String, span: Span },
    Argument(Node),
    Underscore(Span),
}

impl Piece {
    fn span(&self) -> Span {
        match self {
            Piece::Word { span, .. } | Piece::Underscore(span) => *span,
            Piece::Argument(node) => node.span,
        }
    }

    fn is_symbol_word(&self) -> bool {
        matches!(self, Piece::Word { text, .. } if is_symbol_word(text))
    }
}

fn expression_node(expression: &Expression) -> Node {
    pieces_node(expression_pieces(expression))
}

/// `.` and `:` do not survive into the abstract view. Each hands its left
/// value to the run on its other side (see [`fill`]):
///
/// - `x . y (z)` is `(x) y (z)` and `x . y _` is `y (x)`;
/// - `f : x` is `f (x)` and `_ f : x` is `(x) f`.
fn expression_pieces(expression: &Expression) -> Pieces {
    match expression {
        Expression::Run(run) => run_pieces(run),
        Expression::Dot(binary) => {
            let value = expression_node(&binary.left);
            fill(value, expression_pieces(&binary.right), Side::Front)
        }
        Expression::Colon(binary) => {
            let value = expression_node(&binary.right);
            fill(value, expression_pieces(&binary.left), Side::Back)
        }
    }
}

enum Side {
    Front,
    Back,
}

/// Puts `value` in a slot of `receiver`: in place of its first top-level `_`,
/// else at the `side`. A `_` inside brackets belongs to those brackets.
fn fill(value: Node, receiver: Pieces, side: Side) -> Pieces {
    let span = cover(value.span, receiver.span);
    let mut items = receiver.items;
    match items
        .iter()
        .position(|item| matches!(item, Piece::Underscore(_)))
    {
        Some(index) => items[index] = Piece::Argument(value),
        None => match side {
            Side::Front => items.insert(0, Piece::Argument(value)),
            Side::Back => items.push(Piece::Argument(value)),
        },
    }
    Pieces { items, span }
}

fn cover(a: Span, b: Span) -> Span {
    Span::new(a.start.min(b.start), a.end.max(b.end))
}

fn run_pieces(run: &Run) -> Pieces {
    let items: Vec<Piece> = run.items.iter().filter_map(item_piece).collect();
    let span = match (items.first(), items.last()) {
        (Some(first), Some(last)) => first.span().to(last.span()),
        _ => Span::empty(run.position),
    };
    Pieces { items, span }
}

/// No items is `Missing` and a lone non-word item is itself. Otherwise the
/// items form a call. When symbol words are present, the items between them
/// are first gathered into one argument each.
fn pieces_node(pieces: Pieces) -> Node {
    let Pieces { mut items, span } = pieces;
    let kind = match items.len() {
        0 => NodeKind::Missing,
        1 if !matches!(items[0], Piece::Word { .. }) => return piece_argument(items.remove(0)),
        _ if items.iter().any(Piece::is_symbol_word) => NodeKind::Call(split_at_symbols(items)),
        _ => NodeKind::Call(call(items)),
    };
    Node { kind, span }
}

fn split_at_symbols(items: Vec<Piece>) -> Call {
    let mut pieces = Vec::new();
    let mut stretch = Vec::new();
    for item in items {
        if item.is_symbol_word() {
            pieces.extend(gather(std::mem::take(&mut stretch)));
            pieces.push(item);
        } else {
            stretch.push(item);
        }
    }
    pieces.extend(gather(stretch));
    call(pieces)
}

/// The items between two symbol words, as the one argument they become.
fn gather(stretch: Vec<Piece>) -> Option<Piece> {
    let span = stretch.first()?.span().to(stretch.last()?.span());
    Some(Piece::Argument(pieces_node(Pieces {
        items: stretch,
        span,
    })))
}

fn call(items: Vec<Piece>) -> Call {
    let (name, arguments): (Vec<NamePart>, Vec<Option<Node>>) = items
        .into_iter()
        .map(|item| match item {
            Piece::Word { text, span } => (NamePart::Word { text, span }, None),
            argument => (NamePart::Slot, Some(piece_argument(argument))),
        })
        .unzip();
    Call {
        name,
        arguments: arguments.into_iter().flatten().collect(),
    }
}

fn piece_argument(piece: Piece) -> Node {
    match piece {
        Piece::Argument(node) => node,
        Piece::Underscore(span) => Node {
            kind: NodeKind::Underscore,
            span,
        },
        Piece::Word { text, span } => Node {
            kind: NodeKind::Call(Call {
                name: vec![NamePart::Word { text, span }],
                arguments: Vec::new(),
            }),
            span,
        },
    }
}

fn item_piece(item: &Item) -> Option<Piece> {
    match item {
        Item::Skipped(_) => None,
        Item::Atom(token) => {
            let span = token.span;
            let piece = match &token.kind {
                TokenKind::Word if token.text == "_" => Piece::Underscore(span),
                TokenKind::Word => Piece::Word {
                    text: token.text.clone(),
                    span,
                },
                TokenKind::Number => Piece::Argument(Node {
                    kind: NodeKind::Number(token.text.clone()),
                    span,
                }),
                TokenKind::String { value } => Piece::Argument(Node {
                    kind: NodeKind::String(value.clone()),
                    span,
                }),
                other => unreachable!("an atom is a word, number or string, not {other:?}"),
            };
            Some(piece)
        }
        Item::Group(group) => Some(Piece::Argument(group_node(group))),
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
    if group.kind == GroupKind::Parens && only_groups(group, &children) {
        return children.into_iter().next().expect("one child");
    }
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

/// Parens around a single element with no comma only group it, except around
/// a bare `_`, whose scope they mark.
fn only_groups(group: &Group, children: &[Node]) -> bool {
    matches!(&group.sequence.elements[..], [element] if element.comma.is_none())
        && matches!(children, [child] if child.kind != NodeKind::Underscore)
}
