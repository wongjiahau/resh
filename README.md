# resh

A readable shell.

## Main goal

**The best shell for AI agents.** resh aims to be what an agent reaches for
instead of bash: typed calls checked before they run, diagnostics an agent can
act on without guessing, and output that costs few tokens. The objectives below
serve this goal.

## Objectives

- **Readable shell commands.** Calls read like plain phrases (anyfix names,
  `list file in (dir)`), not flag soup or terse abbreviations. Features such as
  namespaces serve this goal: they group wrapped CLI tools (`git.commit`), and
  deeply chained forms like `a.b.c` are valid but not the intended style.
- **Fast type-checking and fast execution.** Type checking is bottom-up and
  one-way (inference flows from leaves to roots only). No bidirectional type
  checking, since it is expensive.
- **AI-friendly (the main goal).** Diagnostics are written for agents as much as for humans.
  Readability may be sacrificed to pack more context into an error: the call
  site, the inferred argument types, every candidate signature, and what to
  change, so an agent can fix the code without guessing.
- **Function overloading by argument types.** Functions with the same name may
  be declared multiple times, as long as their parameter types differ. The
  overload is resolved bottom-up from the already-inferred types of the
  arguments.

## Install

Homebrew (macOS, Linux):

```
brew install wongjiahau/resh/resh
```

or from source:

```
cargo install --git https://github.com/wongjiahau/resh
```

The wrapper libraries (`fd`, `git`, `herdr`, `rg`, `sed`) and the language prelude
are built into the binary; load a wrapper with `IMPORT git` (see `SYNTAX.md`,
"Importing libraries"). `resh libs` lists them.

### Writing wrappers for your own tools

Wrappers are plain `.resh` files in `~/.config/resh/lib/` (`resh libs` prints the
exact directory). To have Claude Code write one for a CLI tool:

```
resh skill install
```

then ask Claude to "wrap `jq` for resh". The skill reads the tool's `--help`,
writes `~/.config/resh/lib/jq.resh`, and checks it with `resh check`.
