//! The fixtures parse with zero errors and round-trip byte for byte.

mod common;

use common::{nodes, show, tree};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn grammar_parses_as_one_rule_per_element() {
    let nodes = nodes(&fixture("grammar.anyfix"));
    assert_eq!(nodes.len(), 19);
    assert_eq!(show(&nodes[0]), "Colon(document, sequence)");
    assert_eq!(
        show(&nodes[2]),
        r#"Colon(expr, Run[dotted, Brackets[Run[":", expr]]])"#
    );
}

#[test]
fn rg_parses() {
    let nodes = nodes(&fixture("rg.anyfix"));
    assert_eq!(nodes.len(), 1);
    assert!(show(&nodes[0]).starts_with("Run[namespace, Parens[rg], Braces["));
}

#[test]
fn config_parses() {
    assert_eq!(
        tree(&fixture("config.anyfix")),
        r#"Run[search, Braces[Colon(Run[max, depth], 3), Run[include, hidden, file], Run[glob, Brackets["*.rs", "!target"]]]]"#
    );
}

#[test]
fn json_parses() {
    let nodes = nodes(&fixture("json.anyfix"));
    assert_eq!(nodes.len(), 1);
    let shown = show(&nodes[0]);
    assert!(shown.contains(r#"Colon("private", true)"#), "{shown}");
    assert!(shown.contains(r#"Colon("max", Run[-, 3])"#), "{shown}");
    assert!(shown.contains(r#"Colon("p50", 1.25)"#), "{shown}");
}
