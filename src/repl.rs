use std::io::{self, BufRead, IsTerminal};

use crate::Session;
use crate::complete::{Completions, ReplHelper};
use crate::check::{Member, Symbol, SymbolKind};
use crate::eval::json_str;
use crate::lexer::{Tok, lex};

/// True when `src` ends inside an open bracket or string, so the REPL should
/// keep reading lines before running it.
fn is_incomplete(src: &str) -> bool {
    match lex(src) {
        Ok(toks) => {
            let mut depth = 0i32;
            for t in &toks {
                match t.tok {
                    Tok::LParen | Tok::LBrace | Tok::LBracket => depth += 1,
                    Tok::RParen | Tok::RBrace | Tok::RBracket => depth -= 1,
                    _ => {}
                }
            }
            // A comment-only entry waits for the declaration it documents.
            depth > 0
                || (toks.iter().all(|t| matches!(t.tok, Tok::Eof))
                    && src.lines().any(|l| l.trim_start().starts_with("///")))
        }
        Err(d) => d.msg.starts_with("unterminated"),
    }
}

/// Where the interactive REPL keeps its history: `$RESH_HISTORY`, else `~/.resh_history`.
fn history_path() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("RESH_HISTORY") {
        return Some(p.into());
    }
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".resh_history"))
}

/// Runs the init file (see `library::init_path`), if there is one, so its
/// `IMPORT`s and declarations are in place before the first prompt. Its results
/// are not printed; an error is reported and the REPL starts anyway.
fn load_init(session: &mut Session) {
    let Some(path) = crate::library::init_path() else {
        return;
    };
    let Ok(src) = std::fs::read_to_string(&path) else {
        return;
    };
    if let Err(d) = session.run(&src) {
        eprintln!(
            "error in {} at {}:{}: {}",
            path.display(),
            d.span.line,
            d.span.col,
            d.msg
        );
    }
}

/// Feeds one input line to the session. `Ok(true)` means the line completed an
/// entry that was run (so it belongs in the history), `Ok(false)` that more
/// lines are needed; `Err(())` means the REPL should exit.
fn feed(session: &mut Session, buf: &mut String, line: &str, json: bool) -> Result<bool, ()> {
    if buf.is_empty() && matches!(line.trim(), ":quit" | ":q" | ":exit") {
        return Err(());
    }
    if buf.is_empty() {
        let t = line.trim();
        let arg = if t == ":symbols" {
            Some("")
        } else {
            t.strip_prefix(":symbols ")
                .filter(|_| t.starts_with(":symbols"))
        };
        if let Some(arg) = arg {
            let arg = arg.trim();
            if arg.is_empty() {
                println!("{}", render_namespaces(&session.namespaces(), json));
            } else {
                let ns = if arg == PRELUDE_NAME { None } else { Some(arg) };
                match session.symbols(ns) {
                    Ok(syms) => println!("{}", render_symbols(ns, &syms, json)),
                    Err(msg) => eprintln!("error: {msg}"),
                }
            }
            return Ok(true);
        }
    }
    buf.push_str(line);
    buf.push('\n');
    if is_incomplete(buf) {
        return Ok(false);
    }
    // The REPL supplies the final `;`, so a one-line entry can omit it.
    let src = std::mem::take(buf) + ";";
    if !src.trim().is_empty() {
        let result = session.run_with(&src, &mut |interp, v| {
            println!("{}", interp.render(v, json))
        });
        if let Err(d) = result {
            eprintln!("error at {}:{}: {}", d.span.line, d.span.col, d.msg);
        }
    }
    Ok(true)
}

