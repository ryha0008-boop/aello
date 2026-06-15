---
name: sync
description: Checkpoint the current project — verify repo health, reconcile the key docs against the latest code (README, CHANGELOG, docs/, CLAUDE.md, memory), then commit and push. Invoke manually with /sync.
disable-model-invocation: true
allowed-tools: Bash, Read, Edit, Write, Grep, Glob
---

# /sync — project checkpoint

When invoked, checkpoint the current project: confirm the repo is healthy, reconcile the key docs so they match the current code, then commit and push. Invoking this skill is your authorization to commit and push.

## 1. Repo health
- Run `git rev-parse --is-inside-work-tree`. If this is not a git repo, tell the user and stop.
- Check for an `origin` remote (`git remote get-url origin`). If there is none: warn that there's no GitHub remote and offer to create one with `gh repo create` — do NOT create it without explicit confirmation.
- Report the current branch (warn on a detached HEAD).
- `git fetch` (best-effort), then report ahead/behind vs the upstream if one is set.
- Show a short `git status` summary (staged / unstaged / untracked).

## 2. Reconcile the key docs (staleness check — only files that exist)
For each file below that exists, compare it against the current code and recent commits, then make it accurate. This is a **two-way reconcile, not append-only**:
- add information that's missing,
- **correct statements that are now wrong**,
- **delete content that no longer applies**.

Report per file: updated / already-fresh / skipped (absent). If nothing is stale, say so.

**File roles — keep each in its lane:**
- **README.md** — user-facing entry point: what the project is, install steps, usage, and the command/feature reference. Must reflect the current commands and features.
- **CHANGELOG.md** — version history of **user-facing** changes. Add new entries under `[Unreleased]` (create that section if missing). Match the file's existing style.
- **docs/** — deeper, topic-by-topic reference docs (only if the directory exists). Keep each page consistent with actual behavior; don't just duplicate the README.
- **CLAUDE.md** — project/agent instructions and context. Maintain freely as the project evolves; no directions needed.
- **memory files** (your memory dir — `MEMORY.md` plus its entries) — durable, cross-session project facts. Add, update, or remove as facts change.

## 3. Commit + push
- Stage changes (`git add -A`), commit with a clear, descriptive message summarizing what changed (docs and code), and push to `origin` on the current branch.
- If the push fails for a missing upstream, set it: `git push -u origin <branch>`.
- Report the final state: branch, commit sha, push result, and the remote URL.

Use normal prose for commit messages. Don't skip hooks or force-push unless the user explicitly asks.
