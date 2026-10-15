use resh::Session;
use resh::eval::json_str;

fn usage_text() -> &'static str {
    "usage: resh [repl] [--json]
       resh run <file.resh> [--json | --raw]
       resh check <file.resh> [--json]
       resh -c \"<source>\" [--json | --raw]
       resh libs [show <name>]
       resh syntax
       resh skill install
       resh --help

--raw prints results as plain text: a Result is unwrapped (an error goes to
stderr and makes the exit code 1), strings are unescaped, string lists are
printed one per line."
}

fn usage() -> ! {
    eprintln!("{}", usage_text());
    std::process::exit(2);
}

fn read(file: &str) -> String {
    std::fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("cannot read {file}: {e}");
        std::process::exit(2);
    })
}

const SYNTAX: &str = include_str!("../SYNTAX.md");
const SKILL: &str = include_str!("../.claude/skills/new-wrapper/SKILL.md");

/// `resh libs`: where user libraries go, and every library `IMPORT` can load.
fn libs() {
    let dir = resh::library::user_dir();
    match &dir {
        Some(d) => println!("user library directory: {}", d.display()),
        None => println!("user library directory: (none: set HOME or RESH_LIB_DIR)"),
    }
    for (name, origin) in resh::library::available(dir.as_deref()) {
        let origin = match origin {
            resh::library::Origin::User => "user",
            resh::library::Origin::Bundled => "bundled",
        };
        println!("  {name:<12} {origin}");
    }
}

/// `resh libs show <name>`: prints a library's source.
fn show_lib(name: &str) {
    let dir = resh::library::user_dir();
    match resh::library::find(name, dir.as_deref()) {
        Some((_, src)) => print!("{src}"),
        None => {
            eprintln!("no library `{name}`; run `resh libs` to list them");
            std::process::exit(1);
        }
    }
}

/// `resh skill install`: writes the `new-wrapper` skill for Claude Code.
fn install_skill() {
    let Some(home) = std::env::var_os("HOME") else {
        eprintln!("HOME is not set");
        std::process::exit(1);
    };
    let dir = std::path::PathBuf::from(home).join(".claude/skills/new-wrapper");
    let file = dir.join("SKILL.md");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, SKILL)) {
        eprintln!("cannot write {}: {e}", file.display());
        std::process::exit(1);
    }
    println!("installed {}", file.display());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let raw = args.iter().any(|a| a == "--raw");
    let rest: Vec<&str> = args
        .iter()
        .map(|s| s.as_str())
        .filter(|a| *a != "--json" && *a != "--raw")
        .collect();
    let mut check_only = false;
    let src = match rest.as_slice() {
        ["check", file] => {
            check_only = true;
            read(file)
        }
        ["--help"] | ["-h"] | ["help"] => {
            println!("{}", usage_text());
            return;
        }
        ["libs"] => {
            libs();
            return;
        }
        ["libs", "show", name] => {
            show_lib(name);
            return;
        }
        ["syntax"] => {
            print!("{SYNTAX}");
            return;
        }
        ["skill", "install"] => {
            install_skill();
            return;
        }
        ["-c", code] => code.to_string(),
        ["run", file] => read(file),
        [] | ["repl"] => {
            resh::repl::run(json);
            return;
        }
        _ => usage(),
    };
    let mut session = Session::new();
    let mut failed = false;
    let result = if check_only {
        session.check_only(&src)
    } else {
        session.run_with(&src, &mut |interp, v| {
            if raw {
                let (ok, text) = interp.raw(v);
                if ok {
                    println!("{text}");
                } else {
                    eprintln!("{text}");
                    failed = true;
                }
            } else {
                println!("{}", interp.render(v, json))
            }
        })
    };
    if let Err(d) = result {
        if json {
            println!(
                "{{\"error\":{{\"line\":{},\"column\":{},\"message\":{}}}}}",
                d.span.line,
                d.span.col,
                json_str(&d.msg)
            );
        } else {
            eprintln!("error at {}:{}: {}", d.span.line, d.span.col, d.msg);
        }
        std::process::exit(1);
    }
    if check_only && !json {
        println!("ok");
    }
    if failed {
        std::process::exit(1);
    }
}
