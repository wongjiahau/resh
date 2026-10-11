---
name: new-issue
description: Create a GitHub issue for this repo from the user's rough description (and optional screenshot). Use whenever the user says "create a new github issue", "new issue", or describes a bug/feature to be filed.
---

# new-issue

Turn the user's rough description into a precise GitHub issue with `gh issue create`.

1. **Restate the intent in your own, more accurate wording.** Do not echo the user's phrasing. Look at the code (`app/src/*-tab.ts`, `app/src/index.html`) so the issue names the real tab, component/function, symptom and expected result. Use the labels shown in the UI (e.g. 掛號, 掛號單, 音源) and say which tab is meant if the names are close.
2. **Title:** `app: <tab/area> — <concise problem or feature>`, matching existing issue titles.
3. **Body:** Where (file/function), Symptom or Request, Likely cause (only if found in the code; mark unconfirmed guesses as such), Expected.
4. **Screenshots:** `gh` cannot upload images. If the user gave one, say in the issue that it is not attached, and tell the user in the reply (offer: commit it to the repo, or they drag it into the issue in the browser).
5. Do not implement the change. Reply with the issue URL and a short summary of how you restated it.
