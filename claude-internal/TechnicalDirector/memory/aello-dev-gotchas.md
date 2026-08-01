---
name: aello-dev-gotchas
description: "Windows dev gotchas for aello — locked-exe install workaround, print-mode doesn't load persistent memory, universal skills don't backfill, gh issue view needs --json, Claude cwd-encoding folds all non-alphanumerics to '-'"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 72999d30-dfc8-4f50-aac5-cbe588b64645
  modified: 2026-08-01T11:01:28.018Z
---

Two non-obvious things when developing/testing aello on Windows:

1. **`cargo install --path . --force` fails with "Access is denied" when any `aello.exe` is running** (live TUI/run sessions hold the binary). Fix: rename the running exe out of the way first — `Rename-Item ~/.cargo/bin/aello.exe aello.exe.old-<tag> -Force` — then `cargo install`. Windows allows *renaming* a running exe (just not overwriting/deleting). aello's startup sweep cleans `aello.exe.old*` later. Alternatively use `aello update` (it does the rename trick internally, pulling the released binary).

2. **Claude Code print mode (`claude -p`, i.e. `aello run <bp> -p "..."`) does NOT inject persistent project memory** (`<env>/projects/<slug>/memory/MEMORY.md`). Only interactive sessions load it. Verified by control test: even an env with memory definitely loaded (TechnicalDirector) reports "NO MEMORY LOADED" under `-p`. So you can't use `-p` to verify a memory-seeding feature — verify by path-identity instead (the seeded `projects/<slug>/memory/` dir must match the slug Claude writes its session `.jsonl` into; `sessions::encode_project_path` produces it). See [[aello-ci-release]].

3. **`gh issue view <n>` fails on this machine** with `authentication token is missing required scopes [read:project]` (and sometimes API timeouts) — the default view tries to fetch linked project data. Read issues with `gh issue view <n> --json title,body -q '.title + "\n\n" + .body'`, and `gh issue list` for the list. Commits with `Closes #n` trailers still auto-close the issues on push regardless.

4. **A new universal skill (`/handoff`, `/twosentences`, `/note`, …) does NOT backfill to already-placed envs.** `place()` re-seeds the universal skills on every run, but only for the env being placed, and only via the *installed* binary — so existing env dirs across the machine stay stale until you `aello run` each one again with a freshly `cargo install`ed binary. The user runs ~29 envs across ~20 repos and does NOT want to re-run them. When you add a universal skill, **seed it directly** into every existing env: `find /c/Users/H -maxdepth 6 -name .aello.toml`, derive the blueprint name from the `.claude-env-<name>` dir, and write `<envdir>/skills/<skill>/SKILL.md` with the name substituted (the per-blueprint name is woven into the skill body). Design note for `/note`: it **overwrites** `<target>.NOTE.md` (single current note), not appends — the user reads a note the moment it's left, so accumulation is unwanted.

5. **`aello update` cannot deliver work that lives on a branch — and it fails silently.** It fetches the latest *release*, and CI only builds on pushes to `main`. Testing a branch feature means `cargo install --path . --force` (see #1), not `update`. The trap is that the released version can be *higher* than the branch build (2026-08-01: installed 0.1.52 from a release vs 0.1.51 on `launch-prep`), so `aello update` looks like it upgrades while actually removing the feature you wanted to test. Check `aello <cmd> --help` for the new flag, not the version number. After `cargo install`, the local build's lower version is cosmetic — CI renumbers on merge.

6. **Print mode fires hooks even though it skips memory.** Complementing #2: `aello run <bp> -p "..."` does load the persona and does run `Stop`/`SessionEnd` hooks, so it's a valid way to place an env and exercise hook behaviour headlessly. Only persistent *memory* injection is missing.

7. **Claude Code's cwd→projects-dir encoding folds EVERY non-alphanumeric char to `-`, not just separators.** Verified empirically 2026-07-08: a real folder `…\work\human_behavior` is stored by Claude under `~/.claude/projects/` as `…-work-human-behavior` (underscore → hyphen; case preserved). So the rule is `replace([^A-Za-z0-9], '-')` — spaces and `_` fold too, not only `\ / : .`. aello's `sessions::encode_project_path` previously only folded `\ / : .`, so for any project path containing `_`/space it pointed seeded memory + `--resume` at a dir Claude never reads (silent no-op). Fixed on branch `audit-fixes-2026-07-08`. This is the ground truth behind the path-identity requirement in gotcha #2. See [[aello-architecture-decisions]].
