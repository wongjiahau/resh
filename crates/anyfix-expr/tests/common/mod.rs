#![allow(dead_code)]

use anyfix_expr::{Error, ErrorCode, Node, NodeKind, Parsed, parse};

/// Parses `source`, asserting the round trip is byte-identical, as every test input must be.
pub fn parse_round_trip(source: &str) -> Parsed {
    let parsed = parse(source);
    assert_eq!(
        parsed.syntax.to_source(),
        source,
        "round trip changed {source:?}"
    );
    parsed
}

/// The abstract view of a valid input.
pub fn nodes(source: &str) -> Vec<Node> {
    let parsed = parse_round_trip(source);
    assert_eq!(parsed.errors, Vec::<Error>::new(), "errors in {source:?}");
    parsed.syntax.nodes()
}

/// The abstract view of a valid input in the grouping table's notation.
pub fn tree(source: &str) -> String {
    nodes(source)
        .iter()
        .map(show)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The error codes and spans of an input.
pub fn errors(source: &str) -> Vec<(ErrorCode, usize, usize)> {
    parse_round_trip(source)
        .errors
        .iter()
        .map(|error| (error.code, error.span.start, error.span.end))
        .collect()
}

pub fn show(node: &Node) -> String {
    let list = |nodes: &[Node]| nodes.iter().map(show).collect::<Vec<_>>().join(", ");
    match &node.kind {
        NodeKind::Word(text) | NodeKind::Number(text) => text.clone(),
        NodeKind::String(value) => format!("{value:?}"),
        NodeKind::Parens(nodes) => format!("Parens[{}]", list(nodes)),
        NodeKind::Brackets(nodes) => format!("Brackets[{}]", list(nodes)),
        NodeKind::Braces(nodes) => format!("Braces[{}]", list(nodes)),
        NodeKind::Run(nodes) => format!("Run[{}]", list(nodes)),
        NodeKind::Underscore => "_".into(),
        NodeKind::Missing => "Missing".into(),
    }
}
