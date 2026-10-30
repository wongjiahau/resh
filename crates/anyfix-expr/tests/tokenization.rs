mod common;

use anyfix_expr::{ErrorCode, Expression, Item, TokenKind, TriviaKind, parse};
use common::{errors, parse_round_trip, tree};

/// An atom token of the concrete tree.
#[derive(Debug, PartialEq)]
enum Atom {
    Word(String),
    Number(String),
    String(String),
}

/// The atoms of a valid single-run input, in order.
fn atoms(source: &str) -> Vec<Atom> {
    let parsed = parse_round_trip(source);
    assert!(parsed.errors.is_empty(), "errors in {source:?}");
    let [element] = &parsed.syntax.sequence.elements[..] else {
        panic!("expected one element in {source:?}")
    };
    let Expression::Run(run) = &element.expression else {
        panic!("expected a run in {source:?}")
    };
    run.items
        .iter()
        .map(|item| match item {
            Item::Atom(token) => match &token.kind {
                TokenKind::Word => Atom::Word(token.text.clone()),
                TokenKind::Number => Atom::Number(token.text.clone()),
                TokenKind::String { value } => Atom::String(value.clone()),
                other => panic!("an atom is not {other:?}"),
            },
            other => panic!("expected only atoms in {source:?}, got {other:?}"),
        })
        .collect()
}

fn word(text: &str) -> Atom {
    Atom::Word(text.into())
}

fn number(text: &str) -> Atom {
    Atom::Number(text.into())
}

fn string(source: &str) -> String {
    match atoms(source).as_slice() {
        [Atom::String(value)] => value.clone(),
        other => panic!("expected one string in {source:?}, got {other:?}"),
    }
}

#[test]
fn a_word_is_all_name_or_all_symbol_characters() {
    assert_eq!(atoms("flat map"), [word("flat"), word("map")]);
    assert_eq!(atoms("snake_case2"), [word("snake_case2")]);
    assert_eq!(atoms("_"), [word("_")]);
    assert_eq!(atoms("a->b"), [word("a"), word("->"), word("b")]);
    assert_eq!(atoms("<=>!"), [word("<=>!")]);
    assert_eq!(
        atoms("kebab-case"),
        [word("kebab"), word("-"), word("case")]
    );
    assert_eq!(atoms("a;b"), [word("a"), word(";"), word("b")]);
}

