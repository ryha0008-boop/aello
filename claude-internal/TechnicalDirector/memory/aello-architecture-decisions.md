---
name: aello-architecture-decisions
description: "The non-obvious WHYs behind aello: token auth (not cred-copy), manual /sync (not hooks), generated /sync, git attribution"
metadata: 
  node_type: memory
  type: project
  originSessionId: afdce7ab-1496-4a48-a656-d27d377c3496
---

Decisions you can't recover from code — do NOT redo these:
- **Auth = long-lived non-rotating CLAUDE_CODE_OAUTH_TOKEN** via `aello login`/`claude setup-token`, NOT copying `.credentials.json`. Cred-copy was tried + abandoned: OAuth refresh tokens rotate (single-use), so concurrent envs invalidate each other. The user runs MANY blueprints concurrently — this is the reason.
- **Onboarding gotcha:** token authenticates the API but interactive claude shows its first-run wizard on a fresh config dir. `mark_onboarded` seeds `hasCompletedOnboarding:true` into `.claude.json` so it skips it. (That wizard WAS the "still asking for login" bug.)
- **`aello login` tees `claude setup-token` stdout** so the auth URL is visible on a headless VPS (a plain pipe swallows it → looks hung).
- **/sync replaces auto-hooks:** helo's Stop/UserPromptSubmit auto-commit-every-turn hooks were flaky/naggy and removed. /sync is MANUAL (`disable-model-invocation:true`). Only PostCompact hook survives (transcript saver, proven reliable). Don't reintroduce enforcement hooks.
- **/sync is GENERATED per blueprint** (`templates::render_sync_skill(caps,name)`), not static variants — contains only sections for enabled caps; no-github blueprint gets zero git talk + no Bash tool.
- **Per-env git attribution** (the core multi-agent point): with `github`, launch.rs sets GIT_AUTHOR_* AND GIT_COMMITTER_* = `<blueprint> <blueprint@aello.local>`, plus an `Env:<blueprint>` commit trailer — so git blame / log --author reveal which blueprint did what.
- **Two CLAUDE.md layers:** global persona = env CLAUDE.md (set once); project = repo CLAUDE.md (--project-md). Memory is separate/automatic. [[aello-overview]]
