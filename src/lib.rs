pub mod ast;
pub mod check;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod repl;

pub use eval::Value;
pub use lexer::{Diag, Span};

use ast::{Decl, Item, Stmt};
use check::Scope;
use eval::Interp;

const PRELUDE: &str = include_str!("prelude.resh");

/// A persistent interpreter session: declarations and `LET` bindings carry
/// over between calls to `run`, which is what the REPL builds on.
pub struct Session {
    pub interp: Interp,
    globals: Scope,
    env: Vec<(String, Value)>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    pub fn new() -> Session {
        let mut s = Session::bare();
        s.run(PRELUDE).expect("prelude must be valid");
        s
    }

    fn bare() -> Session {
        Session {
            interp: Interp::new(),
            globals: Vec::new(),
            env: Vec::new(),
        }
    }

    /// Runs `src`, calling `on_value` with every non-Unit top-level expression result.
    pub fn run_with(
        &mut self,
        src: &str,
        on_value: &mut dyn FnMut(&Interp, &Value),
    ) -> Result<(), Diag> {
        let items = parser::parse_program(src)?;
        let decls: Vec<&Decl> = items
            .iter()
            .filter_map(|i| if let Item::Decl(d) = i { Some(d) } else { None })
            .collect();
        // A file has one namespace; calls in it resolve against that file's.
        let ns = decls.iter().find_map(|d| match d {
            Decl::Namespace { name, .. } => Some(name.clone()),
            _ => None,
        });
        self.interp.ck.set_namespace(ns);
        let pending = self.interp.ck.declare(&decls)?;
        self.interp.ck.check_bodies(pending)?;
        for item in &items {
            let Item::Stmt(stmt) = item else { continue };
            match stmt {
                Stmt::Let(name, e, span) => {
                    let (ir, ty) = self.interp.ck.check(e, &mut self.globals)?;
                    let v = self
                        .interp
                        .eval(&ir, &mut self.env)
                        .map_err(|m| runtime(*span, m))?;
                    self.globals.push((name.clone(), ty));
                    self.env.push((name.clone(), v));
                }
                Stmt::Expr(e) => {
                    let (ir, _) = self.interp.ck.check(e, &mut self.globals)?;
                    let v = self
                        .interp
                        .eval(&ir, &mut self.env)
                        .map_err(|m| runtime(e.span, m))?;
                    if v != Value::Unit {
                        on_value(&self.interp, &v);
                    }
                }
            }
        }
        Ok(())
    }

    /// Replaces the implementation of `run` (see `eval::Runner`). The runner
    /// receives the command and its argv; `Ok` is stdout, `Err` the error text.
    /// Intended for tests and embedding: mock `run` and assert the argv.
    pub fn set_runner(&mut self, runner: eval::Runner) {
        self.interp.set_runner(runner);
    }

    /// Convenience for tests: collects the non-Unit results.
    pub fn run(&mut self, src: &str) -> Result<Vec<Value>, Diag> {
        let mut out = Vec::new();
        self.run_with(src, &mut |_, v| out.push(v.clone()))?;
        Ok(out)
    }

    pub fn render(&self, v: &Value, json: bool) -> String {
        self.interp.render(v, json)
    }
}

fn runtime(span: Span, msg: String) -> Diag {
    Diag::new(span, format!("runtime error: {msg}"))
}
