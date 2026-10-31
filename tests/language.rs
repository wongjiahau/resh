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
},

FUNCTION describe (s: Shape) -> String {
    MATCH (s) {
        circle (r) => "circle",
        (w) by (h) => "rectangle",
        empty => "nothing"
    }
},
"#;

#[test]
fn anyfix_constructors_and_match() {
    let src =
        format!("{SHAPES}\ndescribe (3.0 by 4.0),\ndescribe (circle 1.0),\ndescribe (empty),");
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
ENUM Light { red, green },
FUNCTION name (l: Light) -> String { MATCH (l) { red => "r" } }
"#;
    assert!(run_err(src).contains("non-exhaustive MATCH; missing: green"));
}

#[test]
fn record_literal_defaults_and_bare_boolean_shorthand() {
    let src = r#"
RECORD Job {
    name (String),
    retries (Int DEFAULT 3),
    dry run (Boolean DEFAULT (false))
},
LET (j : Job) : { name "build", dry run },
MATCH (j) { { name (n), retries (_), dry run (_) } => n },
MATCH (j) { { name (_), retries (r), dry run (_) } => r },
MATCH (j) { { name (_), retries (_), dry run (d) } => d },
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
    (String) is name,
    age (Int),
    between (Int) and (Int)
},
LET (p : Person) : { "Ada" is name, age 36, between 1 and 5 },
MATCH (p) { { (a) is name, age (_), between (_) and (_) } => a },
MATCH (p) { { (_) is name, age (g), between (lo) and (hi) } => g },
MATCH (p) { { (_) is name, age (_), between (lo) and (hi) } => hi },
"#;
    assert_eq!(
        run(src),
        vec![Value::Str("Ada".into()), Value::Int(36), Value::Int(5)]
    );
}

