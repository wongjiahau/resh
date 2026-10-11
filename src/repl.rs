use std::io::{self, BufRead, IsTerminal, Write};

use crate::Session;
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
            depth > 0
        }
        Err(d) => d.msg.starts_with("unterminated"),
    }
}

/// Runs an interactive read-eval-print loop on stdin/stdout. Declarations and
/// `LET` bindings persist across entries; `:quit` or end of input exits.
pub fn run(json: bool) {
    let stdin = io::stdin();
    let interactive = stdin.is_terminal();
    let mut session = Session::new();
    let mut buf = String::new();
    if interactive {
        println!("resh repl (:quit or Ctrl-D to exit)");
    }
    let prompt = |continuation: bool| {
        if interactive {
            print!("{}", if continuation { "... " } else { "resh> " });
            let _ = io::stdout().flush();
        }
    };
    prompt(false);
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if buf.is_empty() && matches!(line.trim(), ":quit" | ":q" | ":exit") {
            return;
        }
        buf.push_str(&line);
        buf.push('\n');
        if is_incomplete(&buf) {
            prompt(true);
            continue;
        }
        let src = std::mem::take(&mut buf);
        if !src.trim().is_empty() {
            let result = session.run_with(&src, &mut |interp, v| {
                println!("{}", interp.render(v, json))
            });
            if let Err(d) = result {
                eprintln!("error at {}:{}: {}", d.span.line, d.span.col, d.msg);
            }
        }
        prompt(false);
    }
    if interactive {
        println!();
    }
}
