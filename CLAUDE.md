# resh conventions

## Naming

- Avoid "of" in names, including function names, record fields, and type names.
  Prefer a name that reads naturally without it.
  - Bad: `file name of (self)`, `modified at of (self)`, `lines of (text)`
  - Better: `(self) file name`, `(self) modified at`, `(text) lines`
- Existing `prelude.resh` names that use "of" are legacy; don't copy the pattern,
  and rename them when touching them.

## Type syntax

- Type names are anyfix, like functions. Bare words are part of the identifier;
  parentheses mark a parameter slot: `List (Entry)`,
  `Result (T) Or (E)`. The `<A, B>` syntax is gone.
- Every word in a type name is capitalized (title case) so syntax highlighting
  can recognise types lexically: `Sort Key`, `Entry Kind`.

## Parser

- resh source is anyfix expressions (`crates/anyfix-expr`). That crate does all
  the grouping; `src/parser.rs` only reads the tree it returns. It flattens each
  call into its name words and arguments (`flatten`, `Flat::Word` /
  `Flat::Arg`) and recognises resh's own forms by their uppercase keyword
  (`FUNCTION`, `MATCH`, `IF`, ...) or symbol word (`=>`, `->`, `=`).
- A new construct is a new keyword or shape read from that tree. Don't add
  tokens, operators or a recursive-descent parser of resh's own: chaining goes
  through `.` (left) and `:` (right), as `crates/anyfix-expr/README.md` says.
- Matching a call's `Seq` against declared signatures stays in the checker
  (`src/check.rs`), since only it knows the signatures.
