---
name: aello-migration-help-todo
description: "TODO: build a migration help page documenting the full step-by-step process for migrating an EXISTING repo onto aello (validated live on cleaning-website 2026-06-21)"
metadata:
  type: project
---

**Build a migration help page** (docs/ + README link) that clearly walks a user through migrating an **existing** project/repo onto aello — not just `add`/`run` reference, the whole end-to-end flow. Validated live migrating `cleaning-website` (repo `omg-solutions`) on 2026-06-21; capture exactly these steps + gotchas.

**The validated flow:**
1. `aello add <Name>` — create the blueprint; at the caps checklist tick the full set (`github, changelog, project-md, docs, readme`).
2. From inside the project dir: `aello run <Name> -- --version` — places the env (`.claude-env-<Name>/`) without starting a real session. This sets git attribution, generates `/sync`, marks onboarded, adds `.claude-env-*` to `.gitignore`, and **seeds CI scaffolding** (`.github/workflows/version.yml`, `VERSION` 0.1.0, `CHANGELOG.md` stub, `.gitattributes`).
3. Seed memories — manually copy the old env's memory store into the new one (same project-slug path): `cp -r <old-env>/projects/<slug>/memory/. <new-env>/projects/<slug>/memory/`.
4. `aello run <Name>` — real session; verify context loads (env CLAUDE.md + project CLAUDE.md + MEMORY.md).
5. **Baseline commit** of the seeded scaffolding (one scoped commit: `.github/ VERSION .gitattributes .gitignore CHANGELOG.md` + `Env:` trailer), then push — activates CI.
6. Ongoing: `/sync` for doc/changelog upkeep.

**Gotchas the help page MUST call out (each one tripped the user):**
- **`/sync` will NOT bootstrap the CI scaffolding itself.** Its commit step stages only the docs it maintains (`CLAUDE.md`/`README.md`/`CHANGELOG.md`/`docs/`) + session-touched files, and is explicitly forbidden from `git add -A`. The seeded `.github/`/`VERSION`/`.gitattributes` look like "another tool's untracked files" to that rule → it skips them. So the scaffolding needs ONE deliberate first commit. (This is the single most confusing point — lead with it.)
- **Attribution requires running git INSIDE the aello-launched session.** `GIT_AUTHOR/COMMITTER=<Name>` only exist in that session's env. Committing from a plain terminal → authored as the user's default identity, attribution lost.
- **First push triggers the CI bump loop** (`version.yml` bumps VERSION + commits `release: vX [skip ci]`), so local ends up 1 behind → `git pull --rebase`. `/sync` already does this; a manual baseline commit must too.
- **A cold `/sync` is NOT a no-op** — it's a full doc-reconcile (two-way: add/correct/delete) that edits files + pushes autonomously under bypass-permissions. Offer a **review-first variant**: reconcile docs, show `git diff`, STOP before commit (revert with `git checkout --`); commit only after approval.
- **aello does NOT create the repo/remote** — migration assumes an existing git repo + `origin`. (Repo-creation is still the deferred roadmap item; `/sync` only *offers* `gh repo create`.)

Note: the CI/scaffolding seeding is now MORE complete than `migrate.md §5` claims (which lists it as deferred) — update that doc too. See [[aello-ci-release]], [[aello-architecture-decisions]].
