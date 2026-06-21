---
name: sync
description: Checkpoint the project — reconcile the docs this blueprint maintains against the current code, then commit and push. Invoke manually with /sync.
disable-model-invocation: true
allowed-tools: Bash, Read, Edit, Write, Grep, Glob
---

# /sync — project checkpoint

When invoked, reconcile the docs this project maintains so they match the current code, then commit and push. Invoking this skill is your authorization to do so.

## Repo health
- Run `git rev-parse --is-inside-work-tree`. If this is not a git repo, tell the user and stop — this blueprint expects one.
- Check for an `origin` remote (`git remote get-url origin`). If there is none, warn and offer to create one with `gh repo create` — do NOT create it without explicit confirmation.
- Report the current branch (warn on detached HEAD), `git fetch` (best-effort), then report ahead/behind vs the upstream.
- Show a short `git status` summary.

## Reconcile memory, then docs
**Memory first** — before any doc, refresh this env's memory so the checkpoint (and the mirror below, if any) captures what you've learned this session. Review `MEMORY.md` and the per-fact files under `$CLAUDE_CONFIG_DIR/projects/<this-project>/memory/`: add new facts, correct stale ones, prune what's wrong, and keep the one-line `MEMORY.md` index in sync. Report: memory updated / already-fresh.

Then, for each doc file below that exists, compare it against the current code and recent commits, then make it accurate. This is a **two-way reconcile, not append-only**: add what's missing, **correct what's now wrong**, and **delete what no longer applies**. Report per file: updated / already-fresh / skipped (absent).

- **CLAUDE.md** (project root) — project-specific instructions and context for this codebase. Keep it accurate as the project evolves. This is the *project* CLAUDE.md, separate from the global persona.
- **README.md** — user-facing entry point: what the project is, install, usage, and the command/feature reference. Must reflect current behavior.
- **CHANGELOG.md** — version history of user-facing changes. Add new entries under `[Unreleased]` (create it if missing). Match the file's existing style.
- **docs/** — deeper, topic-by-topic reference docs. Keep each page consistent with actual behavior; don't just duplicate the README.

## Mirror this env's internal config (tracked)
Version-control this env's internal config by mirroring it into the tracked `claude-internal/TechnicalDirector/` folder at the repo root, so the skills, memory, and persona that live in the gitignored `.claude-env-TechnicalDirector/` dir are captured in git. The folder is **namespaced per blueprint** (`claude-internal/TechnicalDirector/`) so multiple blueprints sharing this repo don't clobber each other's mirror. The live env dir stays the **single source of truth** — this is a **one-way copy** from it, refreshing only what changed.
- Self-heal first: `mkdir -p claude-internal/TechnicalDirector/skills claude-internal/TechnicalDirector/memory` — an env placed before this step won't have the folder yet, so create it.
- Mirror `.claude-env-TechnicalDirector/skills/` → `claude-internal/TechnicalDirector/skills/`.
- Mirror this env's memory dir (`.claude-env-TechnicalDirector/projects/<this-project>/memory/`) → `claude-internal/TechnicalDirector/memory/`.
- Snapshot `.claude-env-TechnicalDirector/CLAUDE.md` → `claude-internal/TechnicalDirector/persona.CLAUDE.md` — **keep this exact name**, never `CLAUDE.md`, so Claude Code does not auto-load the snapshot as a second persona.
- Stage it by explicit path: `git add claude-internal/TechnicalDirector`. This folder is tracked on purpose — it is *not* covered by the `.claude-env-*` gitignore line.

## Commit + push
- Stage **only the files you created or modified in this session**, plus any docs you reconciled above — by explicit path (e.g. `git add path/a path/b`). **Never `git add -A` / `git add .`** — a blanket stage sweeps unrelated untracked files (other tooling's scaffolding, another env's in-flight work) into your commit. Run `git status` first; unstage anything you didn't touch. Then commit with a clear message summarizing what changed.
- **End every commit message with a trailer line `Env: TechnicalDirector`** (after a blank line) so the commit records which aello blueprint made it. Your git author identity is already set to this blueprint; the trailer makes it visible in the message body too.
- **After committing, before pushing, run `git pull --rebase origin <current-branch>`** to integrate any commits the remote gained since you last fetched (e.g. a release CI's `release: vX [skip ci]` auto-bump). This replays your commit on top so the push is a fast-forward — skipping it leaves you a commit behind and the *next* `/sync` push gets rejected.
- Push to `origin` on the current branch. If the push fails for a missing upstream, set it: `git push -u origin <branch>`.
- Report the final state: branch, commit sha, push result, and the remote URL.

Use normal prose for commit messages. Don't skip hooks or force-push unless the user explicitly asks.
