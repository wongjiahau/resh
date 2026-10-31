// ---------- lib/herdr.resh end to end ----------

use resh::Session;
use std::cell::RefCell;
use std::rc::Rc;

const HERDR: &str = include_str!("../lib/herdr.resh");

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
    s.run(HERDR)
        .unwrap_or_else(|d| panic!("herdr.resh failed: {d}"));
    s.run(src).unwrap_or_else(|d| panic!("{d}"));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "herdr");
    calls[0].1.clone()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn list_agent() {
    assert_eq!(argv("herdr.list agent,"), strs(&["agent", "list"]));
}

#[test]
fn list_pane_in_workspace() {
    assert_eq!(
        argv(r#"herdr.{ list pane in workspace "w2" },"#),
        strs(&["pane", "list", "--workspace", "w2"])
    );
}

#[test]
fn read_pane_plain() {
    assert_eq!(
        argv(r#"herdr.{ read pane "w1:p1" },"#),
        strs(&["pane", "read", "w1:p1"])
    );
}

#[test]
fn read_pane_options() {
    assert_eq!(
        argv(r#"herdr.{ read pane "w1:p1" { lines 50, source : recent unwrapped, ansi } },"#),
        strs(&[
            "pane",
            "read",
            "w1:p1",
            "--lines",
            "50",
            "--source",
            "recent-unwrapped",
            "--format",
            "ansi"
        ])
    );
}

#[test]
fn create_workspace_options() {
    assert_eq!(
        argv(r#"herdr.{ create workspace { label "demo", cwd "/tmp", no focus } },"#),
        strs(&[
            "workspace",
            "create",
            "--label",
            "demo",
            "--cwd",
            "/tmp",
            "--no-focus"
        ])
    );
}

#[test]
fn split_pane_direction() {
    assert_eq!(
        argv(r#"herdr.{ split pane "w1:p1" (right) { keep focus, start in "/tmp" } },"#),
        strs(&[
            "pane",
            "split",
            "w1:p1",
            "--direction",
            "right",
            "--no-focus",
            "--cwd",
            "/tmp"
        ])
    );
}

#[test]
fn run_in_pane() {
    assert_eq!(
        argv(r#"herdr.{ run "just test" in pane "w1:p2" },"#),
        strs(&["pane", "run", "w1:p2", "just test"])
    );
}

#[test]
fn wait_for_output_timeout_is_milliseconds() {
    assert_eq!(
        argv(r#"herdr.{ wait for output "ok" in pane "w1:p2" { timeout : 120 seconds } },"#),
        strs(&[
            "pane",
            "wait-output",
            "w1:p2",
            "--match",
            "ok",
            "--timeout",
            "120000"
        ])
    );
}

#[test]
fn start_agent() {
    assert_eq!(
        argv(r#"herdr.{ start agent "reviewer" (codex) in pane "w1:p3" },"#),
        strs(&[
            "agent", "start", "reviewer", "--kind", "codex", "--pane", "w1:p3"
        ])
    );
}

#[test]
fn prompt_agent_and_wait() {
    assert_eq!(
        argv(
            r#"herdr.{ prompt agent "reviewer" with "go" { wait, give up after : 30 seconds } },"#
        ),
        strs(&[
            "agent",
            "prompt",
            "reviewer",
            "go",
            "--wait",
            "--timeout",
            "30000"
        ])
    );
}

#[test]
fn wait_for_agent_settled() {
    assert_eq!(
        argv(r#"herdr.{ wait for agent "reviewer" },"#),
        strs(&["agent", "wait", "reviewer"])
    );
}

#[test]
fn wait_for_agent_until_blocked() {
    assert_eq!(
        argv(r#"herdr.{ prompt agent "reviewer" with "go" { until (blocked) } },"#),
        strs(&["agent", "prompt", "reviewer", "go", "--until", "blocked"])
    );
}
