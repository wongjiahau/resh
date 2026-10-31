// ---------- lib/git.resh end to end ----------

use resh::Session;
use std::cell::RefCell;
use std::rc::Rc;

const GIT: &str = include_str!("../lib/git.resh");

/// Every `run` call: the command and its argv.
type Calls = Rc<RefCell<Vec<(String, Vec<String>)>>>;

fn argv(src: &str) -> Vec<String> {
    let calls: Calls = Rc::default();
    let seen = calls.clone();
    let mut s = Session::new();
    s.set_runner(Box::new(move |c, a| {
        seen.borrow_mut().push((c.to_string(), a.to_vec()));
        Ok(String::new())
    }));
    s.run(GIT)
        .unwrap_or_else(|d| panic!("git.resh failed: {d}"));
    s.run(src).unwrap_or_else(|d| panic!("{d}"));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "git");
    calls[0].1.clone()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn status_plain() {
    assert_eq!(argv("git.status,"), strs(&["status", "--porcelain=v1"]));
}

#[test]
fn status_options() {
    assert_eq!(
        argv("git.{ status { untracked (all), branch } },"),
        strs(&[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--branch"
        ])
    );
}

#[test]
fn log_options_and_paths() {
    assert_eq!(
        argv(r#"git.{ log { max count 5, author "alice", no merge } touching ["src"] },"#),
        strs(&[
            "log",
            "--format=%h %s",
            "--max-count",
            "5",
            "--author",
            "alice",
            "--no-merges",
            "--",
            "src"
        ])
    );
}

#[test]
fn log_since_uses_duration_units() {
    assert_eq!(
        argv("git.{ log { changed within (7 days) } },"),
        strs(&["log", "--format=%h %s", "--since=604800 seconds ago", "--"])
    );
}

#[test]
fn diff_staged_stat() {
    assert_eq!(
        argv("git.{ diff { staged, stat } },"),
        strs(&["diff", "--cached", "--stat", "--"])
    );
}

#[test]
fn diff_against_revision_for_paths() {
    assert_eq!(
        argv(r#"git.{ diff { against "HEAD~3", name only } touching ["src", "tests"] },"#),
        strs(&["diff", "HEAD~3", "--name-only", "--", "src", "tests"])
    );
}
