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
│   ├── hooks/session-start.py #   reads + deletes <name>.HANDOFF.md / .NOTE.md
│   ├── hooks/speak.py        #   the voice — + duck, focus, notify, win_audio.ps1
│   ├── skills/sync/SKILL.md  #   generated from this blueprint's role
│   ├── skills/handoff/SKILL.md       # universal — resume note to self
│   ├── skills/note/SKILL.md          # universal — note to another env
│   ├── skills/twosentences/SKILL.md  # universal — two-sentence summary
│   └── projects/<cwd>/memory/  # starter working-style memory, seeded once
├── .claude-env-reviewer/     # a second blueprint, fully isolated
├── claude-internal/          # TRACKED one-way mirror, namespaced per blueprint
│   └── <name>/               #   one folder per blueprint sharing the repo
│       ├── skills/           #     mirror of <env>/skills/
│       ├── memory/           #     mirror of <env>/projects/<cwd>/memory/
│       └── persona.CLAUDE.md #     snapshot of <env>/CLAUDE.md (renamed; never auto-loads)
├── CLAUDE.md                 # project-level instructions (--project-md)
├── README.md  CHANGELOG.md  docs/   # scaffolded by the role
└── .gitignore                # contains ".claude-env-*" (but NOT claude-internal/)
```

The env dir is gitignored, so the skills, memory, and persona that define a blueprint would never reach git. With the `github` cap, `claude-internal/` (a tracked folder at the repo root) is a **one-way mirror** of that internal config — written *from* the env dir, never back into it, so the live env stays the single source of truth. Each blueprint mirrors into its own `claude-internal/<name>/` namespace, so multiple blueprints sharing one repo don't clobber each other. It's seeded at placement and refreshed by `/sync`. The persona snapshot is renamed (`persona.CLAUDE.md`) so Claude Code never auto-loads it as a second persona.

**A project-level `<project>/.claude/settings.json` is silently ignored under aello.** Claude Code reads its settings from `CLAUDE_CONFIG_DIR`, which aello points at the env dir — so the per-project `.claude/` directory you'd use with a normal Claude Code install has no effect here, with no warning. Put hooks, permissions, and env vars in `<project>/.claude-env-<name>/settings.json` instead. This is per blueprint by design: two blueprints in one repo are meant to be able to disagree about their settings.

## Tracked source of truth vs derived artifacts

A recurring rule across aello's `github` cap: **one tracked source of truth, everything else derived one-way and kept out of git.** `claude-internal/` is derived from the env dir; the same discipline governs versioning. The scaffolded `VERSION` file is the single tracked home of a project's version — any other stamp (a README badge, `package.json`'s `version`, a generated `version.ts`) must be **derived from `VERSION` at build time and the derived file gitignored**, never written into a second tracked file.

This isn't optional polish: the scaffolded CI auto-bumps `VERSION` on every push, and the generated `/sync` stages only files the agent touched this session (never `git add -A`). A version duplicated into a tracked artifact therefore drifts on every CI bump and can never be reconciled by `/sync` — it strands dirty. Deriving + gitignoring the artifact is the structural fix (softening `/sync`'s staging rule is not). See `docs/roles.md` for the full rationale and the `env-console` precedent.

## Blueprint vs instance

- A **blueprint** is global, stored in aello's `config.toml`: `name`, `model`, optional persona (`claude_md`), and a `role`. It's reusable across any number of projects.
- An **instance** is a blueprint placed into a project — recorded as `.aello.toml` inside the env dir. Placement is idempotent: `aello run` re-seeds the generated skill and refreshes the hook each time, but never clobbers your edited persona, scaffolded files, memory, or a skill you've marked kept.

## The seeded skills are yours to run

`/sync`, `/handoff`, `/note` and `/twosentences` only run when **you** type them. Each carries `disable-model-invocation: true`, which stops the agent invoking one as a tool, and a banner saying that reading the file and carrying out its steps is the same as running it. Both are needed: the flag alone left an agent free to open the `SKILL.md` and work through it unasked, which is worse than doing nothing — you end up believing a checkpoint happened.

If an agent tells you it "ran `/sync`" without you typing it, it didn't. Ask it to say so plainly and then invoke the command yourself.

## Keeping a hand-edited skill

The four seeded skills — `/sync`, `/handoff`, `/note`, `/twosentences` — are **rewritten on every `aello run`**. That's what makes a role change reach an env you placed months ago, and it's the right default. But it also means editing one in place is temporary: the next run silently restores the generated version, and if the role then mirrors the env into `claude-internal/`, the generated version is committed over your custom one. (Your edit survives in git history, not in the working tree.)

To pin a skill you've rewritten for one project, drop an empty `.aello-keep` file beside it:

```sh
touch .claude-env-<name>/skills/sync/.aello-keep
```

`place` then leaves that skill entirely alone — not regenerated, and not removed if the blueprint later becomes standalone. Everything else in the env keeps healing normally, and other blueprints are unaffected. The marker is per env dir, which is per project: the same blueprint used elsewhere still gets the generated skill.

Use it when a project genuinely needs a different workflow (a `/sync` that also deploys, say). Because a kept skill no longer tracks its role, changing the role for that blueprint won't be reflected in it — update it by hand, or delete the marker to fall back to the generated version.

## Two CLAUDE.md layers

- **Global / persona** — `<env>/CLAUDE.md`. The agent's identity ("you are a coding agent…"). Chosen with `--claude-md` (a built-in `coder`/`sysadmin` template, or a path). Written once; never overwritten on later runs.
- **Project** — `<project>/CLAUDE.md`. Project-specific facts and instructions, enabled with `--project-md`. Maintained over time by `/sync`.

Memory is a third, separate channel — nothing to do with the role. On first placement aello seeds a starter working-style memory under `<env>/projects/<encoded-cwd>/memory/` (a `working-style.md` note plus a one-line `MEMORY.md` index), so a fresh env boots with it already in `/context`. It's seeded only when no `MEMORY.md` exists yet, so a re-place never clobbers memory you've accumulated. Thereafter memory is maintained automatically (the PostCompact hook writes transcript summaries).

## Authentication

`aello login` runs `claude setup-token` (a browser/OAuth flow), captures the long-lived `sk-ant-oat…` token, and stores it in `config.toml`. Every `aello run` exports it as `CLAUDE_CODE_OAUTH_TOKEN`. Because this token does **not** rotate, any number of blueprints can run concurrently against it — unlike copying `.credentials.json`, whose rotating refresh tokens invalidate each other across parallel envs.

On a fresh env, aello also marks onboarding complete (`hasCompletedOnboarding` in `.claude.json`) so Claude skips its first-run wizard and goes straight in.

## contextdb (transcripts)

aello seeds three session hooks. **PostCompact** saves each compaction summary; **SessionEnd** captures a session that ends without compacting — `/clear` or a plain exit — which PostCompact would otherwise miss entirely (a `/clear`-heavy workflow never compacts). The SessionEnd record archives the `/handoff` note (`<blueprint>.HANDOFF.md`) plus a pointer to the full transcript; it skips subagent sessions so the tree isn't flooded. The two archives land in a unified tree:

```
<contextdb>/<project>/<blueprint>/<timestamp>_<session>.jsonl       # PostCompact
<contextdb>/<project>/<blueprint>/<timestamp>_<session>_end.jsonl   # SessionEnd
```

The root is per-machine, defaults to `~/aello/contextdb`, and is configurable from the TUI (`C`). aello passes it to Claude as `AELLO_CONTEXTDB`; if unset, the hooks fall back to a local folder inside the env.

**SessionStart** is the third, and it reads rather than writes. On boot it delivers `<blueprint>.HANDOFF.md` (your `/handoff` note to yourself) and `<blueprint>.NOTE.md` (a `/note` left by another env sharing the repo) into the new session, then **deletes** them. That is what makes those skills' promise true — before it existed nothing consumed either file, so they sat at the project root dirtying `git status` and SessionEnd re-archived the same stale note every session. Deleting is safe because SessionEnd has already archived the content.
