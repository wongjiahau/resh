# resh dogfooding gaps

Things that forced a fallback to bash (or other tools) while using resh as the
shell for Claude Code sessions. Append as new gaps are found; strike through
or remove once fixed.

## Missing library coverage

- `git` wrapper is read-only: no `add`, `commit`, `push`, `checkout`.
- No `cargo` wrapper (`build`, `test`, `install`).
- No `brew` wrapper.
- No way found to inspect `PATH` / environment variables or `which <tool>`.
- No file read/write in the prelude (not verified; may exist under another name).

## Language / CLI friction

- `:symbols` is REPL-only; `resh -c ':symbols'` is a parse error, so names
  cannot be discovered non-interactively (workaround: read `lib/*.resh` or
  `resh libs show <name>`).
- `--raw` prints every field of every record (e.g. `list file in`), with no
  concise way to ask for just one field from the command line.
- Calling a namespaced function without `rg.` / `IMPORT` qualification errors
  (diagnostic is good; noted for discoverability only).
