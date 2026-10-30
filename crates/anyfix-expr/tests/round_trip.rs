//! `to_source()` returns the input byte for byte, valid or not. Every other
//! test parses through `common::parse_round_trip`, so this file adds the
//! trivia-heavy and malformed inputs no other test covers.

mod common;

use common::parse_round_trip;

#[test]
fn trivia_round_trips() {
    for source in [
        "",
        " ",
        "\n\n",
        "# only a comment",
        "#!/usr/bin/env resh\n",
        "a # one\n# two\n\n  b   # three\n",
        "\t a\t,\r\n b ,\n",
        "  ( a , b )  . c : d  ",
        "x\u{a0}y\u{2003}z",
        "λ → ∀ x",
    ] {
        parse_round_trip(source);
    }
}

#[test]
fn malformed_input_round_trips() {
    for source in [
        "(",
        ")",
        "(]",
        "[(a]",
        "a ) ] } b",
        "a..b::c",
        ",,,",
        ". : .",
        "\"open",
        "\"\\",
        "\"\\u{",
        "\"\\u{110000\" x",
        "f (a,, b] . : \"\\q\" c) ,\n{ x : \"open",
        &"(".repeat(1000),
        &format!("{}x{}", "{".repeat(600), "}".repeat(600)),
    ] {
        parse_round_trip(source);
    }
}
