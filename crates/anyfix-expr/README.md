# anyfix-expr

A lexer, parser and lossless tree for **anyfix expressions**: a keyword-free
notation that only describes grouping and gives nothing a meaning.

## Motivation

Anyfix expressions are to a language what S-expressions are to Lisp and WAT: a
consumer-neutral notation. The crate builds tree structure and nothing else.
Deciding what a word means is the consumer's job. resh is the first consumer,
but this crate knows nothing about it.

`.`, `:` and `_` are syntactic sugar, like `'` in Lisp: `'x` is read as
`(quote x)` before any evaluator sees it. They fill a slot of a call while
parsing, so the consumer never sees them (see [Sugar](#sugar)).

Consumers read a tree of calls, not flat runs. A run's words name the call
and its other items fill the slots, and symbol words bind looser than the
rest: `x < y < z` is the call `_ < _ < _` of `x`, `y` and `z` (see
[Tree](#tree)).

## Design principles

1. **The syntax has no meaning.** Tokens and operators only build tree
   structure. There are no keywords and no special identifiers (`true` and
   `let` are ordinary words). No evaluation, holes, namespaces or types. The
   one exception is the sugar of `.`, `:` and a bare `_`, which is rewritten
   away and is meaning-free like Lisp's `'`.
2. **Letter case is never significant.** Words keep the text as written, and
   nothing in the notation depends on case. String escapes are
   case-insensitive (`\N` is `\n`, `\U{4a}` is `\u{4A}`).
3. **The word boundary is conventional.** A word is all name characters
   (letters, digits, `_`) or all symbol characters. Switching kinds ends it, so
   `x+1` is three words. There's no kebab-case: multi-word names are runs, like
   `flat map`.
4. **Exactly eleven special characters:** `, : . ( ) [ ] { } # "`. They never
   appear inside a word.
5. **Exactly two operators:** `.` (left-assoc) binds tighter than `:`
   (right-assoc). Both take whole runs as operands, and both are sugar.
   Symbol words are not operators: they split a run into slots but have no
   precedence or associativity among themselves.
6. **One separator, `,`**, at the top level and inside every bracket. Trailing
   commas are allowed. `;` is an ordinary symbol word.
7. **Newlines are plain whitespace.** They never end anything.

## Grammar

```ebnf
(* Structure *)
document   = sequence ;
sequence   = [ expr { "," expr } [ "," ] ] ;     (* top level and inside every bracket *)
expr       = dotted [ ":" expr ] ;                (* ":" right-assoc, loosest *)
dotted     = run { "." run } ;                    (* "." left-assoc *)
run        = item { item } ;                      (* the anyfix run *)
item       = word | number | string
           | "(" sequence ")"
           | "[" sequence "]"
           | "{" sequence "}" ;

(* Lexical *)
special    = "," | ":" | "." | "(" | ")" | "[" | "]" | "{" | "}" | "#" | '"' ;
alnum      = letter | digit | "_" ;
symbol     = ? any char that is not whitespace, special, or alnum ? ;
word       = name | symbols ;
name       = ( letter | "_" ) { alnum } ;
symbols    = symbol { symbol } ;
number     = digit { alnum | "." digit } ;        (* source text kept *)
string     = '"' { ? any char except '"' or "\" ? | escape } '"' ;
escape     = "\" ( '"' | "\" | "n" | "t" | "r" | "u{" hex { hex } "}" ) ;
comment    = "#" { ? any char except newline ? } ;
whitespace = ? char::is_whitespace ? ;
letter     = ? char::is_alphabetic ? ;
digit      = "0" … "9" ;                          (* ASCII only *)
```

## Tokenization rules

1. Comments and whitespace separate tokens and are kept as trivia. `#` always
   starts a comment, even mid-token: `a#b` is the word `a` followed by a
   comment. A shebang line is just a comment.
2. Each special character is its own token, except that `"` starts a string
   and `#` starts a comment.
3. A word is as long as possible within its kind (name or symbols). Switching
   kinds ends it, with or without a space.
4. A token starting with an ASCII digit is a number. Inside a number, `.` is
   consumed only when an ASCII digit follows: `1.5` and `1.2.3` are single
   numbers, but `1.to` is `1`, `.`, `to`. Numbers keep their source text, so
   `0xFF`, `1_000`, `2.5e3` and `1mb` are all numbers whose meaning the
   consumer decides.
5. Numbers have no sign. `-5` is `Word "-"` then `Number "5"`, the same tree as
   `- 5`.
6. A name starts with a `char::is_alphabetic` letter or `_`, and continues with
   `char::is_alphanumeric` or `_`. So `x٣` is one name, while `٣` on its own
   (alphanumeric but not alphabetic) is a symbol word.
7. `\u{…}` takes 1 to 6 hex digits and must be a valid Unicode scalar value.
   Strings may contain literal newlines. No interpolation and no raw strings in
   v1.

## Sugar

`.` and `:` each hand their left value to the run on their other side, as an
argument of the call that run becomes:

- If that run has a bare `_` at its top level, the value fills that slot.
- Otherwise a new slot goes at the front for `.` and at the end for `:`.

| Source | Same as |
| --- | --- |
| `x . y (z)` | `(x) y (z)` |
| `x . y _` | `y (x)` |
| `f : x` | `f (x)` |
| `_ f : x` | `(x) f` |

A `_` inside brackets belongs to those brackets (as in Scala) and is not
consumed. It reaches the consumer as an ordinary `Underscore`. A run that
receives a value may hold at most one bare `_`; a second is `ExtraUnderscore`.

This lets a consumer define infix operators as plain functions:
`1 .+ 1 .+ 2` is `((1) + 1) + 2`, the call `_ + _` of `_ + _` and `2`. There
is no precedence: operators apply left to right.

A consumer should not do recursive descent parsing. `.` and `:` already
provide associative chaining, so the tree arrives fully grouped. The consumer
walks it and matches each call's name against its own signatures. When
designing its syntax, a consumer that wants associative chaining should express
it with `.` (left-assoc) or `:` (right-assoc), not with operators or keywords
of its own that would need precedence parsing.

## Tree

Consumers read the abstract view, `SyntaxTree::nodes()`: a document is a list
of nodes, its top-level sequence. It is the tree after the sugar is applied,
with every run read as a call.

```text
Node = Number     (source text)
     | String     (decoded value)
     | Call       { name: [NamePart], arguments: [Node] }
     | Parens     [Node]        // `()`, `(a, b)` or `(a,)`
     | Brackets   [Node]
     | Braces     [Node]
     | Underscore               // a `_` that no `.` or `:` consumed
     | Missing                  // only where an error left a hole

NamePart = Word { text, span }
         | Slot                 // filled by the next argument, in order
```

A run becomes a call in two steps:

1. **Symbol words split the run.** Symbol words (`<=`, `+`, `->`) bind looser
   than everything else, so the items between them gather into one argument
   each. `my weight <= max (all) < 10` is the call `_ <= _ < _` of
   `my weight`, `max (all)` and `10`. Symbol words carry no precedence among
   themselves, so a consumer declares the whole shape `(a) <= (b) < (c)`.
2. **Words name the call.** Every name word becomes part of the name. Every
   other item (number, string, bracket, `_`, or a value from `.` or `:`) fills
   a slot. `list file in (dir)` is the call `list file in _` of `dir`, and
   `my weight` is the call `my weight` with no arguments.

- A word is a call with no arguments. A run of one non-word item is that item,
  never a call.
- Parens around one element with no comma only group: `(x)` is `x`. The
  exception is `(_)`, which stays `Parens[_]` because the parens mark the
  `_`'s scope.
- `Call::name_text()` joins the name with spaces and writes `_` for each slot:
  `_ <= _ < _`, `list file in _`. A lone `_` is never a word, so the text is
  unambiguous even though words like `x_` may contain `_`.
- A name word mixing symbol and name characters is impossible: `null?` is the
  call `_ ?` of `null`.
- Every node and name word carries a byte `Span { start, end }`.
  `line_column(source, offset)` converts an offset to a 1-based line and
  column (counted in characters).
- `Missing` stands for an empty operand or element. It appears only when
  `Parsed::errors` is not empty.

## Groupings that must hold

Calls are written as s-expressions with the name first, in backticks when it
has spaces. A call with no arguments is just its name.

| Source | Tree |
| --- | --- |
| `x < y < z` | ``(`_ < _ < _` x y z)`` |
| `my weight <= average (all weights) < max (all weights)` | ``(`_ <= _ < _` `my weight` (`average _` `all weights`) (`max _` `all weights`))`` |
| `a b . c d . e f` | ``(`_ e f` (`_ c d` `a b`))`` |
| `a : b : c` | ``(`a _` (`b _` c))`` |
| `k : x . f` | ``(`k _` (`_ f` x))`` |
| `search { max depth : 3 }` | ``(`search _` Braces[(`max depth _` 3)])`` |
| `x . y _` | ``(`y _` x)`` |
| `_ f : x` | ``(`_ f` x)`` |
| `x . f (_)` | ``(`_ f _` x Parens[_])`` |
| `1 .+ 1 .+ 2` | ``(`_ + _` (`_ + _` 1 1) 2)`` |
| `-5` | ``(`- _` 5)`` |
| `x+1` | ``(`_ + _` x 1)`` |
| `a->b` | ``(`_ -> _` a b)`` |
| `null?` | ``(`_ ?` null)`` |
| `f(x)` | ``(`f _` x)`` |
| `1.to` | ``(`_ to` 1)`` |
| `1.2.3` | `Number "1.2.3"` |
| `rg.search "TODO" in "src"` | ``(`_ search _ in _` rg "TODO" "src")`` |
| `fd.(option) argument` | ``(`_ _ argument` fd option)`` |
| `("rg", x)` | `Parens["rg", x]` |
| `(x)` | `x` |
| `()`, `[]`, `{}` | empty sequences |
| `[a, b,]` | `Brackets[a, b]` |

## Lossless tree

`parse(source).syntax.to_source()` reproduces the input byte for byte, for
every input, valid or not. The concrete tree (`Sequence`, `Element`,
`Expression`, `Run`, `Item`, `Group`, `Token`) keeps every token, commas,
brackets and operators included. Each token has leading trivia (everything
since the previous token's trailing trivia) and trailing trivia (the
whitespace and comment after it on the same line). The document's final trivia
sits on the `End` token. Tokens skipped during recovery are kept as
`Item::Skipped`. The abstract view is derived from this tree.

## Diagnostics

Parsing recovers and reports every error, sorted by position. Each `Error` has
a stable `ErrorCode`, a span, and a one-line message for an agent: what was
found, what was expected, and the fix.

| Code | Example | Span and recovery |
| --- | --- | --- |
| `UnclosedBracket` | `f (a` | The opener. The group ends at the end of input, or before a closer that matches an outer bracket (`[(a]`). |
| `MismatchedBracket` | `(a]` | The closer, which still closes the group. The message names both brackets. |
| `UnexpectedCloser` | `a)` | The closer, which is skipped. |
| `EmptyOperand` | `a..b`, `. a`, `a :`, `: b`, `a::b` | The operator next to the missing run, which becomes `Missing`. |
| `EmptyElement` | `a,,b`, `[, a]` | The comma with nothing before it. A trailing comma is fine. |
| `ExtraUnderscore` | `x . f _ _`, `_ f _ : x` | The second bare `_` in a run that receives a value. It stays an `Underscore`. |
| `UnterminatedString` | `"abc` | The string, which runs to the end of input. |
| `InvalidEscape` | `"\q"`, `"\u{110000}"` | The escape, kept as written in the decoded value. |
| `TooDeep` | 1000 nested `(` | The first bracket deeper than `MAX_DEPTH` (256). It and its contents are skipped, so the stack never overflows. |

The notation never reports semantic problems. An unknown word or a `:` in an
odd place is the consumer's job.

## The grammar in anyfix expressions

The grammar written in anyfix expressions (`tests/fixtures/grammar.anyfix`). A
grammar consumer would read each `name _` call as a rule, a `_ | _` call as
alternation, any other call as a sequence of its words and arguments in order,
`( )` as grouping, `[ ]` as optional and `{ }` as repetition. This crate only
has to parse it.

```text
# Anyfix expressions — described in anyfix expressions.

document : sequence,

sequence : [ expr { "," expr } [ "," ] ],
expr     : dotted [ ":" expr ],
dotted   : run { "." run },
run      : item { item },

item     : word | number | string
         | "(" sequence ")"
         | "[" sequence "]"
         | "{" sequence "}",

special    : "," | ":" | "." | "(" | ")" | "[" | "]" | "{" | "}" | "#" | "\"",
alnum      : letter | digit | "_",
symbol     : any char except (whitespace | special | alnum),

word       : name | symbols,
name       : (letter | "_") { alnum },
symbols    : symbol { symbol },

number     : digit { alnum | "." digit },

string     : "\"" { any char except ("\"" | "\\") | escape } "\"",
escape     : "\\" ("\"" | "\\" | "n" | "t" | "r" | "u{" hex { hex } "}"),

comment    : "#" { any char except newline },
whitespace : unicode whitespace,

letter     : unicode letter,
digit      : "0" to "9",
```

## JSON

Most JSON parses as anyfix expressions: objects are braces of `_ _` calls
(`"key" : value` is the call `_ _` of the key and the value), arrays are
brackets, and `true`, `false`, `null` are calls with no arguments. The
exceptions:

- Exponent numbers like `1e-5` split at the sign into the call `_ - _` of
  `Number "1e"` and `Number "5"`, and a negative number is the call `- _` of
  a number.
- JSON's `\uXXXX`, `\/`, `\b` and `\f` escapes are not anyfix escapes and
  report `InvalidEscape`; anyfix writes code points as `\u{…}`.

## Usage

```rust
let parsed = anyfix_expr::parse("search { max depth : 3 }");
assert!(parsed.errors.is_empty());
let nodes = parsed.syntax.nodes();
let anyfix_expr::NodeKind::Call(call) = &nodes[0].kind else { unreachable!() };
assert_eq!(call.name_text(), "search _");
assert_eq!(parsed.syntax.to_source(), "search { max depth : 3 }");
```
