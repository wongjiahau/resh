use resh::Session;

#[test]
fn result_alias_equals_full_form() {
    let mut s = Session::new();
    s.run(
        r#"FUNCTION (a) RETURNS (Result (String)) { run ("true", []) },
FUNCTION (b) RETURNS (Result (String) Or (String)) { a },
FUNCTION (c) RETURNS (Result (String)) { b }"#,
    )
    .unwrap_or_else(|d| panic!("{d}"));
}

#[test]
fn alias_with_wrong_arity_is_an_error() {
    let mut s = Session::new();
    assert!(s.run("FUNCTION (a) RETURNS (Result) { 1 }").is_err());
    assert!(
        s.run("FUNCTION (b) RETURNS (Result (Int) (Int)) { 1 }")
            .is_err()
    );
}
