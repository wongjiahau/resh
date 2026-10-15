pub mod ast;
pub mod check;
pub mod eval;
pub mod lexer;
pub mod library;
pub mod parser;
pub mod pattern;
pub mod repl;

pub use eval::Value;
pub use lexer::{Diag, Span};

use ast::{Decl, Item, Stmt};
use check::Scope;
use eval::Interp;

const PRELUDE: &str = include_str!("../lib/prelude.resh");

/// Wrapper libraries shipped inside the binary, as `(namespace, source)`.
pub const BUNDLED: &[(&str, &str)] = &[
    ("fd", include_str!("../lib/fd.resh")),
    ("git", include_str!("../lib/git.resh")),
    ("herdr", include_str!("../lib/herdr.resh")),
    ("rg", include_str!("../lib/rg.resh")),
    ("sed", include_str!("../lib/sed.resh")),
];

/// A persistent interpreter session: declarations and `LET` bindings carry
/// over between calls to `run`, which is what the REPL builds on.
pub struct Session {
    pub interp: Interp,
    globals: Scope,
    env: Vec<(String, Value)>,
    /// Where `IMPORT` looks before the bundled libraries; see `library::user_dir`.
    lib_dir: Option<std::path::PathBuf>,
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
            lib_dir: library::user_dir(),
        }
    }

    /// Parses `src` and type-checks its declarations (signatures and bodies)
    /// without executing anything. Top-level statements are not checked.
    pub fn check_only(&mut self, src: &str) -> Result<(), Diag> {
        self.declare_all(src).map(|_| ())
    }

    /// Overrides the user library directory (`None`: only bundled libraries).
    pub fn set_library_dir(&mut self, dir: Option<std::path::PathBuf>) {
        self.lib_dir = dir;
    }

    /// Loads library `name` once: its declarations join the session as namespace
    /// `name`. Its top-level statements, if any, are not run.
    fn import(&mut self, name: &str, span: Span) -> Result<(), Diag> {
        if self.interp.ck.namespace_names().iter().any(|n| n == name) {
            return Ok(());
        }
        let Some((_, src)) = library::find(name, self.lib_dir.as_deref()) else {
            let names: Vec<String> = library::available(self.lib_dir.as_deref())
                .into_iter()
                .map(|(n, _)| n)
                .collect();
            let hint = names
                .iter()
                .find(|n| n.eq_ignore_ascii_case(name))
                .map_or(String::new(), |n| {
                    format!(" Did you mean `{n}`? Library names are lowercase.")
                });
            return Err(Diag::new(
                span,
                format!(
                    "no library `{name}`; available: {}.{hint} Put `{name}.resh` in {} to add your own",
                    names.join(", "),
                    self.lib_dir
                        .as_deref()
                        .map_or("the resh library directory".into(), |d| d
                            .display()
                            .to_string()),
                ),
            ));
        };
        let in_library = |d: Diag| Diag::new(d.span, format!("in library `{name}`: {}", d.msg));
        let items = parser::parse_program(&src).map_err(in_library)?;
        let declares = items
            .iter()
            .any(|i| matches!(i, Item::Decl(Decl::Namespace { name: n, .. }) if n == name));
        if !declares {
            return Err(Diag::new(
                span,
                format!("library `{name}` must start with `NAMESPACE {name}`"),
            ));
        }
        // Register the namespace first so a library cannot import itself forever.
        self.interp.ck.set_namespace(Some(name.to_string()));
        self.declare_items(&items).map_err(in_library)?;
        Ok(())
    }

    /// Parses `src` and declares and checks its declarations; returns its statements.
    fn declare_all(&mut self, src: &str) -> Result<Vec<Item>, Diag> {
        let items = parser::parse_program(src)?;
        self.declare_items(&items)?;
        Ok(items)
    }

    /// Loads the imports of `items`, then declares and checks its declarations
    /// in the file's namespace.
    fn declare_items(&mut self, items: &[Item]) -> Result<(), Diag> {
        for item in items {
            if let Item::Import(name, span) = item {
                self.import(name, *span)?;
            }
        }
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
        Ok(())
    }

    /// Runs `src`, calling `on_value` with every non-Unit top-level expression result.
    pub fn run_with(
        &mut self,
        src: &str,
        on_value: &mut dyn FnMut(&Interp, &Value),
    ) -> Result<(), Diag> {
        let items = self.declare_all(src)?;
        for item in &items {
            let Item::Stmt(stmt) = item else { continue };
            self.exec(stmt, on_value)?;
        }
        Ok(())
    }

    fn exec(&mut self, stmt: &Stmt, on_value: &mut dyn FnMut(&Interp, &Value)) -> Result<(), Diag> {
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
            Stmt::Expr(e) | Stmt::Discard(e) => {
                let (ir, _) = self.interp.ck.check(e, &mut self.globals)?;
                let v = self
                    .interp
                    .eval(&ir, &mut self.env)
                    .map_err(|m| runtime(e.span, m))?;
                if v != Value::Unit {
                    on_value(&self.interp, &v);
                }
            }
            Stmt::Scope(ns, inner, span) => {
                self.interp.ck.open_scope(ns, *span)?;
                let result = inner.iter().try_for_each(|s| self.exec(s, on_value));
                self.interp.ck.close_scope();
                result?;
            }
        }
        Ok(())
    }

    /// Replaces the implementation of `run` (see `eval::Runner`). The runner
    /// receives the command and its argv; `Ok` is stdout, `Err` the error text.
    /// Intended for tests and embedding: mock `run` and assert the argv.
    pub fn set_runner(&mut self, runner: eval::Runner) {
        let _previous = self.interp.set_runner(runner);
    }

    /// Convenience for tests: collects the non-Unit results.
    pub fn run(&mut self, src: &str) -> Result<Vec<Value>, Diag> {
        let mut out = Vec::new();
        self.run_with(src, &mut |_, v| out.push(v.clone()))?;
        Ok(out)
    }

    /// See `check::Checker::symbols`.
    pub fn symbols(&self, ns: Option<&str>) -> Result<Vec<check::Symbol>, String> {
        self.interp.ck.symbols(ns)
    }

    /// Names `:symbols` can list: `prelude` first, then declared namespaces.
    pub fn namespaces(&self) -> Vec<String> {
        let mut names = vec![repl::PRELUDE_NAME.to_string()];
        names.extend(self.interp.ck.namespace_names());
        names
    }

    pub fn render(&self, v: &Value, json: bool) -> String {
        self.interp.render(v, json)
    }
}

fn runtime(span: Span, msg: String) -> Diag {
    Diag::new(span, format!("runtime error: {msg}"))
}
