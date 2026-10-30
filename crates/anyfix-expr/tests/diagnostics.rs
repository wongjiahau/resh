//! One test per error code, checking its code and span, plus recovery.

mod common;

use anyfix_expr::{ErrorCode, MAX_DEPTH, Parsed};
use common::{errors, parse_round_trip, show};

fn messages(source: &str) -> Vec<String> {
    let Parsed { errors, .. } = parse_round_trip(source);
    errors.into_iter().map(|error| error.message).collect()
}

fn tree_despite_errors(source: &str) -> String {
    parse_round_trip(source)
        .syntax
        .nodes()
        .iter()
        .map(show)
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn unclosed_bracket_points_at_the_opener() {
    assert_eq!(errors("f (a"), [(ErrorCode::UnclosedBracket, 2, 3)]);
    assert_eq!(errors("{ [x }"), [(ErrorCode::UnclosedBracket, 2, 3)]);
    assert_eq!(tree_despite_errors("f (a"), "Run[f, Parens[a]]");
    assert!(messages("f (a")[0].contains("`)`"));
}

#[test]
fn mismatched_bracket_names_both_brackets() {
    assert_eq!(errors("(a]"), [(ErrorCode::MismatchedBracket, 2, 3)]);
    let message = &messages("(a]")[0];
    assert!(
        message.contains("`]`") && message.contains("`(`"),
        "{message}"
    );
    assert_eq!(tree_despite_errors("(a] b"), "Run[Parens[a], b]");
}

#[test]
fn unexpected_closer() {
    assert_eq!(errors("a)"), [(ErrorCode::UnexpectedCloser, 1, 2)]);
    assert_eq!(errors("}"), [(ErrorCode::UnexpectedCloser, 0, 1)]);
    assert_eq!(tree_despite_errors("a) b, ]"), "Run[a, b]");
}

#[test]
fn empty_operand_needs_a_run_on_both_sides() {
    assert_eq!(errors("a..b"), [(ErrorCode::EmptyOperand, 1, 2)]);
    assert_eq!(errors(". a"), [(ErrorCode::EmptyOperand, 0, 1)]);
    assert_eq!(errors("a :"), [(ErrorCode::EmptyOperand, 2, 3)]);
    assert_eq!(errors(": b"), [(ErrorCode::EmptyOperand, 0, 1)]);
    assert_eq!(errors("a::b"), [(ErrorCode::EmptyOperand, 1, 2)]);
    assert_eq!(errors("(a .)"), [(ErrorCode::EmptyOperand, 3, 4)]);
    assert_eq!(tree_despite_errors("a :"), "Run[a, Parens[Missing]]");
    assert_eq!(tree_despite_errors(". a"), "Run[Parens[Missing], a]");
}

#[test]
fn empty_element_but_trailing_comma_is_fine() {
    assert_eq!(errors("a,,b"), [(ErrorCode::EmptyElement, 2, 3)]);
    assert_eq!(errors("[, a]"), [(ErrorCode::EmptyElement, 1, 2)]);
    assert_eq!(errors(","), [(ErrorCode::EmptyElement, 0, 1)]);
    assert_eq!(errors("[a,]"), []);
    assert_eq!(tree_despite_errors("a,,b"), "a, Missing, b");
}

#[test]
fn unterminated_string() {
    assert_eq!(errors(r#""abc"#), [(ErrorCode::UnterminatedString, 0, 4)]);
    assert_eq!(errors("x \"a\nb"), [(ErrorCode::UnterminatedString, 2, 6)]);
    assert_eq!(errors(r#""abc\"#), [(ErrorCode::UnterminatedString, 0, 5)]);
}

#[test]
fn invalid_escape() {
    assert_eq!(errors(r#""\q""#), [(ErrorCode::InvalidEscape, 1, 3)]);
    assert_eq!(
        errors(r#""\u{110000}""#),
        [(ErrorCode::InvalidEscape, 1, 11)]
    );
    assert_eq!(tree_despite_errors(r#""a\qb""#), r#""a\\qb""#);
}

#[test]
fn too_deep_caps_nesting_instead_of_overflowing() {
    let source = "(".repeat(1000);
    let found = errors(&source);
    let deep: Vec<_> = found
        .iter()
        .filter(|error| error.0 == ErrorCode::TooDeep)
        .collect();
    assert_eq!(deep, [&(ErrorCode::TooDeep, MAX_DEPTH, MAX_DEPTH + 1)]);
    assert!(
        found
            .iter()
            .all(|e| matches!(e.0, ErrorCode::TooDeep | ErrorCode::UnclosedBracket))
    );

    let balanced = format!("{}x{}", "(".repeat(1000), ")".repeat(1000));
    assert_eq!(
        errors(&balanced),
        [(ErrorCode::TooDeep, MAX_DEPTH, MAX_DEPTH + 1)]
    );

    let deepest = format!("{}x{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert_eq!(errors(&deepest), []);
}

#[test]
fn recovery_reports_every_error() {
    let source = "f (a,, b] . : \"\\q\" c) ,\n{ x : \"open";
    let codes: Vec<_> = errors(source).iter().map(|error| error.0).collect();
    assert_eq!(
        codes,
        [
            ErrorCode::EmptyElement,
            ErrorCode::MismatchedBracket,
            ErrorCode::EmptyOperand,
            ErrorCode::InvalidEscape,
            ErrorCode::UnexpectedCloser,
            ErrorCode::UnclosedBracket,
            ErrorCode::UnterminatedString,
        ]
    );
}

#[test]
fn messages_are_one_line_with_what_was_found_and_expected() {
    let source = "f (a,, b] . : \"\\q\" c) ,\n{ x : \"open";
    for message in messages(source) {
        assert!(!message.contains('\n'), "{message}");
        assert!(message.starts_with("found "), "{message}");
        assert!(message.contains("; expected "), "{message}");
    }
}

#[test]
fn render_gives_file_line_column_and_a_caret() {
    let source = "a\nf (b]";
    let error = &parse_round_trip(source).errors[0];
    let rendered = error.render(source, "x.resh");
    let mut lines = rendered.lines();
    assert!(lines.next().unwrap().starts_with("x.resh:2:5: found `]`"));
    assert_eq!(lines.next(), Some("  f (b]"));
    assert_eq!(lines.next(), Some("      ^"));
}

#[test]
fn extra_underscore_in_a_receiving_run() {
    assert_eq!(errors("x . f _ _"), [(ErrorCode::ExtraUnderscore, 8, 9)]);
    assert_eq!(errors("_ f _ : x"), [(ErrorCode::ExtraUnderscore, 4, 5)]);
    assert_eq!(errors("x . f _ (_)"), []);
    // Not receiving a value, so `_` is an ordinary word here.
    assert_eq!(errors("_ _"), []);
    assert_eq!(errors("_ _ . f"), []);
}
