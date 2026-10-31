# resh conventions

## No backward compatibility (until release)

- resh has no users yet. Change syntax, semantics and APIs freely: no
  deprecated aliases, no shims, no old form kept alongside the new one.
- Update every caller, library, test and doc in the same commit instead.
- Don't let compatibility rule out a design.

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
  `Flat::Arg`) and recognises resh's own forms by their exact call shape
  (`IF _ THEN _ ELSE _`, `MATCH _ _`, `_ AS _`, `FN _ _`,
  `FUNCTION _ RETURNS _ _`, ...), so a signature or name goes in parentheses.
  It never searches a call for keywords, takes a keyword off the front of a
  name, or splits one call into several expressions. The one exception is a
  MATCH arm `pattern : value`, whose last slot is the value.
- A bare word is always part of a call's name, never a value: the checker only
  lets arguments fill slots, so a variable is written `(n) to string`.
- A new construct is a new keyword or shape read from that tree. Don't add
  tokens, operators or a recursive-descent parser of resh's own: chaining goes
  through `.` (left) and `:` (right), as `crates/anyfix-expr/README.md` says.
- resh's own forms use word keywords only (`LET`, `IF`, `MATCH`, `AS`, ...),
  never symbol words. The one exception is `->`, namespace access
  (`git -> status`, the form `_ -> _`). Every other symbol word (`=`, `=>`,
  `*`, `++`, ...) is left to users, so they can declare it as an ordinary
  function such as `_ = _`. Don't add more exceptions.
- Matching a call's `Seq` against declared signatures stays in the checker
  (`src/check.rs`), since only it knows the signatures.
