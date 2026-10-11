---
name: new-issue
description: Create a GitHub issue for this repo from the user's rough description. Use whenever the user says "create a new github issue", "new issue", or describes a bug/feature to be filed.
---

# new-issue

Turn the user's rough description into a precise GitHub issue with `gh issue create`.

1. **Restate the intent in your own, more accurate wording.** Do not echo the user's phrasing. Look at the code (`src/lexer.rs`, `parser.rs`, `check.rs`, `eval.rs`, `prelude.resh`, `tests/language.rs`) so the issue names the real stage (lexing, parsing, type checking, evaluation, prelude/stdlib, CLI), function and symptom. Use resh's own terms: anyfix function, record, enum, signature, pipe, `(self)`.
2. **Title:** `<area>: <concise problem or feature>` (e.g. `check: ...`, `prelude: ...`, `parser: ...`), matching existing issue titles.
3. **Body:** Where (file/function), Symptom or Request (include a minimal `resh -c '...'` reproduction and its actual vs expected output when relevant), Likely cause (only if found in the code; mark unconfirmed guesses as such), Expected.
4. **Screenshots:** `gh` cannot upload images. If the user gave one, say in the issue that it is not attached, and tell the user in the reply.
5. Do not implement the change. Reply with the issue URL and a short summary of how you restated it.
