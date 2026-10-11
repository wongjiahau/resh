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
    name: String,
    retries?: Int,
    dry run?: Boolean
}
LET j = Job { name: "build", dry run }
(dry run) j
"#;
    // `dry run` is a prefix field: `dry run (j)` -- `(dry run) j` must not parse.
    assert!(Session::new().run(src).is_err());

    let src = r#"
RECORD Job {
    name: String,
    retries?: Int,
    dry run?: Boolean
}
LET j = Job { name: "build", dry run }
dry run j
retries j
name j
"#;
    assert_eq!(
        render(src),
        "\"build\"",
        "plain fields are prefix functions: `name j`"
    );
    let vs = run(src);
    assert_eq!(
        vs[0],
        Value::Variant("Option".into(), 0, vec![Value::Bool(true)])
    );
    assert_eq!(vs[1], Value::Variant("Option".into(), 1, vec![]));
}

#[test]
fn self_field_can_be_suffix_and_infix() {
    let src = r#"
RECORD Person {
    (self) 's name: String,
    age of (self): Int
}
LET p = Person { 's name: "Ada", age of: 36 }
(p) 's name
age of p
"#;
    assert_eq!(run(src), vec![Value::Str("Ada".into()), Value::Int(36)]);
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
RECORD Bag { items: List (Int) }
RECORD Pair { left: Int, right: Int }
RECORD Triple { left: Int, right: Int, extra: Int }
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
        "{OVERLOADS}\ndescribe 1\ndescribe true\ndescribe [\"a\"]\ndescribe []\ndescribe (Bag {{ items: [1] }})"
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
    let src =
        format!("{OVERLOADS}\nsum {{ left: 1, right: 2 }}\nsum {{ left: 1, right: 2, extra: 3 }}");
    assert_eq!(run(&src), vec![Value::Int(1), Value::Int(3)]);
}

#[test]
fn ambiguous_record_literal_needs_an_annotation() {
    let src = r#"
RECORD Apple { x: Int }
RECORD Banana { x: Int }
FUNCTION f (a: Apple): Int { 1 }
FUNCTION f (b: Banana): Int { 2 }
"#;
    let msg = run_err(&format!("{src}\nf {{ x: 1 }}"));
    assert!(msg.contains("ambiguous call"), "{msg}");
    assert!(msg.contains("{ ... } : Record Name"), "{msg}");
    assert_eq!(
        run(&format!("{src}\nf ({{ x: 1 }} : Banana)")),
        vec![Value::Int(2)]
    );
}

#[test]
fn bare_record_literal_has_no_type_of_its_own() {
    let msg = run_err("LET x = { a: 1 }");
    assert!(msg.contains("annotate it"), "{msg}");
    let msg = run_err("RECORD Apple { x: Int }\nLET x = { y: 1 } : Apple");
    assert!(msg.contains("has no field `y`"), "{msg}");
}

#[test]
fn optional_fields_need_explicit_some() {
    let src = "RECORD Job { name: String, retries?: Int }\nLET j = Job { name: \"b\", retries: some 3 }\nretries j";
    assert_eq!(
        run(src),
        vec![Value::Variant("Option".into(), 0, vec![Value::Int(3)])]
    );
    let msg =
        run_err("RECORD Job { name: String, retries?: Int }\nJob { name: \"b\", retries: 3 }");
    assert!(msg.contains("explicit `some"), "{msg}");
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
    assert_eq!(names("{ sort by: some size }")[0], "big.txt");
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
