---
name: new-wrapper
description: Write a typed resh wrapper (a `.resh` library) for an existing CLI tool. Use whenever the user asks to wrap, bind or create a wrapper for a command-line tool (e.g. "wrap docker", "make a resh wrapper for jq").
---

A wrapper is a `.resh` library with one `NAMESPACE`, typed options as `ENUM`s,
and functions that build an argv and call the `run` primitive. Users load it
with `IMPORT <name>;`. This skill makes new ones in the same shape as the
bundled ones. It needs the `resh` binary on `PATH`.

## Steps

1. **Learn the language.** Run `resh syntax` (read "Running external commands"
   closely), then `resh libs` and `resh libs show git`, `resh libs show fd` and
   `resh libs show herdr`. Copy their structure, comment style and naming.
   Conventions:
   - No "of" in names: `(self) file name`, not `file name of (self)`.
   - Every word of a type name is capitalised: `Sort Key`, `Entry Kind`.
   - Type names are anyfix like functions: `List (Entry)`, `Result (T)`.
2. **Learn the tool.** Run `<tool> --help`, `<tool> <subcommand> --help` and
   `man <tool>` (all read-only). Pick the commands and flags people actually use;
   don't mirror every flag. Prefer machine-readable output flags (`--porcelain`,
   `--json`, `-z`, …), always pass them, and document the output shape in the
   header comment.
3. **Design the surface.**
   - One `ENUM <Command> Option` per command, one member per flag, named as a
     readable phrase (`include hidden file`, not `H`). Alternatives that exclude
     each other are one variant carrying an enum.
   - A `FUNCTION (option: <Command> Option) argument: List (String)` that maps each
     member to argv with `MATCH`.
   - One function per command that takes `Set (<Command> Option)` and returns
     `Result (T)`; parse stdout into typed values (`lines`, records) where the
     format is stable, otherwise return the raw `String`.
   - `///` doc comments directly above every public `FUNCTION`, `RECORD`, `ENUM`.
   - A trailing `// --flag` comment on each option member.
4. **Be conservative with side effects.** Wrap read-only commands first. If you
   wrap a mutating command, say so in the header comment and in the doc comment,
   and never invoke it while testing.
5. **Write it** to `<user library directory>/<tool>.resh`, where the directory is
   the one `resh libs` prints (`~/.config/resh/lib` by default), unless the user
   names another path. The file must start with `NAMESPACE <tool>`; the file name
   and namespace must match, or `IMPORT` rejects it.
6. **Check it.** Run `resh check <file>`. It type-checks without running anything.
   Diagnostics list the call site, the inferred types and every candidate
   signature; fix what they say and re-run until it exits 0. Then confirm it loads:
   `resh -c 'IMPORT <tool>;'`. If a needed language feature is missing, say so
   rather than working around it with something unidiomatic.
7. **Smoke-test read-only commands** with `resh -c 'IMPORT <tool>; <tool>::...;'`
   only when running them is harmless.
8. **Report** the namespace, the commands covered, what was left out on purpose,
   how to use it (`IMPORT <tool>;`) and any language gaps hit.

## Working inside the resh repository

Put the wrapper in `lib/<tool>.resh` instead and add it to `BUNDLED` in
`src/lib.rs` (`tests/bundled.rs` then checks it automatically). Add a test in the
style of `tests/git_example.rs`: replace `run` with `Session::set_runner`, record
argv, and assert it for each option, plus parsing tests with canned stdout.

## Rules

- Never run the wrapped tool with arguments that modify anything, and never
  guess a flag: confirm it in `--help` output first.
- No shell strings: every argv entry is its own list element.
- Don't copy legacy "of" names from the prelude.
