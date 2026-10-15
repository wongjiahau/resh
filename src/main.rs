use resh::Session;
use resh::eval::json_str;

fn usage() -> ! {
    eprintln!(
        "usage: resh [repl] [--json]\n       resh run <file.resh> [--json]\n       resh check <file.resh> [--json]\n       resh -c \"<source>\" [--json]\n       resh libs [show <name>]\n       resh syntax\n       resh skill install"
    );
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
    let rest: Vec<&str> = args
        .iter()
        .map(|s| s.as_str())
        .filter(|a| *a != "--json")
        .collect();
    let mut check_only = false;
    let src = match rest.as_slice() {
        ["check", file] => {
            check_only = true;
            read(file)
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
    let result = if check_only {
        session.check_only(&src)
    } else {
        session.run_with(&src, &mut |interp, v| {
            println!("{}", interp.render(v, json))
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
}