#[test]
fn special_characters_never_appear_inside_a_word() {
    assert_eq!(tree("a,b"), "a, b");
    assert_eq!(tree("a:b"), "(`a _` b)");
    assert_eq!(tree("+.-"), "(`_ -` +)");
    assert_eq!(tree("f(x)[y]{z}"), "(`f _ _ _` x Brackets[y] Braces[z])");
    assert_eq!(atoms(r#"a"b""#), [word("a"), Atom::String("b".into())]);
}

#[test]
fn letter_case_does_not_change_tokens() {
    assert_eq!(atoms("Flat MAP"), [word("Flat"), word("MAP")]);
    assert_eq!(string(r#""\N\T\R\U{4A}""#), "\n\t\rJ");
    assert_eq!(string(r#""\u{4a}\u{4A}""#), "JJ");
}

#[test]
fn unicode_names_and_symbols() {
    assert_eq!(atoms("λ"), [word("λ")]);
    assert_eq!(atoms("→"), [word("→")]);
    assert_eq!(atoms("λ→x"), [word("λ"), word("→"), word("x")]);
    assert_eq!(atoms("日本語 名前"), [word("日本語"), word("名前")]);
    // A name continues with any alphanumeric, but a non-ASCII digit alone is a symbol.
    assert_eq!(atoms("x٣"), [word("x٣")]);
    assert_eq!(atoms("٣"), [word("٣")]);
}

#[test]
fn numbers_start_with_an_ascii_digit_and_keep_their_text() {
    assert_eq!(atoms("42"), [number("42")]);
    assert_eq!(atoms("1.5"), [number("1.5")]);
    assert_eq!(atoms("1.2.3"), [number("1.2.3")]);
    assert_eq!(atoms("0xFF"), [number("0xFF")]);
    assert_eq!(atoms("1_000"), [number("1_000")]);
    assert_eq!(atoms("2.5e3"), [number("2.5e3")]);
    assert_eq!(atoms("1mb"), [number("1mb")]);
}

#[test]
fn dot_in_a_number_needs_a_digit_after_it() {
    assert_eq!(tree("1.to"), "(`_ to` 1)");
    assert_eq!(tree("1. 2"), "(`_ _` 1 2)");
    assert_eq!(tree("x.1"), "(`_ _` x 1)");
    assert_eq!(tree("1.5.to"), "(`_ to` 1.5)");
}

#[test]
fn numbers_have_no_sign_and_exponents_split_at_it() {
    assert_eq!(atoms("-5"), [word("-"), number("5")]);
    assert_eq!(atoms("1e-5"), [number("1e"), word("-"), number("5")]);
}

#[test]
fn hash_starts_a_comment_even_mid_token() {
    assert_eq!(tree("a#b"), "a");
    assert_eq!(tree("a # b c\nd"), "`a d`");
    assert_eq!(tree("+#-\n-"), "`+ -`");
    assert_eq!(tree("12#3"), "12");
    assert_eq!(tree("#!/usr/bin/env resh\nx"), "x");
    assert_eq!(string(r##""a#b""##), "a#b");
}

#[test]
fn comments_and_whitespace_are_trivia() {
    let parsed = parse("a # note\n  b");
    let anyfix_expr::Expression::Run(run) = &parsed.syntax.sequence.elements[0].expression else {
        panic!("expected a run")
    };
    let [anyfix_expr::Item::Atom(a), anyfix_expr::Item::Atom(b)] = &run.items[..] else {
        panic!("expected two atoms")
    };
    let trailing: Vec<_> = a
        .trailing
        .iter()
        .map(|t| (t.kind, t.text.as_str()))
        .collect();
    assert_eq!(
        trailing,
        [
            (TriviaKind::Whitespace, " "),
            (TriviaKind::Comment, "# note")
        ]
    );
    let leading: Vec<_> = b
        .leading
        .iter()
        .map(|t| (t.kind, t.text.as_str()))
        .collect();
    assert_eq!(leading, [(TriviaKind::Whitespace, "\n  ")]);
    assert_eq!(parsed.syntax.end.kind, TokenKind::End);
}

#[test]
fn every_string_escape() {
    assert_eq!(string(r#""\"""#), "\"");
    assert_eq!(string(r#""\\""#), "\\");
    assert_eq!(string(r#""\n""#), "\n");
    assert_eq!(string(r#""\t""#), "\t");
    assert_eq!(string(r#""\r""#), "\r");
    assert_eq!(string(r#""\u{41}""#), "A");
    assert_eq!(string(r#""\u{3bb}""#), "λ");
    assert_eq!(string(r#""plain""#), "plain");
    assert_eq!(string(r#""""#), "");
}

#[test]
fn strings_may_contain_newlines_and_special_characters() {
    assert_eq!(string("\"a\nb\""), "a\nb");
    assert_eq!(string(r#""(,:.)[]{}""#), "(,:.)[]{}");
}

#[test]
fn unicode_escape_takes_one_to_six_hex_digits() {
    assert_eq!(string(r#""\u{0}""#), "\0");
    assert_eq!(string(r#""\u{10FFFF}""#), "\u{10FFFF}");
    assert_eq!(string(r#""\u{00004A}""#), "J");
    let invalid = |source: &str| {
        assert_eq!(
            errors(source).iter().map(|e| e.0).collect::<Vec<_>>(),
            [ErrorCode::InvalidEscape],
            "{source}"
        )
    };
    invalid(r#""\u{}""#);
    invalid(r#""\u{0000041}""#);
    invalid(r#""\u{110000}""#);
    invalid(r#""\u{D800}""#);
    invalid(r#""\u41""#);
    invalid(r#""\u{41""#);
    invalid(r#""\u{4G}""#);
}
