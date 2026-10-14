# resh syntax

Design notes for the syntax. Sections marked **(planned)** are decided but not
yet implemented. There are no open questions at the moment.

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

FUNCTION commit (message: String): Result (Commit) Or (String) { ... }
```

- `NAMESPACE name` applies file-wide: it must come first in the file, and every
  declaration in the file belongs to it. One namespace per file. Members do not
  repeat the prefix.
- Namespaces are written with `::`. Single `:` stays the type annotation.
- Everything can be namespaced: functions, types, enums, records.
- Outside the namespace, names are qualified. Inside the file, they resolve
  unqualified.
- There is no `USE`; callers always qualify.
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
git::commit "fix bug"
LET c: git::Commit = ...
```

## Let bindings

`LET name = value` binds a single-word name. A type may be stated with
`LET name: Type = value`, which means `LET name = (value : Type)`.

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
a single-case `MATCH`:

```
MATCH config {
    { host (h), port (p), timeout (t) } => ...
}
```

- The pattern has one phrase per field, with a `(name)` binder in each slot.
  Every field must be listed, in any order; `(_)` ignores a slot.
- A record pattern is the only arm; there is no wildcard or second case.

## Sets (planned)

`Set (E)` is a set of variants of an enum `E`. Each variant appears at most
once. It is the way to take optional, order-independent flags: absence is the
natural state, so there is no `Option`, no sentinel default and no `some`.

```
ENUM Find Option {
    include hidden file,
    extension (String),
    max depth (Int),
    size between (Size) and (Size),
    exclude (List (String))
}

fd::find "" in "src" { extension "rs", max depth 3 }
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

## Prelude functions

- `flat map (items) with (f)`: `items` is a `List (T)` or a `Set (T)` (two
  overloads); `f` is `Function (T) To (List (U))`; the result is a `List (U)`.
  A set is unordered, so the result order follows the set's literal order.
- `(n) to string`: `n` is an `Int` or a `Float`; the result is a `String`.
- `map (result) with (f)`: `result` is `Result (T) Or (E)`, `f` is
  `Function (T) To (U)`; `ok` is mapped, `error` is passed through unchanged:
  `Result (U) Or (E)`.
- `join (a) with (b)`: concatenates two `String`s, or two `List (T)` of the
  same element type. Nothing else (no mixing, no sets).
- When a call has several candidates and none fits, the error lists each
  candidate with the reason it was rejected.

## Units (planned)

A unit is an ordinary suffix function. There is no special syntax.

```
FUNCTION (n: Int) kb: Size { ... }
FUNCTION (n: Int) mb: Size { ... }
FUNCTION (n: Int) days: Duration { ... }
```

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
FUNCTION run ((command: String, args: List (String))): Result (String) Or (String) { NATIVE "run" }
```

Stdout is returned as `Ok`; stderr and the exit code as `Err`. A wrapper builds
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
    Result (List (String)) Or (String)
{
    run ("fd", [...(flat map (options) with ((o) => (o) argument)), pattern, dir])
        |> map (_) with (text) lines
}
```

Callers:

```
fd::find "" in "src" { extension "rs", max depth 3 }
```

Language features this relies on and that may not exist yet: `flat map (items)
with (f)` over sets, `(n) to string` and `map (result) with (f)`.

## Naming conventions

See `CLAUDE.md`: no "of" in names; type names are title case.