#[test]
fn dot_takes_at_most_one_placeholder() {
    assert!(run_err(r#"[1] . join _ with _,"#).contains("second `_`"));
    assert!(run_err(r#"count (_),"#).contains("`_` can only be used"));
}

#[test]
fn user_function_with_infix_signature_and_generics() {
    let src = r#"
FUNCTION (a: List (T)) has more than (b: List (T)) -> Boolean {
    IF ((count (a)) equals (count (b))) THEN (false) ELSE (true)
},
[1, 2, 3] has more than [1],
"#;
    // `(count a) equals (count b)` is not Int-comparable via `>`; here we only
    // check that generics unify and the call resolves.
    assert_eq!(run(src), vec![Value::Bool(true)]);
}

#[test]
fn unknown_name_lists_variables_in_scope() {
    let msg = run_err("LET x : 1,\ncount (y),");
    assert!(msg.contains("unknown name `y`"), "{msg}");
    assert!(msg.contains("x"), "{msg}");
}

#[test]
fn overlapping_signatures_are_rejected() {
    let msg = run_err("FUNCTION ping (a: Int) -> Int { a },\nFUNCTION ping (b: Int) -> Int { 1 }");
    assert!(msg.contains("clashes"), "{msg}");
    // Specialization and bare generics always overlap something.
    let msg =
        run_err("FUNCTION f (a: List (T)) -> Int { 1 },\nFUNCTION f (a: List (Int)) -> Int { 2 }");
    assert!(msg.contains("specialization"), "{msg}");
    let msg = run_err("FUNCTION f (a: T) -> Int { 1 },\nFUNCTION f (a: Int) -> Int { 2 }");
    assert!(msg.contains("clashes"), "{msg}");
}

#[test]
fn nested_calls_need_parentheses() {
    // `count list file {}` is not a single call: arguments are single tokens.
    let msg = run_err("count list file {},");
    assert!(msg.contains("no function matches"), "{msg}");
}

const OVERLOADS: &str = r#"
RECORD Bag { items (List (Int)) },
RECORD Pair { left (Int), right (Int) },
RECORD Triple { left (Int), right (Int), extra (Int) },
FUNCTION describe (x: Int) -> String { "int" },
FUNCTION describe (x: Boolean) -> String { "boolean" },
FUNCTION describe (x: List (T)) -> String { "list" },
FUNCTION describe (x: Bag) -> String { "bag" },
FUNCTION sum (p: Pair) -> Int { 1 },
FUNCTION sum (t: Triple) -> Int { 3 },
"#;

#[test]
fn overload_is_chosen_by_argument_types() {
    let src = format!(
        "{OVERLOADS}\ndescribe 1,\ndescribe (true),\ndescribe [\"a\"],\ndescribe [],\ndescribe {{ items [1] }},"
    );
    let want: Vec<Value> = ["int", "boolean", "list", "list", "bag"]
        .iter()
        .map(|s| Value::Str((*s).into()))
        .collect();
    assert_eq!(run(&src), want);
}

#[test]
fn failed_overload_lists_argument_types_and_candidates() {
    let msg = run_err(&format!("{OVERLOADS}\ndescribe \"hi\","));
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
    let src =
        format!("{OVERLOADS}\nsum {{ left 1, right 2 }},\nsum {{ left 1, right 2, extra 3 }},");
    assert_eq!(run(&src), vec![Value::Int(1), Value::Int(3)]);
}

#[test]
fn ambiguous_record_literal_needs_an_annotation() {
    let src = r#"
RECORD Apple { x (Int) },
RECORD Banana { x (Int) },
FUNCTION f (a: Apple) -> Int { 1 },
FUNCTION f (b: Banana) -> Int { 2 }
"#;
    let msg = run_err(&format!("{src},\nf {{ x 1 }},"));
    assert!(msg.contains("ambiguous call"), "{msg}");
    assert!(msg.contains("{ ... } AS Record Name"), "{msg}");
    assert_eq!(
        run(&format!("{src},\nf ({{ x 1 }} AS (Banana)),")),
        vec![Value::Int(2)]
    );
}

#[test]
fn bare_record_literal_has_no_type_of_its_own() {
    let msg = run_err("LET x : { a 1 },");
    assert!(msg.contains("annotate it"), "{msg}");
    let msg = run_err("RECORD Apple { x (Int) },\nLET x : { y 1 } AS (Apple),");
    assert!(msg.contains("has no field `y (Int)`"), "{msg}");
    assert!(msg.contains("its fields are: x (Int)"), "{msg}");
}

#[test]
fn omitted_fields_need_a_default() {
    let decl = "RECORD Job { name (String), retries (Int DEFAULT 3) },\n";
    let get =
        "FUNCTION retries (j: Job) -> Int { MATCH (j) { { name (_), retries (r) } => r } },\n";
    assert_eq!(
        run(&format!("{decl}{get}retries {{ name \"b\" }},")),
        vec![Value::Int(3)]
    );
    assert_eq!(
        run(&format!("{decl}{get}retries {{ name \"b\", retries 9 }},")),
        vec![Value::Int(9)]
    );
    let msg = run_err(&format!("{decl}{get}retries {{ retries 9 }},"));
    assert!(
        msg.contains("missing required field `name (String)`"),
        "{msg}"
    );
}

#[test]
fn no_coercions_and_empty_values_fit_anywhere() {
    // A String is not a Path-like thing: functions just take String.
    run("read file \"nope.txt\",");
    // `none` and `[]` are subtypes of every Option / List.
    run("count [],");
    run("LET x : IF (true) THEN (some 1) ELSE (none),\nx,");
    let msg = run_err("IF (true) THEN 1 ELSE \"a\",");
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
write "hello" to file {a:?},
move {a:?} to {b:?},
read file {b:?},
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
        let src = format!("list file in {d} {opts} . unwrap _,");
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
    assert_eq!(render("human size of 2048,"), "\"2.0K\"");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn type_names_are_anyfix_and_capitalized() {
    let ok = "FUNCTION first (xs: List (T)) -> Result (T) Or (String) { NATIVE \"unwrap\" }";
    assert!(Session::new().run(ok).is_ok());
    assert!(run_err("RECORD my thing { a: Int }").contains("capitalize every word"));
    assert!(run_err("FUNCTION f (x: int) -> Int { 1 }").contains("capitalize every word"));
    assert!(
        run_err("FUNCTION f (x: List<Int>) -> Int { 1 }")
            .contains("type parameters go in parentheses")
    );
    assert!(run_err("FUNCTION f (x: List) -> Int { 1 }").contains("unknown type `List`"));
}

// ---------- `.` qualifiers ----------

mod qualifiers {
    use resh::ast::{Decl, Expr, ExprKind, Item, SeqItem, Stmt, TyExpr};
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

    /// Renders the parse tree of an expression with every argument that is a
    /// call parenthesised: `git.commit "x"` becomes `(git) commit "x"`.
    fn shape(e: &Expr) -> String {
        match &e.k {
            ExprKind::Seq(items) => items
                .iter()
                .map(|i| match i {
                    SeqItem::Word(w, _) => w.clone(),
                    SeqItem::Atom(
                        e @ Expr {
                            k: ExprKind::Seq(_),
                            ..
                        },
                    ) => {
                        format!("({})", shape(e))
                    }
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
        Session::new().run(src).expect_err("expected an error,")
    }

    #[test]
    fn qualified_function_call() {
        assert_eq!(
            shape(&only_expr(r#"git.commit "x","#)),
            r#"(git) commit "x""#
        );
    }

    #[test]
    fn qualifier_fills_the_first_slot_of_the_whole_call() {
        assert_eq!(
            shape(&only_expr(r#"fd.find "" in "src" {},"#)),
            r#"(fd) find "" in "src" {}"#
        );
    }

    #[test]
    fn dot_chains_are_left_associative() {
        assert_eq!(shape(&only_expr("a.b.c d,")), "((a) b) c d");
    }

    #[test]
    fn qualified_infix_call_puts_the_qualifier_first() {
        assert_eq!(shape(&only_expr("fd.(x) argument,")), "(fd) (x) argument");
    }

    #[test]
    fn qualified_type_in_let_annotation() {
        let (value, ty) = only_let_ascription("LET (c : git.Commit) : make commit,");
        assert_eq!(ty.qualifiers, vec!["git".to_string()]);
        assert_eq!(ty.name, "Commit");
        assert!(ty.args.is_empty());
        assert_eq!(shape(&value), "make commit");
    }

    #[test]
    fn qualified_type_inside_a_type_slot() {
        let (_, ty) = only_let_ascription("LET (c : List (git.Commit)) : x,");
        assert_eq!(ty.name, "List _");
        assert_eq!(ty.args[0].qualifiers, vec!["git".to_string()]);
        assert_eq!(ty.args[0].name, "Commit");
    }

    #[test]
    fn qualified_type_in_signature() {
        let items = parse("FUNCTION show (c: git.Commit) -> (git.Commit) { c }");
        let [Item::Decl(Decl::Function { ret, .. })] = items.as_slice() else {
            panic!("expected one function");
        };
        assert_eq!(ret.as_ref().unwrap().qualifiers, vec!["git".to_string()]);
    }

    #[test]
    fn qualifier_needs_a_call_after_it() {
        assert!(!parse_err("a.,").is_empty());
    }

    #[test]
    fn let_annotation_states_the_type() {
        let (_, ty) = only_let_ascription("LET (x : Int) : 1,");
        assert!(ty.qualifiers.is_empty());
        assert_eq!(ty.name, "Int");
        assert_eq!(
            Session::new().run("LET (x : Int) : 1,\nx,").unwrap(),
            vec![resh::Value::Int(1)]
        );
        assert!(!check_err("LET (x : String) : 1,").msg.is_empty());
    }

    #[test]
    fn checker_reports_unknown_namespace() {
        let d = check_err(r#"git.commit "x","#);
        assert!(
            d.msg
                .contains("unknown namespace `git`; declared namespaces: none"),
            "{}",
            d.msg
        );
        let d = check_err("FUNCTION show (c: git.Commit) -> Int { 1 }");
        assert!(d.msg.contains("unknown namespace `git`"), "{}", d.msg);
    }
}

// ---------- NAMESPACE ----------

mod namespaces {
    use resh::{Session, Value};

    const FD: &str = r#"
NAMESPACE fd,

ENUM Mode { regex, glob },

FUNCTION name (m: Mode) -> String {
    MATCH (m) { regex => "regex", glob => "glob" }
},

FUNCTION helper -> Int { 41 },
FUNCTION find (pattern: String) in (dir: String) -> String {
    join (pattern) with (dir)
},
FUNCTION find (pattern: String) -> Int { (helper) equals 41 . IF _ THEN (count [pattern]) ELSE 0 },
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
            s.run(r#"fd.find "a" in "b","#).unwrap(),
            vec![Value::Str("ab".into())]
        );
        assert_eq!(s.run(r#"fd.find "a","#).unwrap(), vec![Value::Int(1)]);
    }

    #[test]
    fn unqualified_call_from_outside_is_a_clear_error() {
        let mut s = session_with_fd();
        let msg = err(&mut s, r#"find "a" in "b","#);
        assert!(msg.contains("declared in namespace `fd`"), "{msg}");
        assert!(msg.contains(r#"`fd.find "#), "{msg}");
        // `helper` has no parameters: same hint for a bare word.
        let msg = err(&mut s, "helper,");
        assert!(msg.contains("declared in namespace `fd`"), "{msg}");
    }

    #[test]
    fn names_inside_the_file_resolve_unqualified_and_qualified() {
        let mut s = Session::new();
        let src = format!("{FD}\nfind \"a\",\nfd.find \"a\",");
        assert_eq!(s.run(&src).unwrap(), vec![Value::Int(1), Value::Int(1)]);
    }

    #[test]
    fn namespaced_types_are_referenced_qualified_from_outside() {
        let mut s = session_with_fd();
        let src = r#"
FUNCTION show (m: fd.Mode) -> Int { 1 },
LET (m : fd.Mode) : fd.regex,
show (m),
fd.name (m),
"#;
        assert_eq!(
            s.run(src).unwrap(),
            vec![Value::Int(1), Value::Str("regex".into())]
        );
        let msg = err(&mut s, "FUNCTION show2 (m: Mode) -> Int { 1 }");
        assert!(msg.contains("unknown type `Mode`"), "{msg}");
    }

    #[test]
    fn unknown_namespace_lists_the_declared_ones() {
        let mut s = session_with_fd();
        let msg = err(&mut s, r#"git.find "a","#);
        assert!(msg.contains("unknown namespace `git`"), "{msg}");
        assert!(msg.contains("declared namespaces: fd"), "{msg}");
    }

    #[test]
    fn qualified_call_only_sees_that_namespace() {
        let mut s = session_with_fd();
        let msg = err(&mut s, r#"fd.count [1],"#);
        assert!(msg.contains("no function matches"), "{msg}");
        assert!(msg.contains("in namespace `fd`"), "{msg}");
    }

    #[test]
    fn namespaces_do_not_nest() {
        let mut s = session_with_fd();
        let msg = err(&mut s, r#"fd.git.find "a","#);
        assert!(msg.contains("namespaces are not nested"), "{msg}");
    }

    #[test]
    fn namespace_must_be_first_and_unique() {
        let mut s = Session::new();
        let msg = err(&mut s, "ENUM A { a },\nNAMESPACE fd");
        assert!(msg.contains("must be the first declaration"), "{msg}");
        let msg = err(&mut s, "NAMESPACE fd,\nNAMESPACE gd");
        assert!(msg.contains("at most one NAMESPACE"), "{msg}");
        let msg = err(&mut s, "NAMESPACE fd gd");
        assert!(msg.contains("single word"), "{msg}");
        let msg = err(&mut s, "NAMESPACE");
        assert!(msg.contains("a namespace name"), "{msg}");
    }

    #[test]
    fn comments_and_blank_lines_may_precede_namespace() {
        let mut s = Session::new();
        s.run("# header\n\nNAMESPACE fd,\nFUNCTION one -> Int { 1 }")
            .unwrap();
        assert_eq!(s.run("fd.one,").unwrap(), vec![Value::Int(1)]);
    }

    #[test]
    fn same_name_in_two_namespaces_does_not_clash() {
        let mut s = Session::new();
        s.run("NAMESPACE a,\nFUNCTION one -> Int { 1 }").unwrap();
        s.run("NAMESPACE b,\nFUNCTION one -> Int { 2 }").unwrap();
        assert_eq!(
            s.run("a.one,\nb.one,").unwrap(),
            vec![Value::Int(1), Value::Int(2)]
        );
    }
}

// ---------- records: anyfix fields, DEFAULT, unnamed literals, MATCH ----------

mod records {
    use resh::ast::{Decl, FieldPart, Item};
    use resh::parser::parse_program;
    use resh::{Session, Value};

    const SERVER: &str = r#"
RECORD Server Config {
    host (String DEFAULT "localhost"),
    port (Int DEFAULT 8080)
},

FUNCTION summary (c: Server Config) -> List (String) {
    MATCH (c) { { host (h), port (p) } => [h, "x"] }
},
FUNCTION port number (c: Server Config) -> Int {
    MATCH (c) { { host (_), port (p) } => p }
},
FUNCTION host name (c: Server Config) -> String {
    MATCH (c) { { port (_), host (h) } => h }
},
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
        assert_eq!(server("port number { port 443 },"), vec![Value::Int(443)]);
        assert_eq!(
            server("host name { port 443 },"),
            vec![Value::Str("localhost".into())]
        );
        assert_eq!(
            server("host name { host \"example.com\" },"),
            vec![Value::Str("example.com".into())]
        );
    }

    #[test]
    fn empty_literal_is_every_default() {
        assert_eq!(server("port number {},"), vec![Value::Int(8080)]);
        assert_eq!(
            server("host name {},"),
            vec![Value::Str("localhost".into())]
        );
    }

    #[test]
    fn entry_order_is_free_and_annotation_names_the_record() {
        let src = r#"
LET (c : Server Config) : { port 1, host "h" },
MATCH (c) { { host (h), port (p) } => p },
MATCH (c) { { port (p), host (h) } => h },
"#;
        assert_eq!(server(src), vec![Value::Int(1), Value::Str("h".into())]);
    }

    #[test]
    fn single_case_match_destructures_every_field() {
        let src = r#"
LET (c : Server Config) : { port 443 },
MATCH (c) { { host (h), port (p) } => [h] },
"#;
        assert_eq!(
            server(src),
            vec![Value::List(vec![Value::Str("localhost".into())])]
        );
    }

    // ----- literal failures -----

    #[test]
    fn unknown_field_lists_the_record_fields() {
        let msg = server_err("port number { prot 443 },");
        assert!(msg.contains("has no field `prot (Int)`"), "{msg}");
        assert!(
            msg.contains("its fields are: host (String), port (Int)"),
            "{msg}"
        );
    }

    #[test]
    fn slot_value_of_the_wrong_type_names_field_expected_and_found() {
        let msg = server_err("port number { port \"443\" },");
        assert!(
            msg.contains("field `port (Int)` of `Server Config` expects Int, found String"),
            "{msg}"
        );
    }

    #[test]
    fn a_field_given_twice_is_an_error() {
        let msg = server_err("port number { port 1, port 2 },");
        assert!(msg.contains("field `port (Int)` is given twice"), "{msg}");
    }

    #[test]
    fn omitting_a_field_without_default_is_an_error() {
        let src = "RECORD Job { name (String), retries (Int DEFAULT 3) },\nLET (j : Job) : { retries 1 },";
        let msg = err(src);
        assert!(
            msg.contains("missing required field `name (String)`"),
            "{msg}"
        );
        // One slot without a default is enough, even when its sibling has one.
        let src = "RECORD Rec { between (Int DEFAULT 1) and (Int) },\nLET (r : Rec) : {},";
        let msg = err(src);
        assert!(
            msg.contains("missing required field `between (Int) and (Int)`"),
            "{msg}"
        );
    }

    #[test]
    fn bare_field_name_is_true_only_for_a_boolean_field() {
        let msg = err("RECORD Rec { depth (Int DEFAULT 1) },\nLET (r : Rec) : { depth },");
        assert!(msg.contains("has no field `depth`"), "{msg}");
        let src = "RECORD Rec { verbose (Boolean DEFAULT (false)) },\nLET (r : Rec) : { verbose },\nMATCH (r) { { verbose (v) } => v },";
        assert_eq!(run(src), vec![Value::Bool(true)]);
        // The explicit form is the same field.
        let src = "RECORD Rec { verbose (Boolean DEFAULT (true)) },\nLET (r : Rec) : { verbose (false) },\nMATCH (r) { { verbose (v) } => v },";
        assert_eq!(run(src), vec![Value::Bool(false)]);
        let msg = err(
            "RECORD Rec { verbose (Boolean DEFAULT (true)) },\nLET (r : Rec) : { verbose, verbose (false) },",
        );
        assert!(msg.contains("is given twice"), "{msg}");
    }

    #[test]
    fn every_slot_of_a_field_has_its_own_default() {
        let src = r#"
RECORD Rec { between (Int DEFAULT 1) and (Int DEFAULT 9) },
LET (r : Rec) : {},
MATCH (r) { { between (lo) and (hi) } => hi },
LET (s : Rec) : { between 2 and 3 },
MATCH (s) { { between (lo) and (hi) } => lo },
"#;
        assert_eq!(run(src), vec![Value::Int(9), Value::Int(2)]);
    }

    #[test]
    fn nested_record_literal_fits_a_record_slot() {
        let src = r#"
RECORD Inner { n (Int DEFAULT 7) },
RECORD Outer { inner (Inner DEFAULT {}), tag (String) },
LET (o : Outer) : { tag "t" },
MATCH (o) { { inner (i), tag (_) } => MATCH (i) { { n (n) } => n } },
LET (p : Outer) : { inner { n 1 }, tag "t" },
MATCH (p) { { inner (i), tag (_) } => MATCH (i) { { n (n) } => n } },
"#;
        assert_eq!(run(src), vec![Value::Int(7), Value::Int(1)]);
    }

    #[test]
    fn default_may_call_a_function_declared_later_in_the_file() {
        let src = r#"
RECORD Rec { n (Int DEFAULT (forty two)) },
FUNCTION forty two -> Int { 42 },
LET (r : Rec) : {},
MATCH (r) { { n (n) } => n },
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
        assert!(msg.contains("expected one value after `DEFAULT`"), "{msg}");
    }

    #[test]
    fn key_value_form_and_optional_fields_are_gone() {
        // `a: Int` is anyfix for `a (Int)`, so only the optional marker is gone.
        assert!(!err("RECORD Rec { a? : Int }").is_empty());
        // No `Name { ... }` constructor either.
        let msg = err("RECORD Rec { a (Int) },\nR { a 1 },");
        assert!(msg.contains("no function matches"), "{msg}");
    }

    #[test]
    fn records_have_no_accessor_functions() {
        let msg = server_err("LET (c : Server Config) : {},\nport c,");
        assert!(msg.contains("no function matches `port c`"), "{msg}");
        let msg = server_err("LET (c : Server Config) : {},\nhost c,");
        assert!(msg.contains("no function matches"), "{msg}");
    }

    // ----- destructuring -----

    #[test]
    fn destructuring_names_every_field_and_only_known_ones() {
        let msg = server_err("LET (c : Server Config) : {},\nMATCH (c) { { host (h) } => h },");
        assert!(
            msg.contains("must list every field; missing: port (Int)"),
            "{msg}"
        );
        let msg = server_err(
            "LET (c : Server Config) : {},\nMATCH (c) { { host (h), port (p), tls (t) } => h },",
        );
        assert!(msg.contains("has no field pattern `tls _`"), "{msg}");
        assert!(msg.contains("host (String), port (Int)"), "{msg}");
        let msg = server_err(
            "LET (c : Server Config) : {},\nMATCH (c) { { host (h), host (g), port (p) } => h },",
        );
        assert!(msg.contains("is listed twice"), "{msg}");
    }

    #[test]
    fn a_second_record_arm_is_unreachable_when_the_first_cannot_fail() {
        let msg = server_err(
            "LET (c : Server Config) : {},\nMATCH (c) { { host (h), port (p) } => h, { host (h), port (p) } => h },",
        );
        assert!(msg.contains("unreachable"), "{msg}");
        let msg = server_err("LET (c : Server Config) : {},\nMATCH (c) { bar => 1 },");
        assert!(msg.contains("pattern must be a record pattern"), "{msg}");
        let msg = server_err(
            "LET (c : Server Config) : {},\nMATCH (c) { { host (h), port (p) _ } => h },",
        );
        assert!(msg.contains("words and slots only"), "{msg}");
    }

    #[test]
    fn destructured_names_have_the_slot_types() {
        // `h` is a String, so it cannot be returned as an Int.
        let msg = server_err(
            "FUNCTION bad (c: Server Config) -> Int { MATCH (c) { { host (h), port (_) } => h } }",
        );
        assert!(msg.contains("declared to return Int"), "{msg}");
        assert!(msg.contains("String"), "{msg}");
        // `(_)` ignores a slot and binds nothing.
        let msg =
            server_err("LET (c : Server Config) : {},\nMATCH (c) { { host (_), port (_) } => _ },");
        assert!(msg.contains("`_` can only be used"), "{msg}");
    }

    #[test]
    fn enum_match_still_rejects_record_patterns() {
        let msg = err(
            "ENUM Light { a, b },\nFUNCTION f (e: Light) -> Int { MATCH (e) { { x (y) } => 1 } }",
        );
        assert!(msg.contains("no variant of `Light` matches"), "{msg}");
    }

    // ----- parser and lexer -----

    #[test]
    fn default_is_an_uppercase_keyword() {
        let msg = parse_program("RECORD Rec { a (Int default 1) }")
            .unwrap_err()
            .msg;
        assert!(msg.contains("capitalize every word"), "{msg}");
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
        let items = parse_program("{ port 443, between (1 mb) and 5, verbose, },").unwrap();
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
                "{SERVER}\nLET (c : Server Config) : {{ port 443 }},\nc,"
            ))
            .unwrap();
        assert_eq!(s.render(&vs[0], false), "{ host \"localhost\", port 443 }");
        assert_eq!(
            s.render(&vs[0], true),
            "{\"host\":\"localhost\",\"port\":443}"
        );
        let vs = s
            .run("RECORD Rec { between (Int) and (Int), flag (Boolean) },\nLET (r : Rec) : { between 1 and 2, flag },\nr,")
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
            format!("FUNCTION entry size (e: Entry) -> Int {{ MATCH (e) {{ {{ {all} }} => s }} }}");
        assert!(Session::new().run(&src).is_ok());
        let short = all.replace(", link target (_)", "");
        let src = format!(
            "FUNCTION entry size (e: Entry) -> Int {{ MATCH (e) {{ {{ {short} }} => s }} }}"
        );
        let msg = err(&src);
        assert!(
            msg.contains("missing: link target (Option (String))"),
            "{msg}"
        );
        // List Options: every field has a DEFAULT, so `{}` and partial literals fit.
        let msg = err("LET (o : List Options) : { sort by 1 },");
        assert!(
            msg.contains(
                "field `sort by (Sort Key)` of `List Options` expects Sort Key, found Int"
            ),
            "{msg}"
        );
        assert!(
            Session::new()
                .run("LET (o : List Options) : { reverse, sort by (size) },")
                .is_ok()
        );
    }
}

#[test]
fn record_literal_diagnostics_list_each_candidate_fields() {
    let src = r#"
RECORD Apple { x (Int), ripe (Boolean) },
RECORD Banana { y (Int DEFAULT 1) },
FUNCTION f (a: Apple) -> Int { 1 },
FUNCTION f (b: Banana) -> Int { 2 }
"#;
    let msg = run_err(&format!("{src},\nf {{ z 1 }},"));
    assert!(msg.contains("no overload of `f <arg>`"), "{msg}");
    assert!(msg.contains("record literal { z (Int) }"), "{msg}");
    assert!(
        msg.contains("`Apple` fields: x (Int), ripe (Boolean)"),
        "{msg}"
    );
    assert!(msg.contains("`Banana` fields: y (Int) (optional)"), "{msg}");
    // `{}` fits only Banana, whose fields are all optional.
    assert_eq!(run(&format!("{src},\nf {{}},")), vec![Value::Int(2)]);

    let all_default = r#"
RECORD Alpha { x (Int DEFAULT 1) },
RECORD Beta { y (Int DEFAULT 1) },
FUNCTION g (a: Alpha) -> Int { 1 },
FUNCTION g (b: Beta) -> Int { 2 }
"#;
    let msg = run_err(&format!("{all_default},\ng {{}},"));
    assert!(msg.contains("ambiguous call"), "{msg}");
    assert!(msg.contains("`Alpha` fields: x (Int) (optional)"), "{msg}");
    assert!(msg.contains("`Beta` fields: y (Int) (optional)"), "{msg}");
}

mod sets {
    use super::*;

    #[test]
    fn set_literal_is_accepted_and_empty_set_works() {
        let src = "ENUM Find Option { include hidden file, extension (e: List (String)), max depth (n: Int) },\nFUNCTION pick (options: Set (Find Option)) -> Set (Find Option) { options },\n";
        for lit in [
            "{ extension [\"rs\"], max depth 3 }",
            "{ include hidden file }",
            "{}",
        ] {
            run(&format!("{src}pick {lit},"));
        }
    }

    #[test]
    fn duplicate_variant_names_both_positions() {
        let src = "ENUM Find Option { max depth (n: Int), include hidden file },\nFUNCTION pick (options: Set (Find Option)) -> Int { 1 },\npick { max depth 1, max depth 2 },";
        let msg = run_err(src);
        assert!(msg.contains("`max depth (Int)` is given twice"), "{msg}");
        assert!(msg.contains("first at 3:8"), "{msg}");
        assert!(msg.contains("again at 3:21"), "{msg}");
    }

    #[test]
    fn unknown_variant_lists_the_variants() {
        let src = "ENUM Find Option { max depth (n: Int), include hidden file },\nFUNCTION pick (options: Set (Find Option)) -> Int { 1 },\npick { depth 1 },";
        let msg = run_err(src);
        assert!(msg.contains("has no variant `depth (Int)`"), "{msg}");
        assert!(msg.contains("max depth (n: Int)"), "{msg}");
        let msg = run_err(&src.replace("{ depth 1 }", "{ max depth \"x\" }"));
        assert!(msg.contains("expects Int, found String"), "{msg}");
    }

    #[test]
    fn record_or_set_is_decided_by_the_candidates() {
        let src = "ENUM Flag { loud, quiet },\nRECORD Config { port (Int DEFAULT 80) },\nFUNCTION f (c: Config) -> Int { 1 },\nFUNCTION f (s: Set (Flag)) -> Int { 2 },\n";
        assert_eq!(run(&format!("{src}f {{ port 1 }},")), vec![Value::Int(1)]);
        assert_eq!(run(&format!("{src}f {{ loud }},")), vec![Value::Int(2)]);
        let msg = run_err(&format!("{src}f {{}},"));
        assert!(msg.contains("ambiguous call"), "{msg}");
    }

    #[test]
    fn set_must_hold_an_enum() {
        let msg = run_err("FUNCTION f (s: Set (Int)) -> Int { 1 }");
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
            run("LET xs : [1, 2, 3],\nflat map (xs) with ((x) => [x, x]),"),
            vec![ints(&[1, 1, 2, 2, 3, 3])]
        );
        // The result type comes from the lambda body.
        assert_eq!(
            run("flat map ([\"a\"]) with ((s) => [s, \"!\"]),"),
            vec![Value::List(vec![
                Value::Str("a".into()),
                Value::Str("!".into())
            ])]
        );
    }

    #[test]
    fn lambda_captures_variables_in_scope() {
        assert_eq!(
            run("LET k : 7,\nflat map ([1, 2]) with ((x) => [x, k]),"),
            vec![ints(&[1, 7, 2, 7])]
        );
    }

    #[test]
    fn lambda_parameter_type_comes_from_the_earlier_argument() {
        // `x` is an Int, so it cannot be joined as a String.
        let msg = run_err("flat map ([1]) with ((x) => [join (x) with \"a\"]),");
        // (`join` is overloaded, so the message lists its candidates.)
        assert!(msg.contains("join <arg> with <arg>"), "{msg}");
        assert!(msg.contains("expects String, found Int"), "{msg}");
        // The outer call explains why each of its candidates was rejected.
        assert!(msg.contains("rejected:"), "{msg}");
    }

    #[test]
    fn lambda_before_its_parameter_type_is_known_is_an_error() {
        let src =
            "FUNCTION apply (f: Function (T) To (T)) to (x: T) -> T { x },\napply ((y) => y) to 1,";
        let msg = run_err(src);
        assert!(msg.contains("not known yet"), "{msg}");
        assert!(msg.contains("move the lambda after it"), "{msg}");
    }

    #[test]
    fn bare_lambda_has_no_type_and_non_functions_reject_it() {
        let msg = run_err("LET f : ((x) => x),");
        assert!(msg.contains("no type of its own"), "{msg}");
        let msg = run_err("count ((x) => x),");
        assert!(msg.contains("lambda"), "{msg}");
    }

    #[test]
    fn lambda_needs_exactly_one_parameter_name() {
        let msg = run_err("flat map ([1]) with ((x y) => [x]),");
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
            run("LET a : [\"1\", \"2\"],\nLET b : [\"3\"],\n[*a, \"x\", *b],"),
            vec![strs(&["1", "2", "x", "3"])]
        );
        // The operand may be a parenthesized call, and `[]` spreads to nothing.
        assert_eq!(
            run("[*(flat map ([\"a\"]) with ((s) => [s, s])), *[]],"),
            vec![strs(&["a", "a"])]
        );
    }

    #[test]
    fn spread_needs_a_list_and_matching_element_types() {
        let msg = run_err("[*1],");
        assert!(msg.contains("splices a list"), "{msg}");
        let msg = run_err("[1, *[\"a\"]],");
        assert!(msg.contains("must share one type"), "{msg}");
        let msg = run_err("LET x : *[1],");
        assert!(msg.contains("only works inside a list literal"), "{msg}");
    }

    #[test]
    fn tuple_value_has_a_tuple_type_and_renders() {
        assert_eq!(
            run("LET t : (\"fd\", [\"a\"]),\nt,"),
            vec![Value::Tuple(vec![Value::Str("fd".into()), strs(&["a"])])]
        );
        assert_eq!(render("(1, \"x\"),"), "(1, \"x\")");
    }

    #[test]
    fn function_takes_a_tuple_parameter() {
        let src = "FUNCTION argv ((command: String, args: List (String))) -> List (String) {\n[command, *args]\n},\nargv (\"fd\", [\"-H\", \"x\"]),\nLET extra : [\"y\"],\nargv (\"ls\", [*extra, \"z\"]),";
        assert_eq!(
            run(src),
            vec![strs(&["fd", "-H", "x"]), strs(&["ls", "y", "z"])]
        );
        // A tuple written inside its own slot parentheses is the same value.
        let src = "FUNCTION first ((a: Int, b: Int)) -> Int { a },\nfirst ((7, 8)),";
        assert_eq!(run(src), vec![Value::Int(7)]);
    }

    #[test]
    fn tuple_parameter_mismatches_are_reported() {
        let def = "FUNCTION first ((a: Int, b: String)) -> Int { a },\n";
        let msg = run_err(&format!("{def}first (1, 2),"));
        assert!(msg.contains("(a: Int, b: String)"), "{msg}");
        assert!(msg.contains("found (Int, Int)"), "{msg}");
        let msg = run_err(&format!("{def}first (1, \"x\", 3),"));
        assert!(msg.contains("found (Int, String, Int)"), "{msg}");
        let msg = run_err(&format!("{def}first 1,"));
        assert!(msg.contains("found Int"), "{msg}");
    }

    #[test]
    fn tuple_parameters_overload_by_shape() {
        let src = "FUNCTION pick ((a: Int, b: Int)) -> String { \"ints\" },\nFUNCTION pick ((a: String, b: Int)) -> String { \"mixed\" },\npick (1, 2),\npick (\"x\", 2),";
        assert_eq!(
            run(src),
            vec![Value::Str("ints".into()), Value::Str("mixed".into())]
        );
        let msg = run_err(
            "FUNCTION pick ((a: Int, b: Int)) -> Int { a },\nFUNCTION pick ((c: Int, d: Int)) -> Int { c }",
        );
        assert!(msg.contains("clashes"), "{msg}");
    }

    #[test]
    fn tuple_parameter_needs_two_components_and_no_enum_variants() {
        let msg = run_err("FUNCTION f ((a: Int,)) -> Int { a }");
        assert!(msg.contains("two or more components"), "{msg}");
        let msg = run_err("ENUM E { pair ((a: Int, b: Int)) }");
        assert!(msg.contains("cannot take a tuple slot"), "{msg}");
    }

    #[test]
    fn the_spec_example_wrapper_shape_works() {
        let src = "FUNCTION exec ((command: String, args: List (String))) -> List (String) {\n[command, *args]\n},\nFUNCTION wrap (pattern: String) in (dir: String) -> List (String) {\nexec (\"fd\", [*[\"--hidden\"], pattern, dir])\n},\nwrap \"x\" in \"src\",";
        assert_eq!(run(src), vec![strs(&["fd", "--hidden", "x", "src"])]);
    }
}

mod prelude_functions {
    use super::*;

    const COLORS: &str = "ENUM Color { red, green, rgb (n: Int) },\n";

    #[test]
    fn flat_map_over_a_set_returns_a_list() {
        let src = format!(
            "{COLORS}FUNCTION twice (cs: Set (Color)) -> List (Color) {{ flat map (cs) with ((c) => [c, c]) }},\ntwice {{ red, green }},"
        );
        assert_eq!(render(&src), "[red, red, green, green]");
    }

    #[test]
    fn flat_map_over_a_set_type_checks_the_lambda_body() {
        let src = format!(
            "{COLORS}FUNCTION bad (cs: Set (Color)) -> List (Int) {{ flat map (cs) with ((c) => 1) }}"
        );
        let msg = run_err(&src);
        assert!(msg.contains("flat map"), "{msg}");
    }

    #[test]
    fn flat_map_on_a_non_collection_is_an_error() {
        let msg = run_err("flat map (1) with ((x) => [x]),");
        assert!(msg.contains("flat map"), "{msg}");
    }

    #[test]
    fn int_to_string() {
        assert_eq!(run("(42) to string,"), vec![Value::Str("42".into())]);
    }

    #[test]
    fn float_to_string() {
        assert_eq!(run("(1.5) to string,"), vec![Value::Str("1.5".into())]);
    }

    #[test]
    fn to_string_rejects_other_types() {
        let msg = run_err("(\"a\") to string,");
        assert!(msg.contains("to string"), "{msg}");
    }

    #[test]
    fn map_result_applies_to_ok() {
        assert_eq!(
            render("map (ok 1) with ((n) => (n) to string),"),
            "ok (\"1\")"
        );
    }

    #[test]
    fn map_result_keeps_error() {
        let src =
            "LET r : read file \"/definitely/not/here\",\nmap (r) with ((s) => count ((s) lines)),";
        let out = render(src);
        assert!(out.starts_with("error"), "{out}");
    }

    #[test]
    fn map_result_needs_a_result() {
        let msg = run_err("map ([1]) with ((n) => n),");
        assert!(msg.contains("map"), "{msg}");
    }

    #[test]
    fn join_strings() {
        assert_eq!(
            run("join (\"a\") with (\"b\"),"),
            vec![Value::Str("ab".into())]
        );
    }

    #[test]
    fn join_lists() {
        assert_eq!(
            run("join ([1, 2]) with ([3]),"),
            vec![Value::List(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3)
            ])]
        );
    }

    #[test]
    fn join_mismatched_types_is_an_error() {
        let msg = run_err("join ([1]) with ([\"a\"]),");
        assert!(msg.contains("join"), "{msg}");
        let msg = run_err("join (\"a\") with ([\"a\"]),");
        assert!(msg.contains("join"), "{msg}");
    }
}

// ---------- Size and Duration units ----------

mod units {
    use resh::{Session, Value};

    const FD: &str = r#"
NAMESPACE fd,

ENUM Find Option {
    size at least (s: Size),
    changed within (d: Duration),
    max depth (n: Int)
},

FUNCTION find (pattern: String) in (dir: String) (options: Set (Find Option)) -> Int { 1 },
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
            .run(r#"fd.find "" in "." { size at least (1 mb) },"#)
            .unwrap_or_else(|d| panic!("{d}"));
        assert_eq!(v, vec![Value::Int(1)]);
    }

    #[test]
    fn size_at_least_five_days_is_a_type_error() {
        let msg = match session().run(r#"fd.find "" in "." { size at least (5 days) },"#) {
            Ok(v) => panic!("expected an error, got {v:?}"),
            Err(d) => d.msg,
        };
        assert!(msg.contains("Duration") && msg.contains("Size"), "{msg}");
    }

    #[test]
    fn duration_units_fit_duration_slots() {
        session()
            .run(r#"fd.find "" in "." { changed within (2 hours), max depth 1 },"#)
            .unwrap_or_else(|d| panic!("{d}"));
        session()
            .run(r#"fd.find "" in "." { changed within ((3 days) ago) },"#)
            .unwrap_or_else(|d| panic!("{d}"));
    }

    #[test]
    fn size_accessor_gives_bytes() {
        assert_eq!(eval("(1 kb) bytes,"), Value::Int(1_000));
        assert_eq!(eval("(2 mb) bytes,"), Value::Int(2_000_000));
        assert_eq!(eval("(3 gb) bytes,"), Value::Int(3_000_000_000));
    }

    #[test]
    fn duration_accessor_gives_seconds() {
        assert_eq!(eval("(90 seconds) seconds,"), Value::Int(90));
        assert_eq!(eval("(2 minutes) seconds,"), Value::Int(120));
        assert_eq!(eval("(1 hours) seconds,"), Value::Int(3_600));
        assert_eq!(eval("(5 days) seconds,"), Value::Int(432_000));
        assert_eq!(eval("((5 days) ago) seconds,"), Value::Int(432_000));
    }

    #[test]
    fn accessor_name_is_resolved_by_argument_type() {
        // `seconds` is both a constructor (Int) and an accessor (Duration).
        assert_eq!(
            eval("((10 seconds) seconds) to string,"),
            Value::Str("10".into())
        );
    }

    #[test]
    fn wrong_unit_argument_is_a_type_error() {
        let msg = match Session::new().run("(1 mb) seconds,") {
            Ok(v) => panic!("expected an error, got {v:?}"),
            Err(d) => d.msg,
        };
        assert!(msg.contains("seconds") && msg.contains("Size"), "{msg}");
        let msg = match Session::new().run("(1 days) bytes,") {
            Ok(v) => panic!("expected an error, got {v:?}"),
            Err(d) => d.msg,
        };
        assert!(msg.contains("bytes") && msg.contains("Duration"), "{msg}");
        assert!(Session::new().run("(\"a\") mb,").is_err());
        assert!(Session::new().run("(1 mb) ago,").is_err());
    }
}

mod run_primitive {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Calls = Rc<RefCell<Vec<(String, Vec<String>)>>>;

    fn ok_str(s: &str) -> Value {
        Value::Variant("Result".into(), 0, vec![Value::Str(s.into())])
    }

    fn err_text(src: &str) -> String {
        let vs = run(src);
        match &vs[0] {
            Value::Variant(n, 1, f) if n == "Result" => match &f[0] {
                Value::Str(s) => s.clone(),
                v => panic!("not a string: {v:?}"),
            },
            v => panic!("expected Err, got {v:?}"),
        }
    }

    #[test]
    fn stdout_is_ok() {
        assert_eq!(
            run(r#"run ("echo", ["hello", "world"]),"#),
            vec![ok_str("hello world\n")]
        );
        assert_eq!(run(r#"run ("true", []),"#), vec![ok_str("")]);
    }

    #[test]
    fn failing_command_gives_stderr_and_exit_code() {
        let e = err_text(r#"run ("sh", ["-c", "echo boom >&2; exit 3"]),"#);
        assert_eq!(e, "boom\nexit code 3");
        assert_eq!(err_text(r#"run ("false", []),"#), "exit code 1");
    }

    #[test]
    fn missing_command_is_err() {
        let e = err_text(r#"run ("resh-no-such-command-xyz", []),"#);
        assert!(
            e.starts_with("cannot run resh-no-such-command-xyz: "),
            "{e}"
        );
    }

    #[test]
    fn shell_metacharacters_are_passed_literally() {
        let src = r#"run ("echo", ["a;b", "$HOME", "*", "`x`", "$(y)"]),"#;
        assert_eq!(run(src), vec![ok_str("a;b $HOME * `x` $(y)\n")]);
    }

    #[test]
    fn argument_types_are_checked() {
        let e = run_err(r#"run ("echo", [1, 2]),"#);
        assert!(e.contains("run"), "{e}");
        let e = run_err(r#"run ("echo"),"#);
        assert!(e.contains("run"), "{e}");
    }

    #[test]
    fn can_be_mocked_to_record_argv() {
        let calls: Calls = Default::default();
        let seen = calls.clone();
        let mut s = Session::new();
        s.set_runner(Box::new(move |c, a| {
            seen.borrow_mut().push((c.to_string(), a.to_vec()));
            Ok("mocked".into())
        }));
        let vs = s.run(r#"run ("fd", ["--hidden", "x"]),"#).unwrap();
        assert_eq!(vs, vec![ok_str("mocked")]);
        assert_eq!(
            *calls.borrow(),
            vec![(
                "fd".to_string(),
                vec!["--hidden".to_string(), "x".to_string()]
            )]
        );
        s.set_runner(Box::new(|_, _| Err("nope".into())));
        let vs = s.run(r#"run ("fd", []),"#).unwrap();
        assert_eq!(
            vs,
            vec![Value::Variant(
                "Result".into(),
                1,
                vec![Value::Str("nope".into())]
            )]
        );
    }
}

// ---------- lib/fd.resh end to end ----------

mod fd_example {
    use resh::{Session, Value};
    use std::cell::RefCell;
    use std::rc::Rc;

    const FD: &str = include_str!("../lib/fd.resh");

    type Calls = Rc<RefCell<Vec<(String, Vec<String>)>>>;

    fn session(reply: Result<&'static str, &'static str>) -> (Session, Calls) {
        let calls: Calls = Rc::default();
        let seen = calls.clone();
        let mut s = Session::new();
        s.set_runner(Box::new(move |c, a| {
            seen.borrow_mut().push((c.to_string(), a.to_vec()));
            reply.map(String::from).map_err(String::from)
        }));
        s.run(FD).unwrap_or_else(|d| panic!("fd.resh failed: {d}"));
        (s, calls)
    }

    fn argv(src: &str) -> Vec<String> {
        let (mut s, calls) = session(Ok(""));
        s.run(src).unwrap_or_else(|d| panic!("{d}"));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "fd");
        calls[0].1.clone()
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn header_note_is_gone() {
        assert!(!FD.contains("does not parse"));
    }

    #[test]
    fn usage_plain() {
        assert_eq!(argv(r#"fd.find "main","#), strs(&["--", "main", "."]));
    }

    #[test]
    fn usage_extension_and_depth() {
        assert_eq!(
            argv(r#"fd.find "" in "src" { extension ["rs", "toml"], max depth 3 },"#),
            strs(&[
                "--extension",
                "rs",
                "--extension",
                "toml",
                "--max-depth",
                "3",
                "--",
                "",
                "src"
            ])
        );
    }

    #[test]
    fn usage_kind_size_duration() {
        assert_eq!(
            argv(
                r#"fd.find "test" in "." { kind (file), size at least (1 mb), changed within (7 days) },"#
            ),
            strs(&[
                "--type",
                "file",
                "--size",
                "+1000000b",
                "--changed-within",
                "604800s",
                "--",
                "test",
                "."
            ])
        );
    }

    #[test]
    fn usage_glob_hidden_exclude() {
        assert_eq!(
            argv(
                r#"fd.find "*.log" in "/var" { match (fd.glob), include hidden file, exclude ["old"] },"#
            ),
            strs(&[
                "--glob",
                "--hidden",
                "--exclude",
                "old",
                "--",
                "*.log",
                "/var"
            ])
        );
    }

    #[test]
    fn scope_block_takes_unqualified_variants() {
        assert_eq!(
            argv(
                r#"fd.{ find "*.log" in "/var" { match (glob), include hidden file, exclude ["old"] }, }"#
            ),
            strs(&[
                "--glob",
                "--hidden",
                "--exclude",
                "old",
                "--",
                "*.log",
                "/var"
            ])
        );
    }

    #[test]
    fn result_is_lines_of_stdout() {
        let (mut s, _) = session(Ok("a.rs\nb.rs\n"));
        let v = s.run(r#"fd.find "x","#).unwrap();
        let list = Value::List(vec![Value::Str("a.rs".into()), Value::Str("b.rs".into())]);
        assert_eq!(v, vec![Value::Variant("Result".into(), 0, vec![list])]);
    }

    #[test]
    fn error_is_passed_through() {
        let (mut s, _) = session(Err("boom"));
        let v = s.run(r#"fd.find "x","#).unwrap();
        assert_eq!(
            v,
            vec![Value::Variant(
                "Result".into(),
                1,
                vec![Value::Str("boom".into())]
            )]
        );
    }

    #[test]
    fn bad_calls_are_type_errors() {
        let (mut s, _) = session(Ok(""));
        let e = s
            .run(r#"fd.find "" in "." { max depth 1, max depth 2 },"#)
            .unwrap_err();
        assert!(e.msg.contains("max depth"), "{}", e.msg);
        let e = s
            .run(r#"fd.find "" in "." { case (fd.sensitive), case (fd.insensitive) },"#)
            .unwrap_err();
        assert!(e.msg.contains("case"), "{}", e.msg);
        let e = s
            .run(r#"fd.find "" in "." { size at least (5 days) },"#)
            .unwrap_err();
        assert!(e.msg.contains("Size"), "{}", e.msg);
        let e = s.run(r#"find "x","#).unwrap_err();
        assert!(e.msg.contains("fd."), "{}", e.msg);
    }

    #[test]
    fn nested_patterns_check_exhaustiveness() {
        let (mut s, _) = session(Ok(""));
        let e = s
            .run("ENUM Wrap { w (Entry Kind) },\nFUNCTION f (x: Wrap) -> Int { MATCH (x) { w (file) => 1, w (directory) => 2 } }")
            .unwrap_err();
        assert!(
            e.msg.contains("non-exhaustive") && e.msg.contains("symlink"),
            "{}",
            e.msg
        );
    }
}

// ---------- lib/rg.resh end to end ----------

mod rg_example {
    use resh::{Session, Value};
    use std::cell::RefCell;
    use std::rc::Rc;

    const RG: &str = include_str!("../lib/rg.resh");
    const BASE: [&str; 3] = ["--no-heading", "--line-number", "--column"];

    type Calls = Rc<RefCell<Vec<(String, Vec<String>)>>>;

    fn session(reply: Result<&'static str, &'static str>) -> (Session, Calls) {
        let calls: Calls = Rc::default();
        let seen = calls.clone();
        let mut s = Session::new();
        s.set_runner(Box::new(move |c, a| {
            seen.borrow_mut().push((c.to_string(), a.to_vec()));
            reply.map(String::from).map_err(String::from)
        }));
        s.run(RG).unwrap_or_else(|d| panic!("rg.resh failed: {d}"));
        (s, calls)
    }

    fn argv(src: &str) -> Vec<String> {
        let (mut s, calls) = session(Ok(""));
        s.run(src).unwrap_or_else(|d| panic!("{d}"));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "rg");
        calls[0].1.clone()
    }

    fn expected(mid: &[&str], tail: &[&str]) -> Vec<String> {
        BASE.iter()
            .chain(mid)
            .chain(["--"].iter())
            .chain(tail)
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn matches_are_typed_and_context_lines_are_dropped() {
        let (mut s, _) = session(Ok(
            "a.rs-1-ctx: with:colons\na.rs:2:5:let x: i32 = 1;\n--\nb.rs:10:1:fn main() {}\n",
        ));
        let out = s.run(r#"rg.search "x","#).unwrap();
        let shown = s.render(&out[0], false);
        assert!(
            shown.contains("line 2,") && shown.contains("column 5,"),
            "{shown}"
        );
        assert!(shown.contains("let x: i32 = 1;"), "{shown}");
        assert!(shown.contains("b.rs") && !shown.contains("ctx"), "{shown}");
    }

    #[test]
    fn search_raw_keeps_every_line() {
        let (mut s, _) = session(Ok("a.rs-1-ctx\na.rs:2:5:x\n"));
        let out = s.run(r#"rg.search raw "x" in "." {},"#).unwrap();
        assert!(s.render(&out[0], false).contains("a.rs-1-ctx"));
    }

    #[test]
    fn usage_plain() {
        assert_eq!(argv(r#"rg.search "TODO","#), expected(&[], &["TODO", "."]));
    }

    #[test]
    fn usage_type_and_count() {
        assert_eq!(
            argv(r#"rg.search "fn main" in "src" { file type ["rust"], max count 1 },"#),
            expected(&["--type", "rust", "--max-count", "1"], &["fn main", "src"])
        );
    }

    #[test]
    fn usage_word_case_context() {
        assert_eq!(
            argv(
                r#"rg.search "error" in "." { match (rg.whole word), case (rg.insensitive), context 2 },"#
            ),
            expected(
                &["--word-regexp", "--ignore-case", "--context", "2"],
                &["error", "."]
            )
        );
    }

    #[test]
    fn usage_hidden_glob_size() {
        assert_eq!(
            argv(
                r#"rg.search "secret" in "." { include hidden file, glob ["!target", "!*.lock"], max file size (1 mb) },"#
            ),
            expected(
                &[
                    "--hidden",
                    "--glob",
                    "!target",
                    "--glob",
                    "!*.lock",
                    "--max-filesize",
                    "1000000"
                ],
                &["secret", "."]
            )
        );
    }

    #[test]
    fn result_is_typed_matches() {
        let (mut s, _) = session(Ok("a.rs:1:1:x\nb.rs:2:3:x\n"));
        let v = s.run(r#"rg.search "x","#).unwrap();
        let m = |p: &str, l: i64, c: i64| {
            Value::Record(
                "rg.Match".into(),
                vec![
                    Value::Str(p.into()),
                    Value::Int(l),
                    Value::Int(c),
                    Value::Str("x".into()),
                ],
            )
        };
        let list = Value::List(vec![m("a.rs", 1, 1), m("b.rs", 2, 3)]);
        assert_eq!(v, vec![Value::Variant("Result".into(), 0, vec![list])]);
    }

    #[test]
    fn error_is_passed_through() {
        let (mut s, _) = session(Err("no matches"));
        let v = s.run(r#"rg.search "x","#).unwrap();
        assert_eq!(
            v,
            vec![Value::Variant(
                "Result".into(),
                1,
                vec![Value::Str("no matches".into())]
            )]
        );
    }

    #[test]
    fn scope_block_drops_every_qualifier() {
        assert_eq!(
            argv(
                r#"rg.{
                    search "error" in "." { match (whole word), case (insensitive), context 2 },
                }"#
            ),
            expected(
                &["--word-regexp", "--ignore-case", "--context", "2"],
                &["error", "."]
            )
        );
    }

    #[test]
    fn scope_block_ends_with_its_brace() {
        let (mut s, _) = session(Ok(""));
        s.run(r#"rg.{ search "x", }"#).unwrap();
        let e = s.run(r#"search "x","#).unwrap_err();
        assert!(e.msg.contains("rg."), "{}", e.msg);
        let e = s
            .run(r#"search "x" in "." { match (whole word) },"#)
            .unwrap_err();
        assert!(e.msg.contains("rg."), "{}", e.msg);
    }

    #[test]
    fn bad_calls_are_type_errors() {
        let (mut s, _) = session(Ok(""));
        let e = s
            .run(r#"rg.search "" in "." { max count 1, max count 2 },"#)
            .unwrap_err();
        assert!(e.msg.contains("max count"), "{}", e.msg);
        let e = s
            .run(r#"rg.search "" in "." { max file size (5 days) },"#)
            .unwrap_err();
        assert!(e.msg.contains("Size"), "{}", e.msg);
        let e = s.run(r#"search "x","#).unwrap_err();
        assert!(e.msg.contains("rg."), "{}", e.msg);
    }
}

// ---------- `,` and `ns.{ ... }` ----------

mod statements {
    use super::*;

    #[test]
    fn newlines_are_whitespace() {
        assert_eq!(run("count\n[1,\n 2]\n,"), vec![Value::Int(2)]);
        assert_eq!(
            run("count [1, 2], count [3],"),
            vec![Value::Int(2), Value::Int(1)]
        );
    }

    #[test]
    fn statements_are_separated_by_commas() {
        assert_eq!(run("count [1]"), vec![Value::Int(1)]);
        for src in [
            "LET x : 1\nLET y : 2,",
            "FUNCTION one -> Int { 1 }\nFUNCTION two -> Int { 2 }",
        ] {
            let msg = run_err(src);
            assert!(msg.contains("`,`"), "{src}: {msg}");
        }
    }

    #[test]
    fn a_trailing_comma_is_fine_and_an_empty_element_is_not() {
        assert_eq!(run("FUNCTION one -> Int { 1 },\none,"), vec![Value::Int(1)]);
        let msg = run_err("FUNCTION one -> Int { 1 },\n,one,");
        assert!(msg.contains("remove the extra `,`"), "{msg}");
    }

    #[test]
    fn last_expression_of_a_body_is_its_value_without_a_return_type_it_is_dropped() {
        assert_eq!(
            run("FUNCTION f -> Int { LET x : 4, x },\nf,"),
            vec![Value::Int(4)]
        );
        assert_eq!(run("FUNCTION f { 4 },\nf,"), vec![]);
        let msg = run_err("FUNCTION f -> Int { LET x : 4 }");
        assert!(msg.contains("Int") && msg.contains("Unit"), "{msg}");
    }

    const ZZ: &str = "NAMESPACE zz,\nFUNCTION count (xs: List (Int)) -> Int { 99 }";

    #[test]
    fn scope_block_resolves_unqualified_and_namespace_wins() {
        let mut s = Session::new();
        s.run(ZZ).unwrap();
        assert_eq!(s.run("zz.{ count [1], }").unwrap(), vec![Value::Int(99)]);
        assert_eq!(s.run("count [1],").unwrap(), vec![Value::Int(1)]);
        assert_eq!(s.run("zz.count [1],").unwrap(), vec![Value::Int(99)]);
        // Names the namespace lacks still come from the prelude.
        assert_eq!(
            s.run("zz.{ count [1, 2, 3], }").unwrap(),
            vec![Value::Int(99)]
        );
        assert_eq!(
            s.run("zz.{ [1] . count _, }").unwrap(),
            vec![Value::Int(99)]
        );
        assert_eq!(
            s.run("zz.{ (5) to string, }").unwrap(),
            vec![Value::Str("5".into())]
        );
    }

    #[test]
    fn scope_block_inside_a_function_body_is_its_value() {
        let mut s = Session::new();
        s.run(ZZ).unwrap();
        let v = s
            .run("FUNCTION f -> Int { zz.{ count [1] } },\nf,")
            .unwrap();
        assert_eq!(v, vec![Value::Int(99)]);
    }

    #[test]
    fn scope_block_names_a_declared_namespace() {
        let msg = run_err("nope.{ count [1], }");
        assert!(msg.contains("unknown namespace `nope`"), "{msg}");
    }
}

// ---------- doc comments ----------

mod docs {
    use resh::Session;

    const SRC: &str = "## Adds one.\n## Second line.\nFUNCTION bump (n: Int) -> Int { n },\n\n# Detached by a blank line.\n\nFUNCTION lonely -> Int { 1 },\n\nx = 1, # trailing\nFUNCTION trailing_above -> Int { 2 },\n\n## A shape.\nRECORD Dot Doc { x (Int) },\n\n## Colours.\nENUM Hue Doc { red, blue },\n\nFUNCTION plain -> Int { 3 }\n";

    fn doc_of_fn(s: &Session, word: &str) -> Option<String> {
        s.interp
            .ck
            .defs
            .iter()
            .find(|d| matches!(d.parts.first(), Some(resh::check::PartT::Word(w)) if w == word))
            .unwrap_or_else(|| panic!("no function {word}"))
            .doc
            .clone()
    }

    #[test]
    fn doc_comments_attach_to_declarations() {
        let mut s = Session::new();
        let src = SRC.replace("x = 1, # trailing\n", "LET x : 1, # trailing\n");
        s.run(&src).unwrap();
        assert_eq!(
            doc_of_fn(&s, "bump").as_deref(),
            Some("Adds one.\nSecond line.")
        );
        assert_eq!(doc_of_fn(&s, "lonely"), None);
        assert_eq!(doc_of_fn(&s, "trailing_above"), None);
        assert_eq!(doc_of_fn(&s, "plain"), None);
        assert_eq!(
            s.interp.ck.records["Dot Doc"].doc.as_deref(),
            Some("A shape.")
        );
        assert_eq!(
            s.interp.ck.enums["Hue Doc"].doc.as_deref(),
            Some("Colours.")
        );
    }

    #[test]
    fn prelude_docs_are_captured() {
        let s = Session::new();
        let doc = s.interp.ck.records["List Options"]
            .doc
            .as_deref()
            .unwrap_or("");
        assert!(doc.starts_with("Mirrors `ls`"), "{doc}");
    }
}

// ---------- symbols ----------

mod symbols {
    use resh::Session;
    use resh::check::SymbolKind;

    #[test]
    fn global_listing_has_sort_key_and_list_options() {
        let s = Session::new();
        let syms = s.symbols(None).unwrap();
        let sort = syms
            .iter()
            .find(|x| x.kind == SymbolKind::Enum && x.name == "Sort Key")
            .expect("Sort Key");
        let variants: Vec<&str> = sort.members.iter().map(|m| m.signature.as_str()).collect();
        assert_eq!(
            variants,
            ["name", "size", "modified time", "extension", "unsorted"]
        );
        let opts = syms
            .iter()
            .find(|x| x.kind == SymbolKind::Record && x.name == "List Options")
            .expect("List Options");
        assert!(opts.doc.as_deref().unwrap().contains("Mirrors `ls`"));
        let sort_by = opts.members.last().unwrap();
        assert_eq!(sort_by.signature, "sort by (Sort Key)");
        assert_eq!(sort_by.default, [Some("name".to_string())]);
        assert_eq!(opts.members[0].default, [Some("false".to_string())]);
        let first_fn = syms
            .iter()
            .position(|x| x.kind == SymbolKind::Function)
            .unwrap();
        assert!(
            syms[..first_fn]
                .iter()
                .all(|x| x.kind != SymbolKind::Function)
        );
        assert!(
            syms[first_fn..]
                .iter()
                .all(|x| x.kind == SymbolKind::Function)
        );
        let list = syms
            .iter()
            .find(|x| x.name == "list file in (dir: String) (options: List Options)")
            .expect("list file in");
        assert_eq!(list.ret.as_deref(), Some("Result (List (Entry))"));
        assert!(syms.iter().any(|x| x.name == "Option (T)"));
        assert!(syms.iter().any(|x| x.name == "Result (T) Or (E)"));
    }

    #[test]
    fn namespaced_listing() {
        let mut s = Session::new();
        s.run(
            "NAMESPACE fd,\n## Search mode.\nENUM Mode { regex, glob },\n## Knobs.\nRECORD Options { port (Int DEFAULT 8080) },\n## Finds.\nFUNCTION find (root: String) with (options: Options) -> Int { 1 }\n",
        )
        .unwrap();
        let syms = s.symbols(Some("fd")).unwrap();
        let shown: Vec<(SymbolKind, &str)> =
            syms.iter().map(|x| (x.kind, x.name.as_str())).collect();
        assert_eq!(
            shown,
            [
                (SymbolKind::Enum, "Mode"),
                (SymbolKind::Record, "Options"),
                (
                    SymbolKind::Function,
                    "find (root: String) with (options: Options)"
                ),
            ]
        );
        assert_eq!(syms[0].doc.as_deref(), Some("Search mode."));
        assert_eq!(syms[1].members[0].signature, "port (Int)");
        assert_eq!(syms[1].members[0].default, [Some("8080".to_string())]);
        assert_eq!(syms[2].ret.as_deref(), Some("Int"));
        assert!(!s.symbols(None).unwrap().iter().any(|x| x.name == "Mode"));
    }

    #[test]
    fn unknown_namespace() {
        let mut s = Session::new();
        assert_eq!(
            s.symbols(Some("x")).unwrap_err(),
            "unknown namespace `x`; declared namespaces: none"
        );
        s.run("NAMESPACE b,\nFUNCTION f -> Int { 1 }").unwrap();
        s.run("NAMESPACE a,\nFUNCTION g -> Int { 1 }").unwrap();
        assert_eq!(
            s.symbols(Some("x")).unwrap_err(),
            "unknown namespace `x`; declared namespaces: a, b"
        );
    }
}

mod symbols_cli {
    use std::io::Write;
    use std::process::{Command, Stdio};

    /// Runs the real binary with `input` on stdin; returns (stdout, stderr).
    fn repl(args: &[&str], input: &str) -> (String, String) {
        repl_with_init(args, input, "/nonexistent/init.resh")
    }

    fn repl_with_init(args: &[&str], input: &str, init: &str) -> (String, String) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_resh"))
            .args(args)
            .env("RESH_INIT", init)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    }

    #[test]
    fn init_file_runs_before_the_first_entry() {
        let dir = std::env::temp_dir().join(format!("resh-init-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let init = dir.join("init.resh");
        std::fs::write(&init, "IMPORT git,\n").unwrap();
        let (out, err) = repl_with_init(&[], ":symbols\n", init.to_str().unwrap());
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(err, "");
        assert_eq!(out, "namespaces\n  prelude\n  git\n");
    }

    // `NAMESPACE` is per REPL entry, so the declaration shares its entry.
    const DECL: &str = "NAMESPACE demo, ENUM Mode { fast, slow }\n";

    #[test]
    fn text_listing_for_a_namespace() {
        let (out, _) = repl(&[], &format!("{DECL}:symbols demo\n"));
        assert_eq!(
            out,
            "demo\n  enum    Mode\n            fast\n            slow\n"
        );
    }

    #[test]
    fn json_listing_for_a_namespace() {
        let (out, _) = repl(&["--json"], &format!("{DECL}:symbols demo\n"));
        assert!(out.starts_with(
            "{\"namespace\":\"demo\",\"symbols\":[{\"kind\":\"enum\",\"name\":\"Mode\""
        ));
        assert!(out.contains("\"signature\":\"fast\"") && out.contains("\"signature\":\"slow\""));
    }

    #[test]
    fn bare_symbols_lists_namespaces() {
        let (out, _) = repl(&[], &format!("{DECL}:symbols\n"));
        assert_eq!(out, "namespaces\n  prelude\n  demo\n");
        let (out, _) = repl(&["--json"], &format!("{DECL}:symbols\n"));
        assert_eq!(out, "{\"namespaces\":[\"prelude\",\"demo\"]}\n");
    }

    #[test]
    fn unknown_namespace_goes_to_stderr() {
        let (out, err) = repl(&[], &format!("{DECL}:symbols nope\n"));
        assert_eq!(out, "");
        assert!(
            err.contains("unknown namespace `nope`") && err.contains("demo"),
            "{err}"
        );
    }

    #[test]
    fn doc_comment_lines_are_one_entry_with_their_declaration() {
        let (out, _) = repl(
            &[],
            "## A point.\nRECORD Pt { x (Int) }\n:symbols prelude\n",
        );
        assert!(out.contains("  /// A point.\n  record  Pt\n"), "{out}");
    }
}

// ---------- recursive patterns and matrix exhaustiveness ----------

mod recursive_patterns {
    use super::{run, run_err};
    use resh::Value;

    const EXPR: &str = r#"
ENUM Color { red, green },
ENUM Wrap { w (Color), pair (Color) and (Color), deep (Wrap) },
"#;

    fn with(body: &str) -> String {
        format!("{EXPR}\nFUNCTION f (x: Wrap) -> Int {{ MATCH (x) {{ {body} }} }},\n")
    }

    #[test]
    fn nests_to_any_depth_and_binds_inside() {
        let src = format!(
            "{}\nf (deep (deep (w (green)))),",
            with(
                "w (red) => 1, w (green) => 2, pair (a) and (b) => 3, deep (deep (w (red))) => 4, deep (deep (w (green))) => 5, deep (_) => 6"
            )
        );
        assert_eq!(run(&src).last(), Some(&Value::Int(5)));
        let src = format!(
            "{}\nf (deep (w (red))),",
            with(
                "w (_) => 1, pair (a) and (b) => 3, deep (deep (_)) => 4, deep (w (_)) => 7, deep (pair (_) and (_)) => 8"
            )
        );
        assert_eq!(run(&src).last(), Some(&Value::Int(7)));
    }

    #[test]
    fn two_nested_slots_in_one_arm_are_checked_as_a_matrix() {
        let arms = "w (_) => 0, deep (_) => 0, pair (red) and (red) => 1, pair (green) and (green) => 2, pair (red) and (green) => 3, pair (green) and (red) => 4";
        let src = format!("{}\nf (pair (green) and (red)),", with(arms));
        assert_eq!(run(&src).last(), Some(&Value::Int(4)));
        let missing = "w (_) => 0, deep (_) => 0, pair (red) and (red) => 1, pair (green) and (green) => 2, pair (red) and (green) => 3";
        let msg = run_err(&with(missing));
        assert!(
            msg.contains("non-exhaustive MATCH; missing: pair (green) and (red)"),
            "{msg}"
        );
    }

    #[test]
    fn missing_cases_name_the_deepest_hole() {
        let msg = run_err(&with(
            "w (_) => 0, pair (_) and (_) => 0, deep (deep (w (red))) => 1",
        ));
        assert!(
            msg.contains("deep (w (_))") && msg.contains("deep (pair (_) and (_))"),
            "{msg}"
        );
    }

    #[test]
    fn unreachable_arms_are_rejected() {
        let msg = run_err(&with(
            "w (_) => 0, pair (_) and (_) => 0, deep (_) => 0, deep (w (red)) => 1",
        ));
        assert!(msg.contains("unreachable"), "{msg}");
        let msg = run_err(&with(
            "w (_) => 0, pair (_) and (_) => 0, deep (_) => 0, _ => 1",
        ));
        assert!(msg.contains("unreachable"), "{msg}");
        let msg = run_err(&with("_ => 0, w (red) => 1"));
        assert!(msg.contains("unreachable"), "{msg}");
    }

    #[test]
    fn a_wildcard_arm_after_partial_arms_is_exhaustive() {
        let src = format!(
            "{}\nf (w (green)),",
            with("w (red) => 1, deep (deep (_)) => 2, _ => 3")
        );
        assert_eq!(run(&src).last(), Some(&Value::Int(3)));
    }

    #[test]
    fn a_non_enum_slot_cannot_hold_a_nested_pattern() {
        let src = "ENUM Num { num (Int) },\nFUNCTION f (x: Num) -> Int { MATCH (x) { num (k (j)) => 1 } }";
        let msg = run_err(src);
        assert!(msg.contains("not an enum"), "{msg}");
    }
}

mod record_patterns_in_the_matrix {
    use super::{run, run_err};
    use resh::Value;

    const DECLS: &str = r#"
ENUM Color { red, green },
RECORD Dot { color (Color), size (Int) },
ENUM Shape { dot (Dot), blank },
"#;

    fn with(body: &str) -> String {
        format!(
            "{DECLS}\nFUNCTION f (x: Shape) -> Int {{ MATCH (x) {{ {body} }} }},\nLET (d : Dot) : {{ color (red), size (7) }},\n"
        )
    }

    #[test]
    fn a_record_pattern_nests_inside_an_enum_pattern() {
        let src = format!(
            "{}f (dot (d)),",
            with(
                "dot ({ color (red), size (n) }) => n, dot ({ color (green), size (_) }) => 0, blank => 9"
            )
        );
        assert_eq!(run(&src).last(), Some(&Value::Int(7)));
    }

    #[test]
    fn missing_cases_inside_a_record_are_reported() {
        let msg = run_err(&with("dot ({ color (red), size (n) }) => n, blank => 9"));
        assert!(
            msg.contains("missing: dot ({ color (green), size (_) })"),
            "{msg}"
        );
    }

    #[test]
    fn refutable_record_arms_can_be_followed_by_more_arms() {
        let src = "ENUM Color { red, green },\nRECORD Dot { color (Color), size (Int) },\nFUNCTION f (d: Dot) -> Int { MATCH (d) { { color (red), size (n) } => n, { color (green), size (_) } => 0 } },\nLET (d : Dot) : { color (green), size (3) },\nf (d),";
        assert_eq!(run(src).last(), Some(&Value::Int(0)));
        let msg = run_err(
            "ENUM Color { red, green },\nRECORD Dot { color (Color), size (Int) },\nFUNCTION f (d: Dot) -> Int { MATCH (d) { { color (red), size (n) } => n } }",
        );
        assert!(
            msg.contains("missing: { color (green), size (_) }"),
            "{msg}"
        );
    }

    #[test]
    fn a_wildcard_matches_a_record() {
        let src = "RECORD Dot { size (Int) },\nFUNCTION f (d: Dot) -> Int { MATCH (d) { _ => 1 } },\nLET (d : Dot) : { size (3) },\nf (d),";
        assert_eq!(run(src).last(), Some(&Value::Int(1)));
    }
}

// ---------- lib/sed.resh end to end ----------

mod sed_example {
    use resh::Session;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Calls = Rc<RefCell<Vec<(String, Vec<String>)>>>;

    const SED: &str = include_str!("../lib/sed.resh");

    fn argv(src: &str) -> Vec<String> {
        let calls: Calls = Rc::default();
        let seen = calls.clone();
        let mut s = Session::new();
        s.set_runner(Box::new(move |c, a| {
            seen.borrow_mut().push((c.to_string(), a.to_vec()));
            Ok(String::new())
        }));
        s.run(SED)
            .unwrap_or_else(|d| panic!("sed.resh failed: {d}"));
        s.run(src).unwrap_or_else(|d| panic!("{d}"));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "sed");
        calls[0].1.clone()
    }

    fn expected(scripts: &[&str]) -> Vec<String> {
        let mut v = vec!["-E".to_string()];
        for s in scripts {
            v.push("-e".into());
            v.push(s.to_string());
        }
        v.push("f.txt".into());
        v
    }

    #[test]
    fn replace_all_on_every_line() {
        assert_eq!(
            argv(
                r#"sed.{ preview "f.txt" with [ in (every line) do (replace all "a/b" with "c") ] },"#
            ),
            expected(&["s\ta/b\tc\tg"])
        );
    }

    #[test]
    fn scopes_become_addresses() {
        assert_eq!(
            argv(
                r#"sed.{ preview "f.txt" with [
                    in (from line 1 to line 10) do (replace first "x" with "y"),
                    in (matching "^#") do (delete line),
                    in (last line) do (delete line),
                    in (line 3) do (delete line)
                ] },"#
            ),
            expected(&["1,10s\tx\ty\t", "\\\t^#\td", "$d", "3d"])
        );
    }
}

#[test]
fn unknown_variable_suggests_closest_name() {
    let msg = run_err("LET counter : 1,\ncount (countr),");
    assert!(msg.contains("Did you mean: `counter`"), "{msg}");
}

#[test]
fn unknown_function_word_suggests_closest_word() {
    let msg = run_err("[1, 2] cout,");
    assert!(
        msg.contains("For `cout`:") && msg.contains("`count"),
        "{msg}"
    );
}

// ---------- forms the anyfix notation enables ----------

mod anyfix_forms {
    use super::{run, run_err};
    use resh::Value;

    #[test]
    fn if_chains_to_the_right_with_colon() {
        let sign = "FUNCTION sign (n: Int) -> String {\n    IF ((n) equals 0) THEN \"zero\" ELSE : IF ((n) equals 1) THEN \"one\" ELSE : \"many\"\n},\n";
        assert_eq!(
            run(&format!("{sign}sign 0, sign 1, sign 5,")),
            vec![
                Value::Str("zero".into()),
                Value::Str("one".into()),
                Value::Str("many".into())
            ]
        );
    }

    #[test]
    fn dot_hands_its_value_to_match_and_to_the_placeholder() {
        let src = "ENUM Light { red, green },\nLET l : green,\nl . MATCH { red => 1, green => 2 },\n[\"a\"] . join _ with [\"b\"],";
        assert_eq!(
            run(src),
            vec![
                Value::Int(2),
                Value::List(vec![Value::Str("a".into()), Value::Str("b".into())])
            ]
        );
    }

    #[test]
    fn as_states_a_type() {
        let src = "RECORD Apple { x (Int) },\nFUNCTION f (a: Apple) -> Int { 1 },\nf ({ x 1 } AS (Apple)),";
        assert_eq!(run(src), vec![Value::Int(1)]);
    }

    #[test]
    fn a_keyword_inside_a_call_points_at_the_missing_comma() {
        let msg = run_err("LET x : 1\nLET y : 2");
        assert!(msg.contains("is a `,` missing before it?"), "{msg}");
    }

    #[test]
    fn a_variable_fills_a_slot_in_parentheses() {
        assert_eq!(
            run("LET n : 4,\n(n) to string,"),
            vec![Value::Str("4".into())]
        );
    }
}