/// Runs a read-eval-print loop. Declarations and `LET` bindings persist across
/// entries; `:symbols [namespace]` lists declarations; `:quit` or end of input
/// exits. On a terminal the line editor gives readline keybindings and a
/// persistent history; piped input is read plainly.
pub fn run(json: bool) {
    let mut session = Session::new();
    load_init(&mut session);
    let mut buf = String::new();
    if !io::stdin().is_terminal() {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if feed(&mut session, &mut buf, &line, json).is_err() {
                return;
            }
        }
        return;
    }
    println!("resh repl (:symbols [namespace] to list declarations, :quit or Ctrl-D to exit)");
    let mut editor = match rustyline::Editor::<ReplHelper, rustyline::history::FileHistory>::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("cannot start the line editor: {e}");
            return;
        }
    };
    editor.set_helper(Some(ReplHelper {
        completions: Completions::of(&session),
    }));
    let history = history_path();
    if let Some(h) = &history {
        let _ = editor.load_history(h);
    }
    // A multi-line entry is stored as one history item, so Up recalls it whole.
    let mut entry = String::new();
    loop {
        let prompt = if buf.is_empty() { "resh> " } else { "... " };
        match editor.readline(prompt) {
            Ok(line) => {
                if !entry.is_empty() {
                    entry.push('\n');
                }
                entry.push_str(&line);
                let fed = feed(&mut session, &mut buf, &line, json);
                if let Some(h) = editor.helper_mut() {
                    h.completions = Completions::of(&session);
                }
                match fed {
                    Err(()) => break,
                    Ok(false) => {}
                    Ok(true) => {
                        if !entry.trim().is_empty() {
                            let _ = editor.add_history_entry(entry.as_str());
                        }
                        entry.clear();
                    }
                }
            }
            // Ctrl-C abandons the entry being typed.
            Err(rustyline::error::ReadlineError::Interrupted) => {
                buf.clear();
                entry.clear();
            }
            Err(_) => break,
        }
    }
    if let Some(h) = &history {
        let _ = editor.save_history(h);
    }
}

