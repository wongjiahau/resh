# resh

A readable shell.

## Objectives

- **Readable shell commands.** Calls read like plain phrases (anyfix names,
  `list file in (dir)`), not flag soup or terse abbreviations. Features such as
  namespaces serve this goal: they group wrapped CLI tools (`git:commit`), and
  deeply chained forms like `a:b:c` are valid but not the intended style.
- **Fast type-checking and fast execution.** Type checking is bottom-up and
  one-way (inference flows from leaves to roots only). No bidirectional type
  checking, since it is expensive.
- **AI-friendly.** Diagnostics are written for agents as much as for humans.
  Readability may be sacrificed to pack more context into an error: the call
  site, the inferred argument types, every candidate signature, and what to
  change, so an agent can fix the code without guessing.
- **Function overloading by argument types.** Functions with the same name may
  be declared multiple times, as long as their parameter types differ. The
  overload is resolved bottom-up from the already-inferred types of the
  arguments.
