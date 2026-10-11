use resh::Session;
use resh::eval::json_str;

fn usage() -> ! {
    eprintln!(
        "usage: resh [repl] [--json]\n       resh run <file.resh> [--json]\n       resh -c \"<source>\" [--json]"
    );
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let rest: Vec<&str> = args
        .iter()
        .map(|s| s.as_str())
        .filter(|a| *a != "--json")
        .collect();
    let src = match rest.as_slice() {
        ["-c", code] => code.to_string(),
        ["run", file] => std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("cannot read {file}: {e}");
            std::process::exit(2);
        }),
        [] | ["repl"] => {
            resh::repl::run(json);
            return;
        }
        _ => usage(),
    };
    let mut session = Session::new();
    let result = session.run_with(&src, &mut |interp, v| {
        println!("{}", interp.render(v, json))
    });
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
