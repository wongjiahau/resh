# resh TODO

Goal: make `examples/fd.resh` lex, parse, type-check and run, following
`SYNTAX.md`. Work one task at a time, top to bottom (later tasks depend on
earlier ones). A task is done only when its acceptance criteria hold and
`cargo test` passes.

Ground rules (see CLAUDE.md and README.md):

- All call-like constructs go through `parse_anyfix` / `anyfix_item`
  in `src/parser.rs`. No hand-rolled word/`(`…`)` loops.
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

- [ ] **8. Prelude functions.**
  `flat map (items) with (f)` (lists and sets), `(n) to string`,
  `map (result) with (f)`, `join (a) with (b)`.
  *Done when:* each has a test, including the `Result` and `Set` cases.

- [ ] **9. `Size` and `Duration` units.**
  Types plus suffix functions `kb`, `mb`, `gb`, `seconds`, `minutes`, `hours`,
  `days`, `ago`, and the accessors `(size) bytes`, `(duration) seconds`.
  Units are ordinary suffix functions, no special syntax.
  *Done when:* `fd::find "" in "." { size at least (1 mb) }` type-checks and
  `{ size at least (5 days) }` is a type error.

- [ ] **10. `run` primitive.**
  `run ((command: String, args: List (String))): Result (String) Or (String)`.
  Stdout is `Ok`; stderr plus exit code is `Err`. Never invoke a shell; pass
  argv directly.
  *Done when:* a test runs a real harmless command and one failing command.

- [ ] **11. `examples/fd.resh` end to end.**
  Type-check the file, and test `fd::find` with `run` mocked, asserting the
  argv produced for the four usage lines at the bottom of the file.
  *Done when:* those tests pass and the "does not parse yet" note in the file
  header is removed.
