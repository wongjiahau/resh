# resh syntax

The language reference. Everything here is implemented unless a section says
otherwise.

## Quick start

```
$ resh -c 'IMPORT git; git::status;'          # run one snippet
$ resh                                         # a REPL; :symbols lists what is available
$ resh run script.resh                         # run a file
$ resh run script.resh --raw                   # plain text instead of `ok ("...")`
```

```
IMPORT git;
LET recent = git::log { max count 3 };        // ok ([...]) or error ("...")
recent;
git:: { diff { staged } };                    // a scope block: names of `git` unqualified
```

Calls read like phrases (see "Anyfix"). Optional flags go in a `{ ... }` set.
Every call that can fail returns a `Result`: `ok (value)` or `error (message)`.
Prelude functions (the ones you can call without `IMPORT`) are listed by
`:symbols` in the REPL, and `:symbols git` lists a library's. Libraries are
described in "Importing libraries"; writing your own is in "Writing a wrapper".

Command line: `resh --help` shows every subcommand. `--json` prints results (and
errors) as JSON; `--raw` prints plain text, unwrapping a `Result` (an `error` goes
to stderr and the exit code is 1) and printing string lists one per line.
`resh check file.resh` type-checks without running anything and prints `ok`.

## Statements

The language is not whitespace-sensitive: newlines and indentation are plain
whitespace, and a call may span lines.

- Every statement ends with `;`: expression statements and `LET` bindings, at
  the top level and in function bodies. Declarations (`FUNCTION`, `RECORD`,
  `ENUM`, `NAMESPACE`) need no `;`, and stray `;` between statements are
  ignored. A missing `;` is an error (the REPL supplies the last one).
