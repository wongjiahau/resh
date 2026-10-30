//! Every row of the grouping table, asserted on the abstract view.

mod common;

use anyfix_expr::NodeKind;
use common::{nodes, tree};

#[test]
fn dot_is_left_associative_over_runs() {
    assert_eq!(
        tree("a b . c d . e f"),
        "Dot(Dot(Run[a, b], Run[c, d]), Run[e, f])"
    );
}

#[test]
fn colon_is_right_associative() {
    assert_eq!(tree("a : b : c"), "Colon(a, Colon(b, c))");
}

#[test]
fn dot_binds_tighter_than_colon() {
    assert_eq!(tree("k : x . f"), "Colon(k, Dot(x, f))");
}

#[test]
fn colon_inside_braces() {
    assert_eq!(
        tree("search { max depth : 3 }"),
        "Run[search, Braces[Colon(Run[max, depth], 3)]]"
    );
}

#[test]
fn minus_five_is_a_word_then_a_number() {
    let nodes = nodes("-5");
    let NodeKind::Run(items) = &nodes[0].kind else {
        panic!("expected a run, got {nodes:?}");
    };
    assert_eq!(items[0].kind, NodeKind::Word("-".into()));
    assert_eq!(items[1].kind, NodeKind::Number("5".into()));
    // The same tree as `- 5`.
    assert_eq!(tree("-5"), tree("- 5"));
}

#[test]
fn switching_character_kind_splits_words() {
    assert_eq!(tree("x+1"), "Run[x, +, 1]");
    assert_eq!(tree("a->b"), "Run[a, ->, b]");
    assert_eq!(tree("null?"), "Run[null, ?]");
}

#[test]
fn call_with_parens() {
    assert_eq!(tree("f(x)"), "Run[f, Parens[x]]");
}

#[test]
fn dot_after_number_without_digit_is_an_operator() {
    let nodes = nodes("1.to");
    assert_eq!(common::show(&nodes[0]), "Dot(1, to)");
    let NodeKind::Dot { left, right } = &nodes[0].kind else {
        unreachable!()
    };
    assert_eq!(left.kind, NodeKind::Number("1".into()));
    assert_eq!(right.kind, NodeKind::Word("to".into()));
}

#[test]
fn dotted_digits_are_one_number() {
    assert_eq!(nodes("1.2.3")[0].kind, NodeKind::Number("1.2.3".into()));
}

#[test]
fn dot_takes_the_whole_run_on_its_right() {
    assert_eq!(
        tree(r#"rg.search "TODO" in "src""#),
        r#"Dot(rg, Run[search, "TODO", in, "src"])"#
    );
    assert_eq!(
        tree("fd.(option) argument"),
        "Dot(fd, Run[Parens[option], argument])"
    );
}

#[test]
fn commas_separate_inside_parens() {
    assert_eq!(tree(r#"("rg", x)"#), r#"Parens["rg", x]"#);
}

#[test]
fn empty_brackets_are_empty_sequences() {
    assert_eq!(tree("()"), "Parens[]");
    assert_eq!(tree("[]"), "Brackets[]");
    assert_eq!(tree("{}"), "Braces[]");
    assert_eq!(tree(""), "");
}

#[test]
fn trailing_comma_is_allowed() {
    assert_eq!(tree("[a, b,]"), "Brackets[a, b]");
    assert_eq!(tree("a, b,"), "a, b");
}

#[test]
fn newlines_are_plain_whitespace() {
    assert_eq!(tree("a\nb\n. c\n:\nd"), "Colon(Dot(Run[a, b], c), d)");
}

#[test]
fn semicolon_is_an_ordinary_word() {
    assert_eq!(tree("a; b"), "Run[a, ;, b]");
}

#[test]
fn spans_cover_each_node() {
    let nodes = nodes("f (a, b) . g");
    assert_eq!(nodes[0].span, anyfix_expr::Span::new(0, 12));
    let NodeKind::Dot { left, right } = &nodes[0].kind else {
        unreachable!()
    };
    assert_eq!(left.span, anyfix_expr::Span::new(0, 8));
    assert_eq!(right.span, anyfix_expr::Span::new(11, 12));
}

#[test]
fn line_column_is_one_based_and_counts_characters() {
    let source = "λ x\n  y";
    assert_eq!(
        anyfix_expr::line_column(source, 0),
        anyfix_expr::LineColumn { line: 1, column: 1 }
    );
    assert_eq!(
        anyfix_expr::line_column(source, 3),
        anyfix_expr::LineColumn { line: 1, column: 3 }
    );
    let y = source.find('y').unwrap();
    assert_eq!(
        anyfix_expr::line_column(source, y),
        anyfix_expr::LineColumn { line: 2, column: 3 }
    );
}
