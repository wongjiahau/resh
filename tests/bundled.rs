use resh::{BUNDLED, Session};

#[test]
fn every_bundled_wrapper_type_checks_in_a_fresh_session() {
    for (ns, src) in BUNDLED {
        Session::new()
            .check_only(src)
            .unwrap_or_else(|d| panic!("{ns}: {}:{}: {}", d.span.line, d.span.col, d.msg));
    }
}

#[test]
fn check_only_does_not_execute_statements() {
    let mut s = Session::new();
    s.set_runner(Box::new(|_, _| panic!("check must not run commands")));
    s.check_only("run (\"echo\", []),").unwrap();
}

#[test]
fn check_only_reports_type_errors() {
    assert!(
        Session::new()
            .check_only("FUNCTION : f () -> Int = { \"x\" }")
            .is_err()
    );
}