- In a block, the last expression may omit its `;`: it is then the block's
  value (the function's return value). With a `;` the value is discarded and
  the block is `Unit`.

```
LET files = list file in "src";
FUNCTION answer: Int { LET x = 41; x }
```

## Anyfix

Every call-like construct (function signatures, type names, patterns, call
expressions, record fields) is an *anyfix*: a run of bare words and parenthesized
slots. Words belong to the name; parentheses mark a parameter slot.

```
list file in (dir)
(a) equals (b)
write (text) to file (path)
Result (T) Or (E)
```

A slot takes either a single token (an atom) or a parenthesized expression.
The parser stays context-free: it never needs to know which words are
functions.

```
count (items)        // slot is a parenthesized expression
print "hello"        // slot is an atom, no parentheses needed
print (1 mb)         // two tokens, parentheses required
```

## Namespaces

```
NAMESPACE git

RECORD Commit { hash (String), message (String) }

FUNCTION commit (message: String): Result (Commit) { ... }
```

- `NAMESPACE name` applies file-wide: it must come first in the file, and every
  declaration in the file belongs to it. One namespace per file. Members do not
  repeat the prefix.
- Namespaces are written with `::`. Single `:` stays the type annotation.
- Everything can be namespaced: functions, types, enums, records.
- Outside the namespace, names are qualified. Inside the file, they resolve
  unqualified.
- There is no file-level `USE`; callers qualify, or open a scope block (below).
- `::` is right associative: `a::b::c d` is `a::(b::(c d))`. The qualifier
  applies to the whole anyfix call to its right.
- A qualifier is only valid at the head of a call (or type). Mid-run
  (`a b::c d`) is a parse error. A function that starts with a slot is qualified
  before the slot: `fd::(option) argument`, not `(option) fd::argument`.
- Namespaces do not nest: a file has one `NAMESPACE name` (a single word), so a
  chain such as `a::b::c d` parses (see above) but is a type error. Use flat
  anyfix names (`git::remote add (name) (url)`). See the README objective
  "Readable shell commands".
- Unqualified calls and types in a file see the prelude and that file's own
  namespace. A call to a namespaced function without its qualifier is an error
  that names the namespace.

```
git::commit "fix bug";
LET c: git::Commit = ...;
```

### Scope blocks

`ns:: { statements }` opens `ns` for the statements inside: its functions,
variants and types are written unqualified.

```
rg:: {
    search "error" in "src" { match (whole word), case (insensitive) };
    search "TODO";
}
```

- The block ends at its `}`; a `;` after it is optional. Outside it, names are
  qualified again. It works at the top level and in function bodies; as the
  last statement of a body, its last expression is the body's value.
- Inside, `ns` wins over names from outside: if a call fits a function of `ns`,
  an outer function with the same shape is not considered. Outer names
  (including the prelude) are still reachable when `ns` has no such shape, or
  with an explicit qualifier (`other::name`).
- Variant values need no special rule: in `{ match (whole word) }` the variant
  resolves because the block opened `rg`.
- Blocks nest; the innermost opened namespace wins over the outer ones.
- The namespace must be declared, else `unknown namespace`.

## Importing libraries

`IMPORT name;` loads the wrapper library `name`, so its namespace can be used:

```
IMPORT git;
git::status;
```

- A library is a `.resh` file that starts with `NAMESPACE name`. `resh` looks
  for `name.resh` in the user library directory first (`$RESH_LIB_DIR`, else
  `$XDG_CONFIG_HOME/resh/lib`, else `~/.config/resh/lib`), then among the
  libraries bundled in the binary (`fd`, `git`, `herdr`, `rg`, `sed`). A user
  file with the same name replaces the bundled one.
- Each library is loaded once per session; importing it again does nothing.
- In a file, `IMPORT` comes after `NAMESPACE` (if any) and before the
  declarations. Imports are loaded before the file's own declarations.
- The library's top-level statements, if it has any, are not run.
- `resh libs` lists the libraries and the user directory; `resh libs show name`
  prints a library's source; `resh check file.resh` type-checks a library without
  running anything.

## Let bindings

`LET name = value` binds a single-word name. A type may be stated with
`LET name: Type = value`, which means `LET name = (value : Type)`.

## Type aliases

`TYPE Result (T) = Result (T) Or (String)` gives a shorter name to a type. The
left side is an anyfix pattern whose slots are single capital letters; the right
side is any type, which may use those letters. `Result (_)` and `Result (_) Or (_)`
are different identifiers, so both exist: the prelude declares the first as the
common case where the error is a message. An alias is interchangeable with what
it stands for, and a type that prints as `Result (T) Or (String)` is shown as
`Result (T)`.

## Records

A record is a set of anyfix fields. There is no `key: value` form.

```
RECORD Server Config {
    host (String DEFAULT "localhost"),
    port (Int DEFAULT 8080),
    timeout (Duration DEFAULT 30 seconds)
}
```

- Slots hold types only, never names: `between (Int) and (Int)`.
- A field with no slots is a plain marker word. For an on/off option, use a
  `Boolean` slot. In a literal, a bare `Boolean` field name is sugar for
  `true`: `{ verbose }` means `{ verbose true }`. (Optional flags belong in a
  `Set (E)`; see below.)
- A slot may carry `DEFAULT value`. Each slot of a multi-slot field has its own
  default.
- A field may be omitted in a literal only if every slot has a `DEFAULT`.
  There are no optional fields, so destructuring always sees every field.

### Literals

Record literals are unnamed (and so are set literals). Entries are anyfix phrases separated by commas.

```
{ host "example.com", port 443 }
{ timeout (2 minutes) }
{}
```

- Slot values follow the anyfix rule: atoms are bare, anything longer needs
  parentheses.
- Bare words in an entry are always part of the field's name, because the
  parser is context-free and cannot tell a name word from a value. The only
  exceptions are the Boolean literals `true` and `false`. A slot holding an
  enum variant, a variable or a zero-parameter call is therefore parenthesized:
  `{ sort by (size) }`, not `{ sort by size }`.
- The literal's type is inferred structurally (the set of field signatures with
  their slot types) and matched against the candidate overloads. See
  *Overload resolution*.

### Destructuring

There are no accessor functions. The only way to take values out of a record is
a `MATCH` with a record pattern:

```
MATCH config {
    { host (h), port (p), timeout (t) } => ...
}
```

