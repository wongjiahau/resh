use resh::{Session, Value};

fn run(src: &str) -> Vec<Value> {
    Session::new()
        .run(src)
        .unwrap_or_else(|d| panic!("unexpected error: {d}"))
}

fn run_err(src: &str) -> String {
    match Session::new().run(src) {
        Ok(v) => panic!("expected an error, got {v:?}"),
        Err(d) => d.msg,
    }
}

fn render(src: &str) -> String {
    let mut s = Session::new();
    let vs = s
        .run(src)
        .unwrap_or_else(|d| panic!("unexpected error: {d}"));
    s.render(vs.last().expect("a value"), false)
}

const SHAPES: &str = r#"
ENUM Shape {
    circle (radius: Float),
    (width: Float) by (height: Float),
    empty
}

FUNCTION describe (s: Shape): String {
    MATCH s {
        circle (r) => "circle",
        (w) by (h) => "rectangle",
        empty => "nothing"
    }
}
"#;

#[test]
fn anyfix_constructors_and_match() {
    let src = format!("{SHAPES}\ndescribe (3.0 by 4.0)\ndescribe (circle 1.0)\ndescribe empty");
    assert_eq!(
        run(&src),
        vec![
            Value::Str("rectangle".into()),
            Value::Str("circle".into()),
            Value::Str("nothing".into())
        ]
    );
}

#[test]
fn non_exhaustive_match_is_an_error() {
    let src = r#"
ENUM Light { red, green }
FUNCTION name (l: Light): String { MATCH l { red => "r" } }
"#;
    assert!(run_err(src).contains("non-exhaustive MATCH; missing: green"));
}

#[test]
fn record_literal_defaults_and_bare_boolean_shorthand() {
    let src = r#"
RECORD Job {
    name (String),
    retries (Int DEFAULT 3),
    dry run (Boolean DEFAULT false)
}
LET j: Job = { name "build", dry run }
MATCH j { { name (n), retries (_), dry run (_) } => n }
MATCH j { { name (_), retries (r), dry run (_) } => r }
MATCH j { { name (_), retries (_), dry run (d) } => d }
"#;
    assert_eq!(
        run(src),
        vec![Value::Str("build".into()), Value::Int(3), Value::Bool(true)]
    );
}

