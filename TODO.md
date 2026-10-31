# resh TODO

Goal: make `lib/fd.resh` lex, parse, type-check and run, following
`SYNTAX.md`. Work one task at a time, top to bottom (later tasks depend on
earlier ones). A task is done only when its acceptance criteria hold and
`cargo test` passes.

Ground rules (see CLAUDE.md and README.md):

- resh source is anyfix expressions: `crates/anyfix-expr` groups it and
  `src/parser.rs` reads the tree. No tokens or operators of resh's own.
- Type checking is bottom-up only. Overloads are resolved from inferred
  argument types; an unresolvable or ambiguous call is a type error whose
  message lists the call site, inferred argument types, every candidate, and
  what to change.
- Names avoid "of"; type names are Title Case.
- If `SYNTAX.md` is underspecified or wrong, stop and ask. When the
  implementation and the spec disagree, update the spec and say so.

## Tasks

- [x] **1. `::` token and qualified names.**
  Lexer: `::` is distinct from `:`. Parser: right associative, the qualifier
  applies to the whole anyfix call to its right (`a::b c` is `a::(b c)`).
  Works for functions and types.
  *Done when:* tests parse `git::commit "x"` and `LET c: git::Commit = ...`,
  and `a::b::c d` parses as `a::(b::(c d))`.

- [x] **2. `NAMESPACE name` (file-wide).**
  Must be the first declaration in a file; one per file. Declarations inside
  are referenced unqualified within the file, and as `name::...` from outside.
  No `USE`.
  *Done when:* a two-file (or two-session) test calls `fd::find` from outside,
  and an unqualified call to it from outside is a clear error.

- [x] **3. New record form.**
  Fields are anyfix phrases with type-only slots, each slot optionally
  `DEFAULT value`. Remove the `key: value` form and the auto-generated
  accessor functions; destructure with a single-case `MATCH`. Literals are
  unnamed `{ entry, entry }`: atoms bare, longer values parenthesized, a bare
  Boolean field name means `true`. A field may be omitted only if every slot
  has a `DEFAULT`.
  Migrate `src/prelude.resh` (`Entry`, `List Options`, ...) and the existing
  tests to the new form.
  *Done when:* `RECORD Server Config { host (String DEFAULT "localhost"), port (Int DEFAULT 8080) }`
  works with `{ port 443 }`, `{}`, and a destructuring `MATCH`.

- [x] **4. Structural literal types and overload matching.**
  Infer a record literal's type structurally (field signatures with slot types)
  and match it against candidate overloads. No candidate or several candidates
  is a type error listing each candidate's fields.
  *Done when:* an unknown field and an ambiguous `{}` both produce the
  diagnostics described above.

- [x] **5. `Set (E)`.**
  A set of variants of an enum `E`, with the `{ ... }` literal. Record vs set
  is decided by matching against candidates. Two entries of the same variant in
  one literal is a type error naming both positions. `{}` is the empty set.
  *Done when:* `{ max depth 1, max depth 2 }` fails with that error and
  `{ extension ["rs"], max depth 3 }` is accepted for `Set (Find Option)`.

- [x] **6. Lambdas.**
  `(x) => body`, parenthesized when used as a slot value.
  *Done when:* `flat map (xs) with ((x) => [x, x])` type-checks and evaluates.

- [x] **7. Tuples and list spread.**
  Tuple values and tuple parameters (`("fd", args)`); list literals with
  `...` spread.
  *Done when:* `[...a, "x", ...b]` and a function taking a tuple parameter
  both work.

- [x] **8. Prelude functions.**
  `flat map (items) with (f)` (lists and sets), `(n) to string`,
  `map (result) with (f)`, `join (a) with (b)`.
  *Done when:* each has a test, including the `Result` and `Set` cases.

- [x] **9. `Size` and `Duration` units.**
  Types plus suffix functions `kb`, `mb`, `gb`, `seconds`, `minutes`, `hours`,
  `days`, `ago`, and the accessors `(size) bytes`, `(duration) seconds`.
  Units are ordinary suffix functions, no special syntax.
  *Done when:* `fd::find "" in "." { size at least (1 mb) }` type-checks and
  `{ size at least (5 days) }` is a type error.

- [x] **10. `run` primitive.**
  `run ((command: String, args: List (String))): Result (String)`.
  Stdout is `Ok`; stderr plus exit code is `Err`. Never invoke a shell; pass
  argv directly.
  *Done when:* a test runs a real harmless command and one failing command.

- [x] **11. `lib/fd.resh` end to end.**
  Type-check the file, and test `fd::find` with `run` mocked, asserting the
  argv produced for the four usage lines at the bottom of the file.
  *Done when:* those tests pass and the "does not parse yet" note in the file
  header is removed.

## Main goal: the best shell for AI agents

resh should be the shell an AI agent reaches for instead of bash. Everything
else (lower-level tiers, bootstrapping) is secondary and waits on this.

Three questions decide it: does the agent end up running resh at all, can resh
do what the agent was about to do, and is it measurably better? Work top to
bottom; same ground rules as above.

- [ ] **A1. Rank what agents actually run.**
  Run `rtk discover` (or read Claude Code history) and list the most common
  commands, with counts. This is the wrapper priority list for A5.
  *Done when:* the ranked list is in `DOGFOOD.md`.

