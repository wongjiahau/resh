---
name: next-issue
description: Start the self-paced loop that works on the next simplest GitHub issue. Use when the user types /next-issue or asks to work through the GitHub issues.
---

# next-issue

Shortcut for running the loop below.

## Starting

When invoked directly (not from inside a running loop), invoke the `loop` skill (Skill tool, skill: "loop") with no interval and args: `follow the "Loop body" in .claude/skills/next-issue/SKILL.md`. Do not do the steps yourself in this invocation. If the user gave a specific issue (e.g. `work on #12`), only use it for the first iteration; the loop args stay exactly as above so later iterations pick the next issue.

## Loop body

Work on the next simplest GitHub issue:

1. Identify the issue.
2. Put a comment in the issue to notify the other agent that you're working on it (to avoid duplicated work).
3. Use a worktree instead of a branch.
4. Implement the change. Add or update tests in `tests/language.rs` that cover it. Run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` and `cargo test`; all must pass (the lefthook pre-commit runs the same three).
5. Create a PR. In the description, link the issue (`Closes #N`) and show a before/after example as `resh -c '...'` commands with their actual output. Never skip the "before"; take it from `master`.
6. Notify me to try it, giving the exact `cargo run -q -- -c '...'` command(s) (run from the worktree) that demonstrate the change.
7. Wait for me to approve or give feedback.
8. Once approved, merge the PR, clean up (remove the worktree), then **keep looping**: schedule the next iteration (ScheduleWakeup with the same `/loop` prompt) and start again from step 1 with the next simplest open issue. Never stop the loop after one issue; stop only when no workable open issue is left (or the user says stop). While waiting for approval, schedule a long fallback wakeup (~1800s).
