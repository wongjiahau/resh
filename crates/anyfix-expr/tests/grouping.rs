//! Every row of the grouping table, asserted on the abstract view.

mod common;

use anyfix_expr::{NamePart, NodeKind};
use common::{nodes, tree};

#[test]
fn dot_is_left_associative_over_runs() {
    assert_eq!(tree("a b . c d . e f"), "(`_ e f` (`_ c d` `a b`))");
}

#[test]
fn colon_is_right_associative() {
    assert_eq!(tree("a : b : c"), "(`a _` (`b _` c))");
}

#[test]
fn dot_binds_tighter_than_colon() {
    assert_eq!(tree("k : x . f"), "(`k _` (`_ f` x))");
}

#[test]
fn colon_inside_braces() {
    assert_eq!(
        tree("search { max depth : 3 }"),
        "(`search _` Braces[(`max depth _` 3)])"
    );
}

#[test]
fn minus_five_is_a_prefix_call() {
    assert_eq!(tree("-5"), "(`- _` 5)");
    // The same tree as `- 5`.
    assert_eq!(tree("-5"), tree("- 5"));
}

#[test]
fn switching_character_kind_splits_words() {
    assert_eq!(tree("x+1"), "(`_ + _` x 1)");
    assert_eq!(tree("a->b"), "(`_ -> _` a b)");
    assert_eq!(tree("null?"), "(`_ ?` null)");
}

#[test]
fn call_with_parens() {
    assert_eq!(tree("f(x)"), "(`f _` x)");
}

#[test]
fn dot_after_number_without_digit_is_an_operator() {
    let nodes = nodes("1.to");
    let NodeKind::Call(call) = &nodes[0].kind else {
        panic!("expected a call, got {nodes:?}")
    };
    assert_eq!(call.name_text(), "_ to");
    assert_eq!(call.arguments[0].kind, NodeKind::Number("1".into()));
}

#[test]
fn dotted_digits_are_one_number() {
    assert_eq!(nodes("1.2.3")[0].kind, NodeKind::Number("1.2.3".into()));
}

#[test]
fn dot_takes_the_whole_run_on_its_right() {
    assert_eq!(
        tree(r#"rg.search "TODO" in "src""#),
        r#"(`_ search _ in _` rg "TODO" "src")"#
    );
    assert_eq!(tree("fd.(option) argument"), "(`_ _ argument` fd option)");
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
    assert_eq!(tree("a\nb\n. c\n:\nd"), "(`_ c _` `a b` d)");
}

#[test]
fn semicolon_is_an_ordinary_word() {
    assert_eq!(tree("a; b"), "(`_ ; _` a b)");
}

#[test]
fn spans_cover_each_node() {
    let nodes = nodes("f (a, b) . g");
    assert_eq!(nodes[0].span, anyfix_expr::Span::new(0, 12));
    let NodeKind::Call(call) = &nodes[0].kind else {
        unreachable!()
    };
    assert_eq!(call.arguments[0].span, anyfix_expr::Span::new(0, 8));
    let NamePart::Word { span, .. } = &call.name[1] else {
        unreachable!()
    };
    assert_eq!(*span, anyfix_expr::Span::new(11, 12));
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

#[test]
fn dot_puts_the_value_at_an_underscore() {
    assert_eq!(tree("x . y _"), "(`y _` x)");
    assert_eq!(tree("x . y _ z"), "(`y _ z` x)");
    assert_eq!(tree("x . y (z)"), "(`_ y _` x z)");
}

#[test]
fn colon_puts_the_value_at_an_underscore() {
    assert_eq!(tree("f : x"), "(`f _` x)");
    assert_eq!(tree("_ f : x"), "(`_ f` x)");
    assert_eq!(tree("f _ g : x"), "(`f _ g` x)");
}

#[test]
fn underscore_inside_brackets_belongs_to_the_brackets() {
    assert_eq!(tree("x . f (_)"), "(`_ f _` x Parens[_])");
    assert_eq!(tree("x . f [_]"), "(`_ f _` x Brackets[_])");
    assert_eq!(tree("_"), "_");
}

#[test]
fn user_defined_infix_operators() {
    assert_eq!(tree("1 .+ 1 .+ 2"), "(`_ + _` (`_ + _` 1 1) 2)");
}

#[test]
fn dot_takes_the_underscore_before_colon_can() {
    assert_eq!(tree("x . y _ : z"), "(`y _ _` x z)");
}

#[test]
fn symbol_words_bind_looser_than_the_rest_of_the_run() {
    assert_eq!(tree("x < y < z"), "(`_ < _ < _` x y z)");
    assert_eq!(
        tree("my weight <= average (all weights) < max (all weights)"),
        "(`_ <= _ < _` `my weight` (`average _` `all weights`) (`max _` `all weights`))"
    );
    assert_eq!(tree("a + - b"), "(`_ + - _` a b)");
    assert_eq!(tree("x . _ + 1"), "(`_ + _` x 1)");
}

#[test]
fn words_name_the_call_and_other_items_fill_its_slots() {
    assert_eq!(tree("my weight"), "`my weight`");
    assert_eq!(tree("list file in (dir)"), "(`list file in _` dir)");
    assert_eq!(tree(r#"print "hello""#), r#"(`print _` "hello")"#);
    assert_eq!(tree("f (a b)"), "(`f _` `a b`)");
    assert_eq!(tree("f _"), "(`f _` _)");
}

#[test]
fn parens_around_one_element_only_group() {
    assert_eq!(tree("(x)"), "x");
    assert_eq!(tree("((a b))"), "`a b`");
    assert_eq!(tree("(a,)"), "Parens[a]");
    assert_eq!(tree("(_)"), "Parens[_]");
}

#[test]
fn name_text_separates_words_with_spaces() {
    let nodes = nodes("x_ (y)");
    let NodeKind::Call(call) = &nodes[0].kind else {
        unreachable!()
    };
    // `x_` is one word, so it can't be mistaken for `x` then a slot.
    assert_eq!(call.name_text(), "x_ _");
}
