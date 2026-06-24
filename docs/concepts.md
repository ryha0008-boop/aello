# Concepts

## The isolation model

A single Claude Code install reads its config from `CLAUDE_CONFIG_DIR`. aello exploits this: each blueprint, when run in a project, gets its own directory at `<project>/.claude-env-<name>/` and Claude is launched with `CLAUDE_CONFIG_DIR` pointed there. Two blueprints in the same repo are fully isolated — separate settings, persona, hooks, skills, and session history — while sharing the project files they're working on.

```
my-project/
├── .git/
├── .claude-env-coder/        # CLAUDE_CONFIG_DIR for the "coder" blueprint
│   ├── settings.json         #   bypass-permissions + PostCompact/SessionEnd hooks
│   ├── CLAUDE.md             #   global persona (set once)
│   ├── hooks/post-compact.py
│   ├── hooks/session-end.py
│   ├── skills/sync/SKILL.md  #   generated from this blueprint's capabilities
│   ├── skills/handoff/SKILL.md  # universal — seeded for every blueprint
│   └── projects/<cwd>/memory/  # starter working-style memory, seeded once
├── .claude-env-reviewer/     # a second blueprint, fully isolated
├── claude-internal/          # TRACKED one-way mirror, namespaced per blueprint
│   └── <name>/               #   one folder per blueprint sharing the repo
│       ├── skills/           #     mirror of <env>/skills/
│       ├── memory/           #     mirror of <env>/projects/<cwd>/memory/
│       └── persona.CLAUDE.md #     snapshot of <env>/CLAUDE.md (renamed; never auto-loads)
├── CLAUDE.md                 # project-level instructions (--project-md)
├── README.md  CHANGELOG.md  docs/   # scaffolded by capabilities
└── .gitignore                # contains ".claude-env-*" (but NOT claude-internal/)
```

The env dir is gitignored, so the skills, memory, and persona that define a blueprint would never reach git. With the `github` cap, `claude-internal/` (a tracked folder at the repo root) is a **one-way mirror** of that internal config — written *from* the env dir, never back into it, so the live env stays the single source of truth. Each blueprint mirrors into its own `claude-internal/<name>/` namespace, so multiple blueprints sharing one repo don't clobber each other. It's seeded at placement and refreshed by `/sync`. The persona snapshot is renamed (`persona.CLAUDE.md`) so Claude Code never auto-loads it as a second persona.

## Tracked source of truth vs derived artifacts

A recurring rule across aello's `github` cap: **one tracked source of truth, everything else derived one-way and kept out of git.** `claude-internal/` is derived from the env dir; the same discipline governs versioning. The scaffolded `VERSION` file is the single tracked home of a project's version — any other stamp (a README badge, `package.json`'s `version`, a generated `version.ts`) must be **derived from `VERSION` at build time and the derived file gitignored**, never written into a second tracked file.

This isn't optional polish: the `github` cap's CI auto-bumps `VERSION` on every push, and the generated `/sync` stages only files the agent touched this session (never `git add -A`). A version duplicated into a tracked artifact therefore drifts on every CI bump and can never be reconciled by `/sync` — it strands dirty. Deriving + gitignoring the artifact is the structural fix (softening `/sync`'s staging rule is not). See `docs/capabilities.md` for the full rationale and the `env-console` precedent.

## Blueprint vs instance

- A **blueprint** is global, stored in aello's `config.toml`: `name`, `model`, optional persona (`claude_md`), and `capabilities`. It's reusable across any number of projects.
- An **instance** is a blueprint placed into a project — recorded as `.aello.toml` inside the env dir. Placement is idempotent: `aello run` re-seeds the generated skill and refreshes the hook each time, but never clobbers your edited persona, scaffolded files, or memory.

## Two CLAUDE.md layers

- **Global / persona** — `<env>/CLAUDE.md`. The agent's identity ("you are a coding agent…"). Chosen with `--claude-md` (a built-in `coder`/`sysadmin` template, or a path). Written once; never overwritten on later runs.
- **Project** — `<project>/CLAUDE.md`. Project-specific facts and instructions, enabled with `--project-md`. Maintained over time by `/sync`.

Memory is a third, separate channel — not a capability. On first placement aello seeds a starter working-style memory under `<env>/projects/<encoded-cwd>/memory/` (a `working-style.md` note plus a one-line `MEMORY.md` index), so a fresh env boots with it already in `/context`. It's seeded only when no `MEMORY.md` exists yet, so a re-place never clobbers memory you've accumulated. Thereafter memory is maintained automatically (the PostCompact hook writes transcript summaries).

## Authentication

`aello login` runs `claude setup-token` (a browser/OAuth flow), captures the long-lived `sk-ant-oat…` token, and stores it in `config.toml`. Every `aello run` exports it as `CLAUDE_CODE_OAUTH_TOKEN`. Because this token does **not** rotate, any number of blueprints can run concurrently against it — unlike copying `.credentials.json`, whose rotating refresh tokens invalidate each other across parallel envs.

On a fresh env, aello also marks onboarding complete (`hasCompletedOnboarding` in `.claude.json`) so Claude skips its first-run wizard and goes straight in.

## contextdb (transcripts)

aello seeds two transcript hooks. **PostCompact** saves each compaction summary; **SessionEnd** captures a session that ends without compacting — `/clear` or a plain exit — which PostCompact would otherwise miss entirely (a `/clear`-heavy workflow never compacts). The SessionEnd record archives the `/handoff` note (`HANDOFF.md`, otherwise deleted on next boot) plus a pointer to the full transcript; it skips subagent sessions so the tree isn't flooded. Both land in a unified tree:

```
<contextdb>/<project>/<blueprint>/<timestamp>_<session>.jsonl       # PostCompact
<contextdb>/<project>/<blueprint>/<timestamp>_<session>_end.jsonl   # SessionEnd
```

The root is per-machine, defaults to `~/aello/contextdb`, and is configurable from the TUI (`C`). aello passes it to Claude as `AELLO_CONTEXTDB`; if unset, the hooks fall back to a local folder inside the env.
