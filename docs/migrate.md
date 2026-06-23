# Migrating an existing project

This walks you through putting an **existing** repo under an aello blueprint — not the `add`/`run` reference, the whole end-to-end flow, including the parts that trip people up. (Starting a brand-new project? You don't need any of this; just `aello run <name>` in the directory.)

## Before you start

- You have an **existing git repo with an `origin` remote**. If the project isn't a repo / has no remote yet, run `aello github-setup` first — it creates the GitHub repo, sets `origin`, and pushes an initial commit.
- For the `github` capability, `gh` is installed and authenticated (`gh auth status`).
- You're logged in to aello (`aello login`) so envs share one token.

## The flow

### 1. Create the blueprint

```sh
aello add <Name>            # or use the TUI: press [A]
```

At the capability checklist, tick the full set for a normal project repo: `github, changelog, project-md, docs, readme`. (Pick a built-in persona — `coder` or `sysadmin` — or leave it none.)

### 2. Place the env without starting a session

From **inside the project directory**:

```sh
aello run <Name> -- --version
```

`-- --version` passes straight through to Claude, which prints its version and exits — so this *places* the env without burning a real session. Placement:

- creates `<project>/.claude-env-<Name>/` (the env dir) and adds `.claude-env-*` to `.gitignore`,
- sets up git attribution, generates `/sync`, seeds the universal `/handoff`, marks onboarding complete,
- **seeds the `github` CI scaffolding**: `.github/workflows/version.yml`, `VERSION` (`0.1.0`), a `CHANGELOG.md` stub, and `.gitattributes`,
- seeds a starter working-style memory.

### 3. Carry over memory (optional)

Migrating from another env? Copy its memory store into the new one at the same project-slug path:

```sh
cp -r <old-env>/projects/<slug>/memory/. <new-env>/projects/<slug>/memory/
```

### 4. Open a real session and sanity-check context

```sh
aello run <Name>
```

Confirm `/context` shows the env persona (`<env>/CLAUDE.md`), the project `CLAUDE.md`, and `MEMORY.md`.

### 5. Baseline-commit the scaffolding (do this once — see the gotcha below)

Inside the aello session, commit the seeded files in one scoped commit and push:

```sh
git add .github/ VERSION .gitattributes .gitignore CHANGELOG.md
git commit -m "chore: adopt aello CI scaffolding"   # the Env: trailer is added by attribution
git push
```

This first push activates CI. (See "the CI bump loop" below.)

### 6. Ongoing

From here, use `/sync` for doc/changelog upkeep. The scaffolding is committed, so `/sync` just maintains it going forward.

## Gotchas (each of these has bitten someone)

- **`/sync` will NOT bootstrap the scaffolding for you.** Its commit step stages only the docs it maintains (`CLAUDE.md`/`README.md`/`CHANGELOG.md`/`docs/`) plus files touched in the session — it is explicitly forbidden from `git add -A`. The freshly-seeded `.github/`/`VERSION`/`.gitattributes` look like another tool's untracked files, so `/sync` skips them. That's why step 5 is a deliberate, one-time commit. **This is the single most confusing point.**
- **Attribution only works inside the aello-launched session.** `GIT_AUTHOR_*`/`GIT_COMMITTER_*` (= `<Name> <Name@aello.local>`) exist only in that session's environment. Commit from a plain terminal and the change is authored as your default git identity — attribution lost.
- **The first push triggers the CI bump loop.** `version.yml` bumps `VERSION` and commits `release: vX [skip ci]`, so your local branch ends up one commit behind → `git pull --rebase`. `/sync` already does this; the manual baseline commit needs it too.
- **A cold `/sync` is not a no-op.** It's a full two-way doc reconcile (add / correct / delete) that edits files and pushes autonomously under bypass-permissions. If you want to inspect first, ask for a review-first run: reconcile docs, show `git diff`, and stop before committing (revert with `git checkout --`); commit only after you approve.
- **`VERSION` is the single source of truth for the version.** Don't duplicate it into a second tracked file (a README badge, `package.json`, a generated `version.ts`) — CI bumps `VERSION` on every push and `/sync` never `git add -A`s, so a duplicate strands dirty. Derive any other stamp from `VERSION` at build time and gitignore the derived file. (See `concepts.md` → "Tracked source of truth vs derived artifacts".)