- [ ] **A2. Shell glue agents rely on.**
  Pipes between commands, `&&`-style sequencing that stops on failure,
  redirecting output to a file, reading stdin, environment variables and
  `which`/`PATH`, multi-line scripts, and file read/write in the prelude
  (verify what already exists first).
  *Done when:* each has a test, and a task from A1 that needed bash for one of
  these no longer does.

- [ ] **A3. Explicit escape hatch.**
  Running a command with no wrapper must not be a dead end. Add a clearly
  marked raw form (argv, or an explicit `sh -c`) whose result is flagged as
  unchecked, so the agent stays in resh instead of switching to bash.
  *Done when:* an unwrapped command runs, and its call site is visibly raw in
  the source and in `--json` output.

- [ ] **A4. Operational behaviour.**
  Fast startup, command timeouts, signal handling, closed stdin, no hangs on
  interactive prompts, exit codes that mean something, stdout/stderr streaming
  for long commands.
  *Done when:* each has a test, including a command that never exits.

- [ ] **A5. Wrapper coverage.**
  Wrappers for the top commands from A1: first the `DOGFOOD.md` gaps (`git`
  write operations, `cargo`, `brew`), then the rest. Use the `new-wrapper`
  skill.
  *Done when:* the top 90% of the A1 list is covered, each wrapper passing
  `resh check` and an argv test.

- [ ] **A6. Cheap output.**
  `--raw` prints every field of every record. Add a concise way to select
  fields (and cap list length), so results cost the model fewer tokens.
  *Done when:* a call can return just one field, from the command line and in
  a script.

- [ ] **A7. Get resh into the agent loop.**
  `resh agent install`: installs the skill and a hook (or a tool/MCP server) so
  the agent's commands go through resh by default, not by persuasion.
  *Done when:* a fresh Claude Code session runs a shell task through resh
  without being told to.

- [ ] **A8. Measure with fresh agents.**
  ~5 real tasks, each done by a fresh agent in bash and in resh (resh agent
  sees only `SYNTAX.md`, `README.md`, `lib/`). Record first-try success,
  attempts, and whether a bad call was rejected before it ran. Destructive
  tasks run in a throwaway directory.
  *Done when:* the table is in `DOGFOOD.md`, failures counted.

- [ ] **A9. Typed permissions (explore).**
  Agent harness allow-lists match command strings and are brittle (`git push`
  vs `git push --force`). Typed calls allow exact rules: allow `git.status`,
  ask for `git.push`, deny `git.push { force }`.
  *Done when:* a design note says what the harness needs, and a prototype rule
  file blocks one typed call and allows another.

## Design direction: going lower level (not scheduled)

Decided: memory is GC-first behind a *pure surface*, so a manual tier can be
added later without breaking programs.

- No references, no pointer identity, no mutation through aliases. Any future
  mutation is an explicit value such as `Cell (T)`.
- Prefer a non-moving tracing collector (or reference counting in the
  interpreter) so addresses stay stable for a later FFI.
- Reserve `Pointer (T)` and sized ints (`Int 32`, `Unsigned Int 8`) as
  unsafe-only names; don't implement them yet.
- Manual management arrives as additions: regions first (`with region { ... }`),
  ownership only if needed.

More decisions:

- Mutation exists, as explicit `Cell (T)`-style values. Functional style is
  encouraged; it is never the default.
- There is a compiled tier, targeting Cranelift.
- Collector: simplest first. Reference counting in the interpreter; a bump
  allocator that never frees in the compiled tier (compiler runs are short).
- `Int` only for now (no sized ints yet).
- Unsafe boundary: a file that uses `INTRINSIC` must declare itself `UNSAFE`
  (like `NAMESPACE`, first declaration); the checker enforces it. Callers of
  safe wrappers need nothing.
- Errors: `Result` all the way, including low-level operations that can fail.

Open: the exact `INTRINSIC` declaration form.

## Deferred goal: bootstrap the compiler

Not the main goal; on hold until the AI-agent shell work above is further
along.

Stage 0 is the Rust implementation (the seed, never deleted). Stage 1 is the
compiler written in resh, run by stage 0. Stage 2 is stage 1 compiling itself;
done when stage 2 reproduces its own output exactly.

Tension to resolve: Cranelift is a Rust crate, so a resh-written compiler
cannot link it. Likely split: the resh frontend (lexer, parser, checker,
lowering) emits a serialized IR; a small Rust backend loads it into Cranelift.

- [ ] **B1. Lexer in resh**, run by the interpreter. Records gaps in `DOGFOOD.md`.
- [ ] **B2. Parser in resh** (the single `parse_anyfix` layer).
- [ ] **B3. Checker in resh** (bottom-up inference, overload resolution).
- [ ] **B4. Lowering to the IR** and the Rust Cranelift backend.
- [ ] **B5. Runtime in resh over intrinsics** (allocator, strings, lists).
- [ ] **B6. Close the loop**: stage 1 builds stage 2, stage 2 builds itself.

Known blockers for B1 (see `DOGFOOD.md`): file read/write, string char
access and building, argv and exit codes, interpreter speed.
