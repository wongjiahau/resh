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

- Every call-like construct (signatures, types, patterns, call expressions,
  declaration names, brace keys) is parsed by the single meta-syntax layer in
  `src/parser.rs`: `parse_anyfix` / `anyfix_item`, a run of `Anyfix::Word`,
  `Anyfix::Slot` (`( … )`) and `Anyfix::Atom` (one token).
- A new call-like construct must go through it, supplying only a slot parser and
  an atom recogniser. Don't hand-roll another word/`(`…`)` loop.
- Matching a call's `Seq` against declared signatures stays in the checker
  (`src/check.rs`), since only it knows the signatures.
