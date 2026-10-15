use resh::Session;

fn session() -> Session {
    let mut s = Session::new();
    s.set_library_dir(None);
    s
}

#[test]
fn import_loads_a_bundled_library() {
    let mut s = session();
    s.set_runner(Box::new(|c, a| Ok(format!("{c} {}", a.join(" ")))));
    let out = s.run("IMPORT git;\ngit::status;").unwrap();
    assert_eq!(out.len(), 1);
    assert!(s.namespaces().contains(&"git".to_string()));
}

#[test]
fn import_twice_is_fine() {
    let mut s = session();
    s.run("IMPORT git;").unwrap();
    s.run("IMPORT git;").unwrap();
}

#[test]
fn namespace_stays_unloaded_without_import() {
    assert!(!session().namespaces().contains(&"git".to_string()));
}

#[test]
fn unknown_library_lists_the_available_ones() {
    let err = session().run("IMPORT nope;").unwrap_err();
    assert!(err.msg.contains("no library `nope`"), "{}", err.msg);
    assert!(err.msg.contains("git"), "{}", err.msg);
}

#[test]
fn import_works_in_a_file_with_a_namespace() {
    let mut s = session();
    s.run("NAMESPACE mine\nIMPORT git\nFUNCTION hello: String { \"hi\" }")
        .unwrap();
    assert!(s.namespaces().contains(&"git".to_string()));
}

#[test]
fn user_library_wins_over_bundled_and_adds_new_ones() {
    let dir = std::env::temp_dir().join(format!("resh-lib-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("tool.resh"),
        "NAMESPACE tool\nFUNCTION hello: String { \"hi\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("git.resh"),
        "NAMESPACE git\nFUNCTION mine: Int { 1 }\n",
    )
    .unwrap();
    let mut s = Session::new();
    s.set_library_dir(Some(dir.clone()));
    s.run("IMPORT tool;\nIMPORT git;").unwrap();
    assert_eq!(s.run("tool::hello;").unwrap().len(), 1);
    assert_eq!(s.run("git::mine;").unwrap().len(), 1);
    assert!(s.run("git::status;").is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn library_with_the_wrong_namespace_is_rejected() {
    let dir = std::env::temp_dir().join(format!("resh-lib-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bad.resh"), "NAMESPACE other\n").unwrap();
    let mut s = Session::new();
    s.set_library_dir(Some(dir.clone()));
    let err = s.run("IMPORT bad;").unwrap_err();
    assert!(
        err.msg.contains("must start with `NAMESPACE bad`"),
        "{}",
        err.msg
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn library_names_cannot_escape_the_directory() {
    assert!(resh::library::find("../x", None).is_none());
}

#[test]
fn syntax_doc_and_skill_are_embedded_and_nonempty() {
    assert!(include_str!("../SYNTAX.md").contains("## Importing libraries"));
    assert!(
        include_str!("../.claude/skills/new-wrapper/SKILL.md")
            .starts_with("---\nname: new-wrapper")
    );
}

#[test]
fn mixed_case_library_name_suggests_the_lowercase_one() {
    let err = session().run("IMPORT Git;").unwrap_err();
    assert!(err.msg.contains("Did you mean `git`?"), "{}", err.msg);
}

#[test]
fn raw_unwraps_results_and_unescapes_strings() {
    let mut s = session();
    s.set_runner(Box::new(|_, _| Ok("a\nb\n".to_string())));
    let mut seen = Vec::new();
    s.run_with(
        "run (\"x\", []);\nrun (\"x\", []) |> map (_) with ((t) => (t) lines);",
        &mut |i, v| seen.push(i.raw(v)),
    )
    .unwrap();
    assert_eq!(seen[0], (true, "a\nb\n".to_string()));
    assert_eq!(seen[1], (true, "a\nb".to_string()));
}