- The pattern has one phrase per field, with a `(name)` binder (or a nested
  pattern) in each slot.
  Every field must be listed, in any order; `(_)` ignores a slot.
- A record is a type with one constructor, so a record pattern takes part in the
  same exhaustiveness check as enum patterns. A slot may hold a nested enum
  pattern (`{ color (red), size (n) }`), and a record pattern may itself sit in
  an enum variant's slot. A record `MATCH` is exhaustive with one arm unless a
  slot is refutable; further arms are checked like any other (an arm after an
  irrefutable one is unreachable). `_` matches a whole record.

## Enum variants and patterns

- A variant slot holds a type alone or a named parameter: `max depth (Int)` or
  `max depth (n: Int)`. The type-only form is the norm.
- A `MATCH` slot is a binder `(name)`, `(_)`, or a nested pattern. A nested
  pattern is a variant of the slot's enum type: `match (glob) => ...`,
  `match (fixed string) => ...`. A lone `(word)` is a nested pattern when
  `word` names a nullary variant of the slot's enum, otherwise a binder (so a
  binder cannot reuse such a name). Nested patterns may be nested further, to
  any depth, and an arm may nest in several slots.
- Exhaustiveness uses the pattern-matrix algorithm: a missing case is reported
  as a pattern (`missing: deep (w (_))`), and an arm that earlier arms already
  cover (including any arm after `_`) is an error.
- Outside the file's namespace, a variant value is qualified like any call:
  `{ match (fd::glob) }`, or unqualified inside a scope block
  `fd:: { ... { match (glob) } ... }`. Patterns inside the file are unqualified.
- A function's return type may start on the line after `:`, and the `{` on
  the line after the type.

## Sets

`Set (E)` is a set of variants of an enum `E`. Each variant appears at most
once. It is the way to take optional, order-independent flags: absence is the
natural state, so there is no `Option`, no sentinel default and no `some`.

```
ENUM Find Option {
    include hidden file,
    extension (List (String)),
    max depth (Int),
    size between (Size) and (Size),
    exclude (List (String))
}

fd::find "" in "src" { extension ["rs"], max depth 3 }
fd::find "" in "." { size between (1 mb) and (5 gb), include hidden file }
fd::find "main" in "src" {}
```

- A set literal uses the same `{ ... }` syntax as a record literal: anyfix
  entries separated by commas, atoms bare, anything longer parenthesized. The
  checker decides between record and `Set (E)` by matching the entries against
  the candidate overloads; a literal that fits more than one is an ambiguity
  type error.
- The duplicate check is static and syntactic. Two entries of the same variant
  in one literal is a type error naming both positions:

```
fd::find "" in "src" { max depth 1, max depth 2 }
// type error: `max depth` given twice
```

- A repeatable flag (`fd --exclude a --exclude b`) is one variant carrying a
  list: `{ exclude ["a", "b"] }`.
- Sets are combined with an explicit function (`(a) merged with (b)`, right side
  wins on a conflict), so a duplicate cannot appear by accident.
- A wrapper converts a set to arguments with one `MATCH` arm per variant. See
  *Running external commands*.

Use a record with `DEFAULT`s for values that are always present, and a
`Set (E)` for optional flags.

## Lambdas

A function value is written `(x) => body`, reusing the arrow from `MATCH` arms.
Being a slot value of more than one token, it is parenthesized:

```
flat map (patterns) with ((p) => ["--exclude", p])
```

- A lambda takes exactly one parameter name, and is written only as a
  parenthesized slot value: `((x) => ...)`.
- Its type is `Function (A) To (B)`, the type of a function parameter such as
  `f` in `flat map (items: List (T)) with (f: Function (T) To (List (U)))`.