#[test]
fn fields_are_anyfix_phrases_with_slots_anywhere() {
    let src = r#"
RECORD Person {
    (String) 's name,
    age (Int),
    between (Int) and (Int)
}
LET p: Person = { "Ada" 's name, age 36, between 1 and 5 }
MATCH p { { (a) 's name, age (_), between (_) and (_) } => a }
MATCH p { { (_) 's name, age (g), between (lo) and (hi) } => g }
MATCH p { { (_) 's name, age (_), between (lo) and (hi) } => hi }
"#;
    assert_eq!(
        run(src),
        vec![Value::Str("Ada".into()), Value::Int(36), Value::Int(5)]
    );
}

#[test]
fn pipe_requires_exactly_one_placeholder() {
    assert!(run_err(r#"list file {} |> unwrap (_) |> count (3)"#).contains("exactly one `_`"));
    assert!(run_err(r#"count (_)"#).contains("`_` can only be used"));
}

#[test]
fn user_function_with_infix_signature_and_generics() {
    let src = r#"
FUNCTION (a: List (T)) has more than (b: List (T)): Boolean {
    IF (count a) equals (count b) THEN false ELSE true
}
[1, 2, 3] has more than [1]
"#;
    // `(count a) equals (count b)` is not Int-comparable via `>`; here we only
    // check that generics unify and the call resolves.
    assert_eq!(run(src), vec![Value::Bool(true)]);
}

#[test]
fn unknown_name_lists_variables_in_scope() {
    let msg = run_err("LET x = 1\ncount y");
    assert!(msg.contains("unknown name `y`"), "{msg}");
    assert!(msg.contains("x"), "{msg}");
}

#[test]
fn overlapping_signatures_are_rejected() {
    let msg = run_err("FUNCTION ping (a: Int): Int { a }\nFUNCTION ping (b: Int): Int { 1 }");
    assert!(msg.contains("clashes"), "{msg}");
    // Specialization and bare generics always overlap something.
    let msg = run_err("FUNCTION f (a: List (T)): Int { 1 }\nFUNCTION f (a: List (Int)): Int { 2 }");
    assert!(msg.contains("specialization"), "{msg}");
    let msg = run_err("FUNCTION f (a: T): Int { 1 }\nFUNCTION f (a: Int): Int { 2 }");
    assert!(msg.contains("clashes"), "{msg}");
}

#[test]
fn nested_calls_need_parentheses() {
    // `count list file {}` is not a single call: arguments are single tokens.
    let msg = run_err("count list file {}");
    assert!(msg.contains("no function matches"), "{msg}");
}

const OVERLOADS: &str = r#"
RECORD Bag { items (List (Int)) }
RECORD Pair { left (Int), right (Int) }
RECORD Triple { left (Int), right (Int), extra (Int) }
FUNCTION describe (x: Int): String { "int" }
FUNCTION describe (x: Boolean): String { "boolean" }
FUNCTION describe (x: List (T)): String { "list" }
FUNCTION describe (x: Bag): String { "bag" }
FUNCTION sum (p: Pair): Int { 1 }
FUNCTION sum (t: Triple): Int { 3 }
"#;

#[test]
fn overload_is_chosen_by_argument_types() {
    let src = format!(
        "{OVERLOADS}\ndescribe 1\ndescribe true\ndescribe [\"a\"]\ndescribe []\ndescribe {{ items [1] }}"
    );
    let want: Vec<Value> = ["int", "boolean", "list", "list", "bag"]
        .iter()
        .map(|s| Value::Str((*s).into()))
        .collect();
    assert_eq!(run(&src), want);
}

#[test]
fn failed_overload_lists_argument_types_and_candidates() {
    let msg = run_err(&format!("{OVERLOADS}\ndescribe \"hi\""));
    assert!(msg.contains("no overload of `describe <arg>`"), "{msg}");
    assert!(msg.contains("argument types (String)"), "{msg}");
    assert!(
        msg.contains("(x: Int) describe") || msg.contains("describe (x: Int)"),
        "{msg}"
    );
    assert!(msg.contains("Fix:"), "{msg}");
}

#[test]
fn record_literal_picks_the_only_record_it_fits() {
    let src = format!("{OVERLOADS}\nsum {{ left 1, right 2 }}\nsum {{ left 1, right 2, extra 3 }}");
    assert_eq!(run(&src), vec![Value::Int(1), Value::Int(3)]);
}

#[test]
fn ambiguous_record_literal_needs_an_annotation() {
    let src = r#"
RECORD Apple { x (Int) }
RECORD Banana { x (Int) }
FUNCTION f (a: Apple): Int { 1 }
FUNCTION f (b: Banana): Int { 2 }
"#;
    let msg = run_err(&format!("{src}\nf {{ x 1 }}"));
    assert!(msg.contains("ambiguous call"), "{msg}");
    assert!(msg.contains("{ ... } : Record Name"), "{msg}");
    assert_eq!(
        run(&format!("{src}\nf ({{ x 1 }} : Banana)")),
        vec![Value::Int(2)]
    );
}

#[test]
fn bare_record_literal_has_no_type_of_its_own() {
    let msg = run_err("LET x = { a 1 }");
    assert!(msg.contains("annotate it"), "{msg}");
    let msg = run_err("RECORD Apple { x (Int) }\nLET x = { y 1 } : Apple");
    assert!(msg.contains("has no field `y (Int)`"), "{msg}");
    assert!(msg.contains("its fields are: x (Int)"), "{msg}");
}

#[test]
fn omitted_fields_need_a_default() {
    let decl = "RECORD Job { name (String), retries (Int DEFAULT 3) }\n";
    let get = "FUNCTION retries (j: Job): Int { MATCH j { { name (_), retries (r) } => r } }\n";
    assert_eq!(
        run(&format!("{decl}{get}retries {{ name \"b\" }}")),
        vec![Value::Int(3)]
    );
    assert_eq!(
        run(&format!("{decl}{get}retries {{ name \"b\", retries 9 }}")),
        vec![Value::Int(9)]
    );
    let msg = run_err(&format!("{decl}{get}retries {{ retries 9 }}"));
    assert!(
        msg.contains("missing required field `name (String)`"),
        "{msg}"
    );
}

#[test]
fn no_coercions_and_empty_values_fit_anywhere() {
    // A String is not a Path-like thing: functions just take String.
    run("read file \"nope.txt\"");
    // `none` and `[]` are subtypes of every Option / List.
    run("count []");
    run("LET x = IF true THEN (some 1) ELSE none\nx");
    let msg = run_err("IF true THEN 1 ELSE \"a\"");
    assert!(msg.contains("IF branches must have the same type"), "{msg}");
}

#[test]
fn file_roundtrip() {
    let dir = std::env::temp_dir().join(format!("resh-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    let src = format!(
        r#"
write "hello" to file {a:?}
move {a:?} to {b:?}
read file {b:?}
"#
    );
    let vs = run(&src);
    assert_eq!(
        vs.last().unwrap(),
        &Value::Variant("Result".into(), 0, vec![Value::Str("hello".into())])
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn list_file_has_ls_parity_options() {
    let dir = std::env::temp_dir().join(format!("resh-ls-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("big.txt"), "0123456789").unwrap();
    std::fs::write(dir.join("a.rs"), "x").unwrap();
    std::fs::write(dir.join(".hidden"), "h").unwrap();
    std::fs::write(dir.join("sub").join("deep.txt"), "d").unwrap();
    let d = format!("{:?}", dir.to_string_lossy());
    let names = |opts: &str| -> Vec<String> {
        let src = format!("list file in {d} {opts} |> unwrap (_)");
        let Value::List(xs) = run(&src).pop().unwrap() else {
            panic!("not a list")
        };
        xs.iter()
            .map(|e| match e {
                Value::Record(_, f) => match &f[3] {
                    Value::Str(s) => s.clone(),
                    _ => panic!(),
                },
                _ => panic!(),
            })
            .collect()
    };
    assert_eq!(names("{}"), ["a.rs", "big.txt", "sub"]);
    assert_eq!(names("{ reverse }"), ["sub", "big.txt", "a.rs"]);
    assert_eq!(names("{ include hidden file }")[0], ".hidden");
    assert_eq!(names("{ sort by (size) }")[0], "big.txt");
    assert_eq!(
        names("{ recursive }"),
        ["a.rs", "big.txt", "sub", "deep.txt"]
    );
    assert_eq!(names("{ directory itself }").len(), 1);
    assert_eq!(render("human size of 2048"), "\"2.0K\"");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn type_names_are_anyfix_and_capitalized() {
    let ok = "FUNCTION first (xs: List (T)): Result (T) Or (String) { NATIVE \"unwrap\" }";
    assert!(Session::new().run(ok).is_ok());
    assert!(run_err("RECORD my thing { a: Int }").contains("capitalize every word"));
    assert!(run_err("FUNCTION f (x: int): Int { 1 }").contains("capitalize every word"));
    assert!(run_err("FUNCTION f (x: List<Int>): Int { 1 }").contains("unexpected character `<`"));
    assert!(run_err("FUNCTION f (x: List): Int { 1 }").contains("unknown type `List`"));
}

// ---------- `::` qualifiers ----------

mod qualifiers {
    use resh::ast::{Decl, Expr, ExprKind, Item, SeqItem, Stmt, TyExpr};
    use resh::lexer::{Tok, lex};
    use resh::parser::parse_program;
    use resh::{Diag, Session};

    fn parse(src: &str) -> Vec<Item> {
        parse_program(src).unwrap_or_else(|d| panic!("unexpected error: {d}"))
    }

    fn parse_err(src: &str) -> String {
        match parse_program(src) {
            Ok(_) => panic!("expected a parse error for {src:?}"),
            Err(d) => d.msg,
        }
    }

    /// Renders the parse tree of an expression with every qualifier scope
    /// parenthesised: `a::b::c d` becomes `a::(b::(c d))`.
    fn shape(e: &Expr) -> String {
        match &e.k {
            ExprKind::Qualified(q, inner) => format!("{q}::({})", shape(inner)),
            ExprKind::Seq(items) => items
                .iter()
                .map(|i| match i {
                    SeqItem::Word(w, _) => w.clone(),
                    SeqItem::Atom(e) => shape(e),
                })
                .collect::<Vec<_>>()
                .join(" "),
            ExprKind::Str(s) => format!("{s:?}"),
            ExprKind::Int(n) => n.to_string(),
            ExprKind::Brace(fields) if fields.is_empty() => "{}".into(),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn only_expr(src: &str) -> Expr {
        match parse(src).as_slice() {
            [Item::Stmt(Stmt::Expr(e))] => e.clone(),
            other => panic!("expected one expression, got {other:?}"),
        }
    }

    fn only_let_ascription(src: &str) -> (Expr, TyExpr) {
        match parse(src).as_slice() {
            [Item::Stmt(Stmt::Let(_, e, _))] => match &e.k {
                ExprKind::Ascribe(inner, ty) => ((**inner).clone(), ty.clone()),
                other => panic!("expected an ascription, got {other:?}"),
            },
            other => panic!("expected one LET, got {other:?}"),
        }
    }

    fn check_err(src: &str) -> Diag {
        Session::new().run(src).expect_err("expected an error")
    }

    #[test]
    fn lexer_distinguishes_double_colon_from_colon() {
        let toks: Vec<Tok> = lex("a::b: c:::d")
            .unwrap()
            .into_iter()
            .map(|t| t.tok)
            .collect();
        assert_eq!(
            toks,
            vec![
                Tok::Word("a".into()),
                Tok::ColonColon,
                Tok::Word("b".into()),
                Tok::Colon,
                Tok::Word("c".into()),
                Tok::ColonColon,
                Tok::Colon,
                Tok::Word("d".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn qualified_function_call() {
        assert_eq!(
            shape(&only_expr(r#"git::commit "x""#)),
            r#"git::(commit "x")"#
        );
    }

    #[test]
    fn qualifier_covers_the_whole_call_to_its_right() {
        assert_eq!(
            shape(&only_expr(r#"fd::find "" in "src" {}"#)),
            r#"fd::(find "" in "src" {})"#
        );
    }

    #[test]
    fn qualifiers_are_right_associative() {
        assert_eq!(shape(&only_expr("a::b::c d")), "a::(b::(c d))");
    }

    #[test]
    fn qualified_infix_call_puts_the_qualifier_first() {
        assert_eq!(shape(&only_expr("fd::(x) argument")), "fd::(x argument)");
    }

    #[test]
    fn qualified_type_in_let_annotation() {
        let (value, ty) = only_let_ascription("LET c: git::Commit = make commit");
        assert_eq!(ty.qualifiers, vec!["git".to_string()]);
        assert_eq!(ty.name, "Commit");
        assert!(ty.args.is_empty());
        assert_eq!(shape(&value), "make commit");
    }

    #[test]
    fn qualified_type_chain_inside_a_type_slot() {
        let (_, ty) = only_let_ascription("LET c: List (a::b::Commit) = x");
        assert_eq!(ty.name, "List _");
        assert_eq!(
            ty.args[0].qualifiers,
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(ty.args[0].name, "Commit");
    }

    #[test]
    fn qualified_type_in_signature() {
        let items = parse("FUNCTION show (c: git::Commit): git::Commit { c }");
        let [Item::Decl(Decl::Function { ret, .. })] = items.as_slice() else {
            panic!("expected one function");
        };
        assert_eq!(ret.as_ref().unwrap().qualifiers, vec!["git".to_string()]);
    }

    #[test]
    fn qualifier_must_come_first_in_a_call() {
        let msg = parse_err("a b::c d");
        assert!(msg.contains("`b::` qualifies a whole call"), "{msg}");
        let msg = parse_err("(x) fd::argument");
        assert!(msg.contains("`fd::` qualifies a whole call"), "{msg}");
        let msg = parse_err("FUNCTION git::commit (m: String): Unit { m }");
        assert!(msg.contains("must come first"), "{msg}");
    }

    #[test]
    fn qualifier_needs_a_call_after_it() {
        assert!(parse_err("a::").contains("a call after the `::` qualifier"));
    }

    #[test]
    fn single_colon_still_means_ascription_and_annotation() {
        let (_, ty) = only_let_ascription("LET x: Int = 1");
        assert!(ty.qualifiers.is_empty());
        assert_eq!(ty.name, "Int");
        assert_eq!(
            Session::new().run("LET x: Int = 1\nx").unwrap(),
            vec![resh::Value::Int(1)]
        );
        assert!(!check_err("LET x: String = 1").msg.is_empty());
    }

    #[test]
    fn checker_reports_unknown_namespace() {
        let d = check_err(r#"git::commit "x""#);
        assert!(
            d.msg
                .contains("unknown namespace `git`; declared namespaces: none"),
            "{}",
            d.msg
        );
        let d = check_err("FUNCTION show (c: git::Commit): Int { 1 }");
        assert!(d.msg.contains("unknown namespace `git`"), "{}", d.msg);
    }
}

// ---------- NAMESPACE ----------

mod namespaces {
    use resh::{Session, Value};

    const FD: &str = r#"
NAMESPACE fd

ENUM Mode { regex, glob }

FUNCTION name (m: Mode): String {
    MATCH m { regex => "regex", glob => "glob" }
}

FUNCTION helper: Int { 41 }
FUNCTION find (pattern: String) in (dir: String): String {
    join pattern with dir
}
FUNCTION find (pattern: String): Int { (helper) equals 41 |> IF _ THEN count [pattern] ELSE 0 }
"#;

    fn session_with_fd() -> Session {
        let mut s = Session::new();
        s.run(FD).unwrap_or_else(|d| panic!("fd failed: {d}"));
        s
    }

    fn err(s: &mut Session, src: &str) -> String {
        match s.run(src) {
            Ok(v) => panic!("expected an error, got {v:?}"),
            Err(d) => d.msg,
        }
    }

    #[test]
    fn other_file_calls_namespaced_function_qualified() {
        let mut s = session_with_fd();
        assert_eq!(
            s.run(r#"fd::find "a" in "b""#).unwrap(),
            vec![Value::Str("ab".into())]
        );
        assert_eq!(s.run(r#"fd::find "a""#).unwrap(), vec![Value::Int(1)]);
    }

    #[test]
    fn unqualified_call_from_outside_is_a_clear_error() {
        let mut s = session_with_fd();
        let msg = err(&mut s, r#"find "a" in "b""#);
        assert!(msg.contains("declared in namespace `fd`"), "{msg}");
        assert!(msg.contains(r#"`fd::find "#), "{msg}");
        // `helper` has no parameters: same hint for a bare word.
        let msg = err(&mut s, "helper");
        assert!(msg.contains("declared in namespace `fd`"), "{msg}");
    }

    #[test]
    fn names_inside_the_file_resolve_unqualified_and_qualified() {
        let mut s = Session::new();
        let src = format!("{FD}\nfind \"a\"\nfd::find \"a\"");
        assert_eq!(s.run(&src).unwrap(), vec![Value::Int(1), Value::Int(1)]);
    }

    #[test]
    fn namespaced_types_are_referenced_qualified_from_outside() {
        let mut s = session_with_fd();
        let src = r#"
FUNCTION show (m: fd::Mode): Int { 1 }
LET m: fd::Mode = fd::regex
show m
fd::name m
"#;
        assert_eq!(
            s.run(src).unwrap(),
            vec![Value::Int(1), Value::Str("regex".into())]
        );
        let msg = err(&mut s, "FUNCTION show2 (m: Mode): Int { 1 }");
        assert!(msg.contains("unknown type `Mode`"), "{msg}");
    }

    #[test]
    fn unknown_namespace_lists_the_declared_ones() {
        let mut s = session_with_fd();
        let msg = err(&mut s, r#"git::find "a""#);
        assert!(msg.contains("unknown namespace `git`"), "{msg}");
        assert!(msg.contains("declared namespaces: fd"), "{msg}");
    }

    #[test]
    fn qualified_call_only_sees_that_namespace() {
        let mut s = session_with_fd();
        let msg = err(&mut s, r#"fd::count [1]"#);
        assert!(msg.contains("no function matches"), "{msg}");
        assert!(msg.contains("in namespace `fd`"), "{msg}");
    }

    #[test]
    fn namespaces_do_not_nest() {
        let mut s = session_with_fd();
        let msg = err(&mut s, r#"fd::git::find "a""#);
        assert!(msg.contains("namespaces are not nested"), "{msg}");
    }

    #[test]
    fn namespace_must_be_first_and_unique() {
        let mut s = Session::new();
        let msg = err(&mut s, "ENUM A { a }\nNAMESPACE fd");
        assert!(msg.contains("must be the first declaration"), "{msg}");
        let msg = err(&mut s, "NAMESPACE fd\nNAMESPACE gd");
        assert!(msg.contains("at most one NAMESPACE"), "{msg}");
        let msg = err(&mut s, "NAMESPACE fd gd");
        assert!(msg.contains("single word"), "{msg}");
        let msg = err(&mut s, "NAMESPACE");
        assert!(msg.contains("a namespace name"), "{msg}");
    }

    #[test]
    fn comments_and_blank_lines_may_precede_namespace() {
        let mut s = Session::new();
        s.run("// header\n\nNAMESPACE fd\nFUNCTION one: Int { 1 }")
            .unwrap();
        assert_eq!(s.run("fd::one").unwrap(), vec![Value::Int(1)]);
    }

    #[test]
    fn same_name_in_two_namespaces_does_not_clash() {
        let mut s = Session::new();
        s.run("NAMESPACE a\nFUNCTION one: Int { 1 }").unwrap();
        s.run("NAMESPACE b\nFUNCTION one: Int { 2 }").unwrap();
        assert_eq!(
            s.run("a::one\nb::one").unwrap(),
            vec![Value::Int(1), Value::Int(2)]
        );
    }
}

// ---------- records: anyfix fields, DEFAULT, unnamed literals, MATCH ----------

mod records {
    use resh::ast::{Decl, FieldPart, Item};
    use resh::lexer::{Tok, lex};
    use resh::parser::parse_program;
    use resh::{Session, Value};

    const SERVER: &str = r#"
RECORD Server Config {
    host (String DEFAULT "localhost"),
    port (Int DEFAULT 8080)
}

FUNCTION summary (c: Server Config): List (String) {
    MATCH c { { host (h), port (p) } => [h, "x"] }
}
FUNCTION port number (c: Server Config): Int {
    MATCH c { { host (_), port (p) } => p }
}
FUNCTION host name (c: Server Config): String {
    MATCH c { { port (_), host (h) } => h }
}
"#;

    fn run(src: &str) -> Vec<Value> {
        Session::new()
            .run(src)
            .unwrap_or_else(|d| panic!("unexpected error: {d}"))
    }

    fn err(src: &str) -> String {
        match Session::new().run(src) {
            Ok(v) => panic!("expected an error, got {v:?}"),
            Err(d) => d.msg,
        }
    }

    fn server(call: &str) -> Vec<Value> {
        run(&format!("{SERVER}\n{call}"))
    }

    fn server_err(call: &str) -> String {
        err(&format!("{SERVER}\n{call}"))
    }

    // ----- the task's "Done when" -----

    #[test]
    fn partial_literal_uses_the_defaults_of_omitted_fields() {
        assert_eq!(server("port number { port 443 }"), vec![Value::Int(443)]);
        assert_eq!(
            server("host name { port 443 }"),
            vec![Value::Str("localhost".into())]
        );
        assert_eq!(
            server("host name { host \"example.com\" }"),
            vec![Value::Str("example.com".into())]
        );
    }

    #[test]
    fn empty_literal_is_every_default() {
        assert_eq!(server("port number {}"), vec![Value::Int(8080)]);
        assert_eq!(server("host name {}"), vec![Value::Str("localhost".into())]);
    }

    #[test]
    fn entry_order_is_free_and_annotation_names_the_record() {
        let src = r#"
LET c: Server Config = { port 1, host "h" }
MATCH c { { host (h), port (p) } => p }
MATCH c { { port (p), host (h) } => h }
"#;
        assert_eq!(server(src), vec![Value::Int(1), Value::Str("h".into())]);
    }

    #[test]
    fn single_case_match_destructures_every_field() {
        let src = r#"
LET c: Server Config = { port 443 }
MATCH c { { host (h), port (p) } => [h] }
"#;
        assert_eq!(
            server(src),
            vec![Value::List(vec![Value::Str("localhost".into())])]
        );
    }

    // ----- literal failures -----

    #[test]
    fn unknown_field_lists_the_record_fields() {
        let msg = server_err("port number { prot 443 }");
        assert!(msg.contains("has no field `prot (Int)`"), "{msg}");
        assert!(
            msg.contains("its fields are: host (String), port (Int)"),
            "{msg}"
        );
    }

    #[test]
    fn slot_value_of_the_wrong_type_names_field_expected_and_found() {
        let msg = server_err("port number { port \"443\" }");
        assert!(
            msg.contains("field `port (Int)` of `Server Config` expects Int, found String"),
            "{msg}"
        );
    }

    #[test]
    fn a_field_given_twice_is_an_error() {
        let msg = server_err("port number { port 1, port 2 }");
        assert!(msg.contains("field `port (Int)` is given twice"), "{msg}");
    }

    #[test]
    fn omitting_a_field_without_default_is_an_error() {
        let src =
            "RECORD Job { name (String), retries (Int DEFAULT 3) }\nLET j: Job = { retries 1 }";
        let msg = err(src);
        assert!(
            msg.contains("missing required field `name (String)`"),
            "{msg}"
        );
        // One slot without a default is enough, even when its sibling has one.
        let src = "RECORD Rec { between (Int DEFAULT 1) and (Int) }\nLET r: Rec = {}";
        let msg = err(src);
        assert!(
            msg.contains("missing required field `between (Int) and (Int)`"),
            "{msg}"
        );
    }

    #[test]
    fn bare_field_name_is_true_only_for_a_boolean_field() {
        let msg = err("RECORD Rec { depth (Int DEFAULT 1) }\nLET r: Rec = { depth }");
        assert!(msg.contains("has no field `depth`"), "{msg}");
        let src = "RECORD Rec { verbose (Boolean DEFAULT false) }\nLET r: Rec = { verbose }\nMATCH r { { verbose (v) } => v }";
        assert_eq!(run(src), vec![Value::Bool(true)]);
        // The explicit form is the same field.
        let src = "RECORD Rec { verbose (Boolean DEFAULT true) }\nLET r: Rec = { verbose false }\nMATCH r { { verbose (v) } => v }";
        assert_eq!(run(src), vec![Value::Bool(false)]);
        let msg = err(
            "RECORD Rec { verbose (Boolean DEFAULT true) }\nLET r: Rec = { verbose, verbose false }",
        );
        assert!(msg.contains("is given twice"), "{msg}");
    }

    #[test]
    fn every_slot_of_a_field_has_its_own_default() {
        let src = r#"
RECORD Rec { between (Int DEFAULT 1) and (Int DEFAULT 9) }
LET r: Rec = {}
MATCH r { { between (lo) and (hi) } => hi }
LET s: Rec = { between 2 and 3 }
MATCH s { { between (lo) and (hi) } => lo }
"#;
        assert_eq!(run(src), vec![Value::Int(9), Value::Int(2)]);
    }

    #[test]
    fn nested_record_literal_fits_a_record_slot() {
        let src = r#"
RECORD Inner { n (Int DEFAULT 7) }
RECORD Outer { inner (Inner DEFAULT {}), tag (String) }
LET o: Outer = { tag "t" }
MATCH o { { inner (i), tag (_) } => MATCH i { { n (n) } => n } }
LET p: Outer = { inner { n 1 }, tag "t" }
MATCH p { { inner (i), tag (_) } => MATCH i { { n (n) } => n } }
"#;
        assert_eq!(run(src), vec![Value::Int(7), Value::Int(1)]);
    }

    #[test]
    fn default_may_call_a_function_declared_later_in_the_file() {
        let src = r#"
RECORD Rec { n (Int DEFAULT forty two) }
FUNCTION forty two: Int { 42 }
LET r: Rec = {}
MATCH r { { n (n) } => n }
"#;
        assert_eq!(run(src), vec![Value::Int(42)]);
    }

    #[test]
    fn default_must_fit_its_slot_type() {
        let msg = err("RECORD Rec { n (Int DEFAULT \"x\") }");
        assert!(msg.contains("DEFAULT of `n (Int)` in `Rec`"), "{msg}");
        assert!(msg.contains("expects Int, found String"), "{msg}");
    }

    // ----- declaration failures and removed forms -----

    #[test]
    fn two_fields_that_a_literal_cannot_tell_apart_are_rejected() {
        let msg = err("RECORD Rec { a (Int), a (String) }");
        assert!(msg.contains("clashes with field"), "{msg}");
        // `{ verbose }` would mean both fields.
        let msg = err("RECORD Rec { verbose (Boolean), verbose }");
        assert!(msg.contains("clashes with field"), "{msg}");
    }

    #[test]
    fn a_field_needs_a_word() {
        let msg = err("RECORD Rec { (Int) }");
        assert!(msg.contains("a field needs at least one word"), "{msg}");
    }

    #[test]
    fn slots_hold_types_not_names() {
        let msg = err("RECORD Rec { a (n: Int) }");
        assert!(!msg.is_empty());
        let msg = err("RECORD Rec { a (Int DEFAULT) }");
        assert!(msg.contains("expected an expression"), "{msg}");
    }

    #[test]
    fn key_value_form_and_optional_fields_are_gone() {
        assert!(!err("RECORD Rec { a: Int }").is_empty());
        assert!(!err("RECORD Rec { a? : Int }").is_empty());
        let msg = err("RECORD Rec { a (Int) }\nLET r: Rec = { a: 1 }");
        assert!(!msg.is_empty(), "{msg}");
        // No `Name { ... }` constructor either.
        let msg = err("RECORD Rec { a (Int) }\nR { a 1 }");
        assert!(msg.contains("no function matches"), "{msg}");
    }

    #[test]
    fn records_have_no_accessor_functions() {
        let msg = server_err("LET c: Server Config = {}\nport c");
        assert!(msg.contains("no function matches `port c`"), "{msg}");
        let msg = server_err("LET c: Server Config = {}\nhost c");
        assert!(msg.contains("no function matches"), "{msg}");
    }

    // ----- destructuring -----

    #[test]
    fn destructuring_names_every_field_and_only_known_ones() {
        let msg = server_err("LET c: Server Config = {}\nMATCH c { { host (h) } => h }");
        assert!(
            msg.contains("must list every field; missing: port (Int)"),
            "{msg}"
        );
        let msg = server_err(
            "LET c: Server Config = {}\nMATCH c { { host (h), port (p), tls (t) } => h }",
        );
        assert!(msg.contains("has no field pattern `tls _`"), "{msg}");
        assert!(msg.contains("host (String), port (Int)"), "{msg}");
        let msg = server_err(
            "LET c: Server Config = {}\nMATCH c { { host (h), host (g), port (p) } => h }",
        );
        assert!(msg.contains("is listed twice"), "{msg}");
    }

    #[test]
    fn destructuring_takes_exactly_one_record_arm() {
        let msg = server_err(
            "LET c: Server Config = {}\nMATCH c { { host (h), port (p) } => h, { host (h), port (p) } => h }",
        );
        assert!(msg.contains("single-case MATCH, found 2 arms"), "{msg}");
        let msg = server_err("LET c: Server Config = {}\nMATCH c { _ => 1 }");
        assert!(msg.contains("pattern must be a record pattern"), "{msg}");
        let msg =
            server_err("LET c: Server Config = {}\nMATCH c { { host (h), port (p) _ } => h }");
        assert!(msg.contains("words and `(name)` binders only"), "{msg}");
    }

    #[test]
    fn destructured_names_have_the_slot_types() {
        // `h` is a String, so it cannot be returned as an Int.
        let msg = server_err(
            "FUNCTION bad (c: Server Config): Int { MATCH c { { host (h), port (_) } => h } }",
        );
        assert!(msg.contains("declared to return Int"), "{msg}");
        assert!(msg.contains("String"), "{msg}");
        // `(_)` ignores a slot and binds nothing.
        let msg = server_err("LET c: Server Config = {}\nMATCH c { { host (_), port (_) } => _ }");
        assert!(msg.contains("`_` can only be used"), "{msg}");
    }

    #[test]
    fn enum_match_still_rejects_record_patterns() {
        let msg =
            err("ENUM Light { a, b }\nFUNCTION f (e: Light): Int { MATCH e { { x (y) } => 1 } }");
        assert!(msg.contains("no variant of `Light` matches"), "{msg}");
    }

    // ----- parser and lexer -----

    #[test]
    fn default_is_a_keyword() {
        let toks: Vec<Tok> = lex("DEFAULT default")
            .unwrap()
            .into_iter()
            .map(|t| t.tok)
            .collect();
        assert_eq!(toks[0], Tok::Kw("DEFAULT".into()));
        assert_eq!(toks[1], Tok::Word("default".into()));
    }

    #[test]
    fn field_ast_is_words_and_slots_with_optional_defaults() {
        let items =
            parse_program("RECORD Rec { between (Int DEFAULT 1) and (Int), flag }").unwrap();
        let [Item::Decl(Decl::Record { fields, .. })] = items.as_slice() else {
            panic!("expected one record");
        };
        let shape = |f: &resh::ast::FieldAst| -> String {
            f.parts
                .iter()
                .map(|p| match p {
                    FieldPart::Word(w) => w.clone(),
                    FieldPart::Slot {
                        default: Some(_), ..
                    } => "(T=)".into(),
                    FieldPart::Slot { default: None, .. } => "(T)".into(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(shape(&fields[0]), "between (T=) and (T)");
        assert_eq!(shape(&fields[1]), "flag");
    }

    #[test]
    fn literal_entries_are_anyfix_phrases() {
        let items = parse_program("{ port 443, between (1 mb) and 5, verbose, }").unwrap();
        let [Item::Stmt(resh::ast::Stmt::Expr(e))] = items.as_slice() else {
            panic!("expected one expression");
        };
        let resh::ast::ExprKind::Brace(entries) = &e.k else {
            panic!("expected a brace literal");
        };
        let lens: Vec<usize> = entries.iter().map(|e| e.items.len()).collect();
        assert_eq!(lens, vec![2, 4, 1]);
    }

    // ----- rendering -----

    #[test]
    fn records_render_as_unnamed_literals() {
        let mut s = Session::new();
        let vs = s
            .run(&format!(
                "{SERVER}\nLET c: Server Config = {{ port 443 }}\nc"
            ))
            .unwrap();
        assert_eq!(s.render(&vs[0], false), "{ host \"localhost\", port 443 }");
        assert_eq!(
            s.render(&vs[0], true),
            "{\"host\":\"localhost\",\"port\":443}"
        );
        let vs = s
            .run("RECORD Rec { between (Int) and (Int), flag (Boolean) }\nLET r: Rec = { between 1 and 2, flag }\nr")
            .unwrap();
        assert_eq!(s.render(&vs[0], false), "{ between 1 and 2, flag (true) }");
        assert_eq!(
            s.render(&vs[0], true),
            "{\"between and\":[1,2],\"flag\":true}"
        );
    }

    // ----- the prelude records -----

    #[test]
    fn prelude_records_use_the_new_form() {
        // Entry has 14 fields; destructuring must list them all.
        let all = "path (_), kind (_), size (s), file name (_), permissions (_), mode (_), \
                   link count (_), owner id (_), group id (_), modified at (_), \
                   accessed at (_), inode number (_), block count (_), link target (_)";
        let src =
            format!("FUNCTION entry size (e: Entry): Int {{ MATCH e {{ {{ {all} }} => s }} }}");
        assert!(Session::new().run(&src).is_ok());
        let short = all.replace(", link target (_)", "");
        let src =
            format!("FUNCTION entry size (e: Entry): Int {{ MATCH e {{ {{ {short} }} => s }} }}");
        let msg = err(&src);
        assert!(
            msg.contains("missing: link target (Option (String))"),
            "{msg}"
        );
        // List Options: every field has a DEFAULT, so `{}` and partial literals fit.
        let msg = err("LET o: List Options = { sort by 1 }");
        assert!(
            msg.contains(
                "field `sort by (Sort Key)` of `List Options` expects Sort Key, found Int"
            ),
            "{msg}"
        );
        assert!(
            Session::new()
                .run("LET o: List Options = { reverse, sort by (size) }")
                .is_ok()
        );
    }
}

#[test]
fn record_literal_diagnostics_list_each_candidate_fields() {
    let src = r#"
RECORD Apple { x (Int), ripe (Boolean) }
RECORD Banana { y (Int DEFAULT 1) }
FUNCTION f (a: Apple): Int { 1 }
FUNCTION f (b: Banana): Int { 2 }
"#;
    let msg = run_err(&format!("{src}\nf {{ z 1 }}"));
    assert!(msg.contains("no overload of `f <arg>`"), "{msg}");
    assert!(msg.contains("record literal { z (Int) }"), "{msg}");
    assert!(msg.contains("`Apple` fields: x (Int), ripe (Boolean)"), "{msg}");
    assert!(msg.contains("`Banana` fields: y (Int) (optional)"), "{msg}");
    // `{}` fits only Banana, whose fields are all optional.
    assert_eq!(run(&format!("{src}\nf {{}}")), vec![Value::Int(2)]);

    let all_default = r#"
RECORD Alpha { x (Int DEFAULT 1) }
RECORD Beta { y (Int DEFAULT 1) }
FUNCTION g (a: Alpha): Int { 1 }
FUNCTION g (b: Beta): Int { 2 }
"#;
    let msg = run_err(&format!("{all_default}\ng {{}}"));
    assert!(msg.contains("ambiguous call"), "{msg}");
    assert!(msg.contains("`Alpha` fields: x (Int) (optional)"), "{msg}");
    assert!(msg.contains("`Beta` fields: y (Int) (optional)"), "{msg}");
}

mod sets {
    use super::*;

    #[test]
    fn set_literal_is_accepted_and_empty_set_works() {
        let src = "ENUM Find Option { include hidden file, extension (e: List (String)), max depth (n: Int) }\n\
                   FUNCTION pick (options: Set (Find Option)): Set (Find Option) { options }\n";
        for lit in [
            "{ extension [\"rs\"], max depth 3 }",
            "{ include hidden file }",
            "{}",
        ] {
            run(&format!("{src}pick {lit}"));
        }
    }

    #[test]
    fn duplicate_variant_names_both_positions() {
        let src = "ENUM Find Option { max depth (n: Int), include hidden file }\n\
                   FUNCTION pick (options: Set (Find Option)): Int { 1 }\n\
                   pick { max depth 1, max depth 2 }";
        let msg = run_err(src);
        assert!(msg.contains("`max depth (Int)` is given twice"), "{msg}");
        assert!(msg.contains("first at 3:8"), "{msg}");
        assert!(msg.contains("again at 3:21"), "{msg}");
    }

    #[test]
    fn unknown_variant_lists_the_variants() {
        let src = "ENUM Find Option { max depth (n: Int), include hidden file }\n\
                   FUNCTION pick (options: Set (Find Option)): Int { 1 }\n\
                   pick { depth 1 }";
        let msg = run_err(src);
        assert!(msg.contains("has no variant `depth (Int)`"), "{msg}");
        assert!(msg.contains("max depth (n: Int)"), "{msg}");
        let msg = run_err(&src.replace("{ depth 1 }", "{ max depth \"x\" }"));
        assert!(msg.contains("expects Int, found String"), "{msg}");
    }

    #[test]
    fn record_or_set_is_decided_by_the_candidates() {
        let src = "ENUM Flag { loud, quiet }\n\
                   RECORD Config { port (Int DEFAULT 80) }\n\
                   FUNCTION f (c: Config): Int { 1 }\n\
                   FUNCTION f (s: Set (Flag)): Int { 2 }\n";
        assert_eq!(run(&format!("{src}f {{ port 1 }}")), vec![Value::Int(1)]);
        assert_eq!(run(&format!("{src}f {{ loud }}")), vec![Value::Int(2)]);
        let msg = run_err(&format!("{src}f {{}}"));
        assert!(msg.contains("ambiguous call"), "{msg}");
    }

    #[test]
    fn set_must_hold_an_enum() {
        let msg = run_err("FUNCTION f (s: Set (Int)): Int { 1 }");
        assert!(msg.contains("variants of an enum"), "{msg}");
    }
}

mod lambdas {
    use super::*;

    fn ints(xs: &[i64]) -> Value {
        Value::List(xs.iter().map(|n| Value::Int(*n)).collect())
    }

    #[test]
    fn flat_map_with_a_lambda_checks_and_evaluates() {
        assert_eq!(
            run("LET xs = [1, 2, 3]\nflat map (xs) with ((x) => [x, x])"),
            vec![ints(&[1, 1, 2, 2, 3, 3])]
        );
        // The result type comes from the lambda body.
        assert_eq!(
            run("flat map ([\"a\"]) with ((s) => [s, \"!\"])"),
            vec![Value::List(vec![Value::Str("a".into()), Value::Str("!".into())])]
        );
    }

    #[test]
    fn lambda_captures_variables_in_scope() {
        assert_eq!(
            run("LET k = 7\nflat map ([1, 2]) with ((x) => [x, k])"),
            vec![ints(&[1, 7, 2, 7])]
        );
    }

    #[test]
    fn lambda_parameter_type_comes_from_the_earlier_argument() {
        // `x` is an Int, so it cannot be joined as a String.
        let msg = run_err("flat map ([1]) with ((x) => [join (x) with \"a\"])");
        // (`join` is overloaded, so the message lists its candidates.)
        assert!(msg.contains("join <arg> with <arg>"), "{msg}");
        assert!(msg.contains("expects String, found Int"), "{msg}");
        // The outer call explains why each of its candidates was rejected.
        assert!(msg.contains("rejected:"), "{msg}");
    }

    #[test]
    fn lambda_before_its_parameter_type_is_known_is_an_error() {
        let src = "FUNCTION apply (f: Function (T) To (T)) to (x: T): T { x }\n\
                   apply ((y) => y) to 1";
        let msg = run_err(src);
        assert!(msg.contains("not known yet"), "{msg}");
        assert!(msg.contains("move the lambda after it"), "{msg}");
    }

    #[test]
    fn bare_lambda_has_no_type_and_non_functions_reject_it() {
        let msg = run_err("LET f = ((x) => x)");
        assert!(msg.contains("no type of its own"), "{msg}");
        let msg = run_err("count ((x) => x)");
        assert!(msg.contains("lambda"), "{msg}");
    }

    #[test]
    fn lambda_needs_exactly_one_parameter_name() {
        let msg = run_err("flat map ([1]) with ((x y) => [x])");
        assert!(msg.contains("exactly one parameter name"), "{msg}");
    }
}

mod tuples_and_spread {
    use super::*;

    fn strs(xs: &[&str]) -> Value {
        Value::List(xs.iter().map(|s| Value::Str(s.to_string())).collect())
    }

    #[test]
    fn spread_splices_lists_into_a_list_literal() {
        assert_eq!(
            run("LET a = [\"1\", \"2\"]\nLET b = [\"3\"]\n[...a, \"x\", ...b]"),
            vec![strs(&["1", "2", "x", "3"])]
        );
        // The operand may be a parenthesized call, and `[]` spreads to nothing.
        assert_eq!(
            run("[...(flat map ([\"a\"]) with ((s) => [s, s])), ...[]]"),
            vec![strs(&["a", "a"])]
        );
    }

    #[test]
    fn spread_needs_a_list_and_matching_element_types() {
        let msg = run_err("[...1]");
        assert!(msg.contains("splices a list"), "{msg}");
        let msg = run_err("[1, ...[\"a\"]]");
        assert!(msg.contains("must share one type"), "{msg}");
        // Outside a list literal `...` does not parse.
        let msg = run_err("LET x = ...[1]");
        assert!(msg.contains("found `...`"), "{msg}");
    }

    #[test]
    fn tuple_value_has_a_tuple_type_and_renders() {
        assert_eq!(
            run("LET t = (\"fd\", [\"a\"])\nt"),
            vec![Value::Tuple(vec![Value::Str("fd".into()), strs(&["a"])])]
        );
        assert_eq!(render("(1, \"x\")"), "(1, \"x\")");
    }

    #[test]
    fn function_takes_a_tuple_parameter() {
        let src = "FUNCTION argv ((command: String, args: List (String))): List (String) {\n\
                   [command, ...args]\n}\n\
                   argv (\"fd\", [\"-H\", \"x\"])\n\
                   LET extra = [\"y\"]\n\
                   argv (\"ls\", [...extra, \"z\"])";
        assert_eq!(
            run(src),
            vec![strs(&["fd", "-H", "x"]), strs(&["ls", "y", "z"])]
        );
        // A tuple written inside its own slot parentheses is the same value.
        let src = "FUNCTION first ((a: Int, b: Int)): Int { a }\nfirst ((7, 8))";
        assert_eq!(run(src), vec![Value::Int(7)]);
    }

    #[test]
    fn tuple_parameter_mismatches_are_reported() {
        let def = "FUNCTION first ((a: Int, b: String)): Int { a }\n";
        let msg = run_err(&format!("{def}first (1, 2)"));
        assert!(msg.contains("(a: Int, b: String)"), "{msg}");
        assert!(msg.contains("found (Int, Int)"), "{msg}");
        let msg = run_err(&format!("{def}first (1, \"x\", 3)"));
        assert!(msg.contains("found (Int, String, Int)"), "{msg}");
        let msg = run_err(&format!("{def}first 1"));
        assert!(msg.contains("found Int"), "{msg}");
    }

    #[test]
    fn tuple_parameters_overload_by_shape() {
        let src = "FUNCTION pick ((a: Int, b: Int)): String { \"ints\" }\n\
                   FUNCTION pick ((a: String, b: Int)): String { \"mixed\" }\n\
                   pick (1, 2)\npick (\"x\", 2)";
        assert_eq!(
            run(src),
            vec![Value::Str("ints".into()), Value::Str("mixed".into())]
        );
        let msg = run_err(
            "FUNCTION pick ((a: Int, b: Int)): Int { a }\nFUNCTION pick ((c: Int, d: Int)): Int { c }",
        );
        assert!(msg.contains("clashes"), "{msg}");
    }

    #[test]
    fn tuple_parameter_needs_two_components_and_no_enum_variants() {
        let msg = run_err("FUNCTION f ((a: Int)): Int { a }");
        assert!(msg.contains("two or more components"), "{msg}");
        let msg = run_err("ENUM E { pair ((a: Int, b: Int)) }");
        assert!(msg.contains("cannot take a tuple slot"), "{msg}");
    }

    #[test]
    fn the_spec_example_wrapper_shape_works() {
        let src = "FUNCTION run ((command: String, args: List (String))): List (String) {\n\
                   [command, ...args]\n}\n\
                   FUNCTION wrap (pattern: String) in (dir: String): List (String) {\n\
                   run (\"fd\", [...[\"--hidden\"], pattern, dir])\n}\n\
                   wrap \"x\" in \"src\"";
        assert_eq!(run(src), vec![strs(&["fd", "--hidden", "x", "src"])]);
    }
}

mod prelude_functions {
    use super::*;

    const COLORS: &str = "ENUM Color { red, green, rgb (n: Int) }\n";

    #[test]
    fn flat_map_over_a_set_returns_a_list() {
        let src = format!(
            "{COLORS}FUNCTION twice (cs: Set (Color)): List (Color) {{ flat map (cs) with ((c) => [c, c]) }}\n\
             twice {{ red, green }}"
        );
        assert_eq!(render(&src), "[red, red, green, green]");
    }

    #[test]
    fn flat_map_over_a_set_type_checks_the_lambda_body() {
        let src = format!(
            "{COLORS}FUNCTION bad (cs: Set (Color)): List (Int) {{ flat map (cs) with ((c) => 1) }}"
        );
        let msg = run_err(&src);
        assert!(msg.contains("flat map"), "{msg}");
    }

    #[test]
    fn flat_map_on_a_non_collection_is_an_error() {
        let msg = run_err("flat map (1) with ((x) => [x])");
        assert!(msg.contains("flat map"), "{msg}");
    }

    #[test]
    fn int_to_string() {
        assert_eq!(run("(42) to string"), vec![Value::Str("42".into())]);
        
    }

    #[test]
    fn float_to_string() {
        assert_eq!(run("(1.5) to string"), vec![Value::Str("1.5".into())]);
    }

    #[test]
    fn to_string_rejects_other_types() {
        let msg = run_err("(\"a\") to string");
        assert!(msg.contains("to string"), "{msg}");
    }

    #[test]
    fn map_result_applies_to_ok() {
        assert_eq!(render("map (ok 1) with ((n) => (n) to string)"), "ok (\"1\")");
    }

    #[test]
    fn map_result_keeps_error() {
        let src = "LET r = read file \"/definitely/not/here\"\n\
                   map (r) with ((s) => count (lines of (s)))";
        let out = render(src);
        assert!(out.starts_with("error"), "{out}");
    }

    #[test]
    fn map_result_needs_a_result() {
        let msg = run_err("map ([1]) with ((n) => n)");
        assert!(msg.contains("map"), "{msg}");
    }

    #[test]
    fn join_strings() {
        assert_eq!(run("join (\"a\") with (\"b\")"), vec![Value::Str("ab".into())]);
    }

    #[test]
    fn join_lists() {
        assert_eq!(
            run("join ([1, 2]) with ([3])"),
            vec![Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])]
        );
    }

    #[test]
    fn join_mismatched_types_is_an_error() {
        let msg = run_err("join ([1]) with ([\"a\"])");
        assert!(msg.contains("join"), "{msg}");
        let msg = run_err("join (\"a\") with ([\"a\"])");
        assert!(msg.contains("join"), "{msg}");
    }
}

// ---------- Size and Duration units ----------

mod units {
    use resh::{Session, Value};

    const FD: &str = r#"
NAMESPACE fd

ENUM Find Option {
    size at least (s: Size),
    changed within (d: Duration),
    max depth (n: Int)
}

FUNCTION find (pattern: String) in (dir: String) (options: Set (Find Option)): Int { 1 }
"#;

    fn session() -> Session {
        let mut s = Session::new();
        s.run(FD).unwrap_or_else(|d| panic!("fd failed: {d}"));
        s
    }

    fn eval(src: &str) -> Value {
        Session::new()
            .run(src)
            .unwrap_or_else(|d| panic!("unexpected error: {d}"))
            .pop()
            .unwrap()
    }

    #[test]
    fn size_at_least_one_mb_type_checks() {
        let v = session()
            .run(r#"fd::find "" in "." { size at least (1 mb) }"#)
            .unwrap_or_else(|d| panic!("{d}"));
        assert_eq!(v, vec![Value::Int(1)]);
    }

    #[test]
    fn size_at_least_five_days_is_a_type_error() {
        let msg = match session().run(r#"fd::find "" in "." { size at least (5 days) }"#) {
            Ok(v) => panic!("expected an error, got {v:?}"),
            Err(d) => d.msg,
        };
        assert!(msg.contains("Duration") && msg.contains("Size"), "{msg}");
    }

    #[test]
    fn duration_units_fit_duration_slots() {
        session()
            .run(r#"fd::find "" in "." { changed within (2 hours), max depth 1 }"#)
            .unwrap_or_else(|d| panic!("{d}"));
        session()
            .run(r#"fd::find "" in "." { changed within ((3 days) ago) }"#)
            .unwrap_or_else(|d| panic!("{d}"));
    }

    #[test]
    fn size_accessor_gives_bytes() {
        assert_eq!(eval("(1 kb) bytes"), Value::Int(1_000));
        assert_eq!(eval("(2 mb) bytes"), Value::Int(2_000_000));
        assert_eq!(eval("(3 gb) bytes"), Value::Int(3_000_000_000));
    }

    #[test]
    fn duration_accessor_gives_seconds() {
        assert_eq!(eval("(90 seconds) seconds"), Value::Int(90));
        assert_eq!(eval("(2 minutes) seconds"), Value::Int(120));
        assert_eq!(eval("(1 hours) seconds"), Value::Int(3_600));
        assert_eq!(eval("(5 days) seconds"), Value::Int(432_000));
        assert_eq!(eval("((5 days) ago) seconds"), Value::Int(432_000));
    }

    #[test]
    fn accessor_name_is_resolved_by_argument_type() {
        // `seconds` is both a constructor (Int) and an accessor (Duration).
        assert_eq!(eval("((10 seconds) seconds) to string"), Value::Str("10".into()));
    }

    #[test]
    fn wrong_unit_argument_is_a_type_error() {
        let msg = match Session::new().run("(1 mb) seconds") {
            Ok(v) => panic!("expected an error, got {v:?}"),
            Err(d) => d.msg,
        };
        assert!(msg.contains("seconds") && msg.contains("Size"), "{msg}");
        let msg = match Session::new().run("(1 days) bytes") {
            Ok(v) => panic!("expected an error, got {v:?}"),
            Err(d) => d.msg,
        };
        assert!(msg.contains("bytes") && msg.contains("Duration"), "{msg}");
        assert!(Session::new().run("(\"a\") mb").is_err());
        assert!(Session::new().run("(1 mb) ago").is_err());
    }
}
