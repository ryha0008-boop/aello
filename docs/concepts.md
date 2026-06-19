# Concepts

## The isolation model

A single Claude Code install reads its config from `CLAUDE_CONFIG_DIR`. aello exploits this: each blueprint, when run in a project, gets its own directory at `<project>/.claude-env-<name>/` and Claude is launched with `CLAUDE_CONFIG_DIR` pointed there. Two blueprints in the same repo are fully isolated — separate settings, persona, hooks, skills, and session history — while sharing the project files they're working on.

```
my-project/
├── .git/
├── .claude-env-coder/        # CLAUDE_CONFIG_DIR for the "coder" blueprint
│   ├── settings.json         #   bypass-permissions + PostCompact hook
│   ├── CLAUDE.md             #   global persona (set once)
│   ├── hooks/post-compact.py
│   └── skills/sync/SKILL.md  #   generated from this blueprint's capabilities
├── .claude-env-reviewer/     # a second blueprint, fully isolated
├── CLAUDE.md                 # project-level instructions (--project-md)
├── README.md  CHANGELOG.md  docs/   # scaffolded by capabilities
└── .gitignore                # contains ".claude-env-*"
```

## Blueprint vs instance

- A **blueprint** is global, stored in aello's `config.toml`: `name`, `model`, optional persona (`claude_md`), and `capabilities`. It's reusable across any number of projects.
- An **instance** is a blueprint placed into a project — recorded as `.aello.toml` inside the env dir. Placement is idempotent: `aello run` re-seeds the generated skill and refreshes the hook each time, but never clobbers your edited persona or scaffolded files.

## Two CLAUDE.md layers

- **Global / persona** — `<env>/CLAUDE.md`. The agent's identity ("you are a coding agent…"). Chosen with `--claude-md` (a built-in `coder`/`sysadmin` template, or a path). Written once; never overwritten on later runs.
- **Project** — `<project>/CLAUDE.md`. Project-specific facts and instructions, enabled with `--project-md`. Maintained over time by `/sync`.

Memory is a third, separate channel — automatic, written by the PostCompact hook; not a capability.

## Authentication

`aello login` runs `claude setup-token` (a browser/OAuth flow), captures the long-lived `sk-ant-oat…` token, and stores it in `config.toml`. Every `aello run` exports it as `CLAUDE_CODE_OAUTH_TOKEN`. Because this token does **not** rotate, any number of blueprints can run concurrently against it — unlike copying `.credentials.json`, whose rotating refresh tokens invalidate each other across parallel envs.

On a fresh env, aello also marks onboarding complete (`hasCompletedOnboarding` in `.claude.json`) so Claude skips its first-run wizard and goes straight in.

## contextdb (transcripts)

The only hook aello seeds is **PostCompact**, a Python script that saves each compaction summary. Transcripts land in a unified tree:

```
<contextdb>/<project>/<blueprint>/<timestamp>_<session>.jsonl
```

The root is per-machine, defaults to `~/aello/contextdb`, and is configurable from the TUI (`C`). aello passes it to Claude as `AELLO_CONTEXTDB`; if unset, the hook falls back to a local folder inside the env.