- A lambda has no type of its own, so the parameter type comes from the
  *earlier arguments of the same call*: once `T` is bound by `items`, the body
  is checked with `x: T` and its type binds `U`. This is bottom-up (nothing
  flows in from the call's context), but it means the lambda must come after
  the argument that fixes its parameter type; otherwise it is a type error.

## Tuples and list spread

A tuple groups two or more values of possibly different types. It is written
with commas inside a slot's parentheses, so the slot's own parentheses are the
tuple's:

```
run ("fd", args)       // one tuple argument
run (("fd", args))     // the same value
```

A tuple parameter lists a name and type per component, in a second pair of
parentheses inside the slot (this is how `run` is declared below). The
components are bound as variables in the body:

```
FUNCTION run ((command: String, args: List (String))): ... { ... }
```

- A tuple has two or more components; `(x)` is just parentheses.
- Its type is written `(String, List (String))` in messages. There is no type
  syntax for it yet, so a function cannot declare a tuple *return* type, and a
  tuple parameter is the only place a tuple type can be written.
- Overloads may differ by tuple shape.
- Components are inferred like any other argument, so a record or set literal
  cannot be a tuple component.
- A tuple cannot be a slot of an enum variant or a record field.

List literals accept `...` to splice a list in place:

```
[...a, "x", ...b]
```

- `...` is only valid as an element of a list literal; its operand follows the
  anyfix rule (one token, or a parenthesized expression: `...(flat map ...)`).
- The operand must be a `List (T)`; element types are joined as for any other
  list literal. Spreading a `Set (E)` is not supported.

## Pipes

`a |> f (_) with (x)` passes `a` as the single `_` placeholder of the call on its
right; it is an error if the right side has no `_` or more than one.

```
run ("git", ["log"]) |> map (_) with ((text) => (text) lines)
```

## Prelude functions

- `flat map (items) with (f)`: `items` is a `List (T)` or a `Set (T)` (two
  overloads); `f` is `Function (T) To (List (U))`; the result is a `List (U)`.
  A set is unordered, so the result order follows the set's literal order.
- `(text) lines`: splits a `String` into a `List (String)`.
- `(n) to string`: `n` is an `Int` or a `Float`; the result is a `String`.
- `map (result) with (f)`: `result` is `Result (T) Or (E)`, `f` is
  `Function (T) To (U)`; `ok` is mapped, `error` is passed through unchanged:
  `Result (U) Or (E)`.
- `join (a) with (b)`: concatenates two `String`s, or two `List (T)` of the
  same element type. Nothing else (no mixing, no sets).
- When a call has several candidates and none fits, the error lists each
  candidate with the reason it was rejected.

## Units

A unit is an ordinary suffix function. There is no special syntax.

```
FUNCTION (n: Int) kb: Size { ... }
FUNCTION (n: Int) mb: Size { ... }
FUNCTION (n: Int) days: Duration { ... }
```

`Size` (held in bytes, decimal: `1 kb` = 1000) and `Duration` (held in seconds)
are prelude records, built only through these functions:

- `(n: Int) kb`, `mb`, `gb`: `Size`. `(n: Int) seconds`, `minutes`, `hours`,
  `days`: `Duration`.
- `(duration: Duration) ago`: `Duration`; it is the identity, a readability
  marker (fd already reads durations relative to now).
- Accessors: `(size: Size) bytes`: `Int`; `(duration: Duration) seconds`: `Int`.
  `seconds` is both a constructor (on `Int`) and an accessor (on `Duration`),
  resolved by overloading on the argument type.
- Calls do not nest bare, so `5 days ago` is written `(5 days) ago`.

Multi-token values need parentheses inside a slot, like any other call:

```
fd::find "" in "." { size between (500 kb) and (2 mb) }
```

## Overload resolution

Functions with the same name may be declared several times when their parameter
types differ. Resolution is bottom-up, from the already-inferred types of the
arguments; the expected type is never pushed down. Record and set literals take
part through their structural type.

If the resolver cannot confidently pick exactly one candidate (for example, a
`{}` literal that fits several overloads), the call is a type error. The first
declaration never wins silently.

Diagnostics should list the call site, the inferred argument types, every
candidate signature, and what to change (for example, an unknown field name
should show each candidate's fields).

## Running external commands

Wrappers around existing CLI tools are written in resh itself, on top of a
single primitive:

```
FUNCTION run ((command: String, args: List (String))): Result (String) { NATIVE "run" }
```

Stdout is returned as `Ok`; stderr and the exit code as `Err`. Precisely:

- `run` never invokes a shell: `command` is looked up on `PATH` and `args` become
  argv entries verbatim, so `;`, `$HOME`, `*` and quotes are passed literally.
  stdin is closed.
- Exit 0: `Ok(stdout)` (UTF-8, lossy; unchanged, including the trailing newline).
- Nonzero exit: `Err(text)` where text is the trimmed stderr, a newline, and
  `exit code N` (just `exit code N` if stderr is empty; `terminated by signal`
  if killed).
- Command cannot be started: `Err("cannot run <command>: <os error>")`.
- Embedders and tests can replace the implementation with
  `Session::set_runner` (see `eval::Runner`) to record argv and return canned
  results.
 A wrapper builds
a `List (String)` from its typed options and calls `run`:

```
NAMESPACE fd

ENUM Find Option {
    include hidden file,
    extension (String),
    max depth (Int),
    exclude (List (String))
}

FUNCTION (option: Find Option) argument: List (String) {
    MATCH option {
        include hidden file => ["--hidden"],
        extension (e) => ["--extension", e],
        max depth (n) => ["--max-depth", n to string],
        exclude (patterns) => flat map (patterns) with ((p) => ["--exclude", p])
    }
}

FUNCTION find (pattern: String) in (dir: String) (options: Set (Find Option)):
    Result (List (String))
{
    run ("fd", [...(flat map (options) with ((o) => (o) argument)), pattern, dir])
        |> map (_) with ((text) => (text) lines)
}
```

Callers:

```
fd::find "" in "src" { extension ["rs"], max depth 3 };
```

Language features this relies on and that may not exist yet: `flat map (items)
with (f)` over sets, `(n) to string` and `map (result) with (f)`.

## Writing a wrapper

A wrapper is a library (see "Importing libraries") for one CLI tool. Put it in
`name.resh` in the user library directory (`resh libs` prints it), starting with
`NAMESPACE name`. Look at `resh libs show git` and `resh libs show fd` for the
shape: an `ENUM` of options, a `FUNCTION (option: ...) argument: List (String)`
that maps each option to argv, and one function per command that calls `run`.
`///` comments above declarations are their docs (shown by `:symbols`). Check it
with `resh check name.resh`, then use it with `IMPORT name;`. `resh skill install`
installs a Claude Code skill that writes wrappers for you.

Wrappers run the tool in resh's current directory. Change it with
`change directory to "/path";` (it stays changed for the rest of the session);
`current directory;` returns it. A wrapper may also take a directory parameter,
as `fd::find ... in (dir)` does.

## Inspecting symbols

In the REPL, `:symbols` lists the available namespaces (`prelude` is the
built-in one), and `:symbols <namespace>` lists that namespace's records, enums
and functions with their signatures. Start `resh --json` for one JSON
object instead of text. An unknown namespace is an error on stderr that lists
the declared ones.

`///` comment lines directly above a `FUNCTION`, `RECORD` or `ENUM` (no blank
line between) are its docs, and `:symbols` shows them. In the REPL a
comment-only `///` line waits for the declaration below it. `NAMESPACE` applies per
REPL entry, so put it on the same line as the declaration:
`NAMESPACE demo; ENUM Mode { fast, slow }`.

```
resh> NAMESPACE demo; ENUM Mode { fast, slow }
resh> :symbols
namespaces
  prelude
  demo
resh> :symbols demo
demo
  enum    Mode
            fast
            slow
```

```
$ printf 'NAMESPACE demo; ENUM Mode { fast, slow }\n:symbols demo\n' | resh --json
{"namespace":"demo","symbols":[{"kind":"enum","name":"Mode","doc":null,"members":[{"signature":"fast","defaults":[]},{"signature":"slow","defaults":[]}],"returns":null}]}
```

## Naming conventions

See `CLAUDE.md`: no "of" in names; type names are title case.
