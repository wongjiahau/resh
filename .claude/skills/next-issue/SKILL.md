---
name: next-issue
description: Start the self-paced loop that works on the next simplest GitHub issue. Use when the user types /next-issue or asks to work through the GitHub issues.
---

# next-issue

Shortcut for running the loop below.

## Starting

When invoked directly (not from inside a running loop), invoke the `loop` skill (Skill tool, skill: "loop") with no interval and args: `follow the "Loop body" in .claude/skills/next-issue/SKILL.md`. Do not do the steps yourself in this invocation. If the user gave a specific issue (e.g. `work on #68`), only use it for the first iteration; the loop args stay exactly as above so later iterations pick the next issue.

## Loop body

Work on the next simplest GitHub issue:

1. Identify the issue.
2. Put a comment in the issue to notify the other agent that you're working on it (to avoid duplicated work).
3. Use a worktree instead of a branch.
4. Before opening the PR, run only the e2e scenarios affected by your change (the pre-commit hook no longer runs e2e): find the `app/e2e/features/*.feature` files covering the tabs/areas you touched, plus any you added or changed, and run just those, e.g. `cd app && NODE_OPTIONS="--experimental-sqlite --no-warnings" bunx bddgen && TEST_PORT=<free port> bunx playwright test <feature name or --grep>`. Use your own `TEST_PORT` (default 4173) so parallel agents don't collide. The new/changed scenario must pass. Never run the full e2e suite (`bun run test`, `check:all`) or `just deploy` yourself; the full suite runs only via `deploy`.
5. Create a PR (with before/after screenshots), and list which e2e features you ran in its description.
   - Screenshots are mandatory (a before and an after, both of the thing the issue is about, cropped to the relevant area, not whole pages). If a change is not visible, say so in the description instead. Take "before" from `master` (or by temporarily undoing the fix in the page), never skip it.
   - Commit the images under `pr-assets/<issue number>/` on the PR branch and push, then embed them in the description with Markdown image syntax so GitHub renders them inline (a bare link or path is not enough): `![before](https://github.com/wongjiahau/temple-registration-system/blob/<branch>/pr-assets/<N>/before.png?raw=true)`. `gh` cannot upload images, so this is the way. After `gh pr create`/`gh pr edit`, check the body contains `![` for each image.
6. Launch the dev server with the test fixture (ensure the port is different from any other agent running the same loop): `cd app && FIXTURE=1 PORT=<free port> just dev`. It opens `app/fixtures/fixture.sqlite` (fake data, edits stay in memory) automatically. The page address carries the tab: `#?tab=<outer tab key>&inner=<inner tab key>` (keys from `TAB_KEYS` in `app/src/main.ts`; inner keys such as `pre` / `actual`), so a link opens straight on it.
7. Notify me to test, giving the full link to the tab your change touches, e.g. `http://localhost:<port>/#?tab=flags&inner=actual`, so I land on it directly.
8. Wait for me to approve or give feedback.
9. Once approved, merge the PR, clean up (stop the dev server, remove the worktree), then **keep looping**: schedule the next iteration (ScheduleWakeup with the same `/loop` prompt) and start again from step 1 with the next simplest open issue. Never stop the loop after one issue; stop only when no workable open issue is left (or the user says stop). While waiting for approval, schedule a long fallback wakeup (~1800s).