/// Inserts each slot's `DEFAULT` into the matching top-level `( ... )` slot of
/// a record field signature: `port (Int)` + `[Some("8080")]` gives
/// `port (Int DEFAULT 8080)`.
fn with_defaults(member: &Member) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    let mut slot = 0usize;
    for c in member.signature.chars() {
        match c {
            '(' => {
                depth += 1;
                out.push(c);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(Some(d)) = member.default.get(slot) {
                        out.push_str(" DEFAULT ");
                        out.push_str(d);
                    }
                    slot += 1;
                }
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

fn kind_name(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Record => "record",
        SymbolKind::Enum => "enum",
        SymbolKind::Function => "function",
    }
}

/// What `:symbols` calls the namespace-less scope: the prelude and built-ins.
pub const PRELUDE_NAME: &str = "prelude";

/// Renders the namespace list that bare `:symbols` prints.
pub fn render_namespaces(names: &[String], json: bool) -> String {
    if json {
        let items: Vec<String> = names.iter().map(|n| json_str(n)).collect();
        format!("{{\"namespaces\":[{}]}}", items.join(","))
    } else {
        let mut out = vec!["namespaces".to_string()];
        out.extend(names.iter().map(|n| format!("  {n}")));
        out.join("\n")
    }
}

/// Renders the result of `:symbols <namespace>` as aligned text or one JSON object.
pub fn render_symbols(ns: Option<&str>, syms: &[Symbol], json: bool) -> String {
    if json {
        render_symbols_json(ns, syms)
    } else {
        render_symbols_text(ns, syms)
    }
}

fn render_symbols_text(ns: Option<&str>, syms: &[Symbol]) -> String {
    let mut out = vec![ns.unwrap_or(PRELUDE_NAME).to_string()];
    for s in syms {
        if let Some(doc) = &s.doc {
            for l in doc.lines() {
                out.push(if l.is_empty() {
                    "  ///".into()
                } else {
                    format!("  /// {l}")
                });
            }
        }
        let (kind, name) = match s.kind {
            SymbolKind::Record => ("record", s.name.clone()),
            SymbolKind::Enum => ("enum", s.name.clone()),
            SymbolKind::Function => (
                "fn",
                match &s.ret {
                    Some(r) => format!("{} -> {}", s.name, r),
                    None => s.name.clone(),
                },
            ),
        };
        out.push(format!("  {kind:<6}  {name}"));
        for m in &s.members {
            out.push(format!("{}{}", " ".repeat(12), with_defaults(m)));
        }
    }
    out.join("\n")
}

fn render_symbols_json(ns: Option<&str>, syms: &[Symbol]) -> String {
    let opt = |o: &Option<String>| o.as_deref().map_or("null".to_string(), json_str);
    let items: Vec<String> = syms
        .iter()
        .map(|s| {
            let members: Vec<String> = s
                .members
                .iter()
                .map(|m| {
                    let defs: Vec<String> = m.default.iter().map(opt).collect();
                    format!(
                        "{{\"signature\":{},\"defaults\":[{}]}}",
                        json_str(&m.signature),
                        defs.join(",")
                    )
                })
                .collect();
            format!(
                "{{\"kind\":\"{}\",\"name\":{},\"doc\":{},\"members\":[{}],\"returns\":{}}}",
                kind_name(s.kind),
                json_str(&s.name),
                opt(&s.doc),
                members.join(","),
                opt(&s.ret)
            )
        })
        .collect();
    format!(
        "{{\"namespace\":{},\"symbols\":[{}]}}",
        json_str(ns.unwrap_or(PRELUDE_NAME)),
        items.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(sig: &str, def: &[Option<&str>]) -> Member {
        Member {
            signature: sig.into(),
            default: def.iter().map(|d| d.map(String::from)).collect(),
        }
    }

    fn sample() -> Vec<Symbol> {
        vec![
            Symbol {
                kind: SymbolKind::Record,
                name: "Options".into(),
                doc: Some("Search \"options\".\nSecond line".into()),
                members: vec![member("hidden (Boolean)", &[Some("false")])],
                ret: None,
            },
            Symbol {
                kind: SymbolKind::Enum,
                name: "Match Kind".into(),
                doc: None,
                members: vec![member("exact", &[]), member("glob (pattern: String)", &[])],
                ret: None,
            },
            Symbol {
                kind: SymbolKind::Function,
                name: "find (root: String)".into(),
                doc: None,
                members: vec![],
                ret: Some("List (Entry)".into()),
            },
        ]
    }

    #[test]
    fn defaults_are_injected_into_slots() {
        assert_eq!(
            with_defaults(&member("port (Int)", &[Some("8080")])),
            "port (Int DEFAULT 8080)"
        );
        assert_eq!(with_defaults(&member("port (Int)", &[None])), "port (Int)");
        assert_eq!(
            with_defaults(&member("a (List (Int)) b (Int)", &[None, Some("1")])),
            "a (List (Int)) b (Int DEFAULT 1)"
        );
        assert_eq!(with_defaults(&member("exact", &[])), "exact");
    }

    #[test]
    fn text_output() {
        let want = "fd\n  /// Search \"options\".\n  /// Second line\n  record  Options\n            hidden (Boolean DEFAULT false)\n  enum    Match Kind\n            exact\n            glob (pattern: String)\n  fn      find (root: String) -> List (Entry)";
        assert_eq!(render_symbols(Some("fd"), &sample(), false), want);
        assert_eq!(render_symbols(None, &[], false), "prelude");
    }

    #[test]
    fn namespace_list() {
        let names = vec!["prelude".to_string(), "fd".to_string()];
        assert_eq!(render_namespaces(&names, false), "namespaces\n  prelude\n  fd");
        assert_eq!(
            render_namespaces(&names, true),
            "{\"namespaces\":[\"prelude\",\"fd\"]}"
        );
    }

    #[test]
    fn json_output() {
        let out = render_symbols(Some("fd"), &sample(), true);
        assert!(out.starts_with("{\"namespace\":\"fd\",\"symbols\":[{\"kind\":\"record\",\"name\":\"Options\",\"doc\":\"Search \\\"options\\\".\\nSecond line\","));
        assert!(out.contains("\"members\":[{\"signature\":\"hidden (Boolean)\",\"defaults\":[\"false\"]}],\"returns\":null"));
        assert!(out.contains("\"kind\":\"function\",\"name\":\"find (root: String)\",\"doc\":null,\"members\":[],\"returns\":\"List (Entry)\""));
        assert_eq!(
            render_symbols(None, &[], true),
            "{\"namespace\":\"prelude\",\"symbols\":[]}"
        );
    }
}
