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
│   ├── hooks/session-start.py #   announces the env; reads + deletes HANDOFF/NOTE
│   ├── hooks/speak.py        #   the voice — + duck, focus, notify, win_audio.ps1
│   ├── skills/sync/SKILL.md  #   generated from this blueprint's role
│   ├── skills/handoff/SKILL.md       # universal — resume note to self
│   ├── skills/note/SKILL.md          # universal — note to another env
│   ├── skills/twosentences/SKILL.md  # universal — two-sentence summary
│   └── projects/<cwd>/memory/  # starter working-style memory, seeded once
├── .claude-env-reviewer/     # a second blueprint, fully isolated
├── claude-internal/          # TRACKED mirror of the env, namespaced per blueprint
│   └── <name>/               #   one folder per blueprint sharing the repo
│       ├── skills/           #     mirror of <env>/skills/ (one-way, pruned)
│       ├── memory/           #     union with <env>/projects/<cwd>/memory/
│       ├── persona.CLAUDE.md #     snapshot of <env>/CLAUDE.md (renamed; never auto-loads)
│       └── handoff.md        #     snapshot of <name>.HANDOFF.md, so it can cross machines
├── CLAUDE.md                 # project-level instructions (maintainer only)
├── README.md  CHANGELOG.md  docs/   # scaffolded by the role
└── .gitignore                # contains ".claude-env-*" (but NOT claude-internal/)
```

The env dir is gitignored, so the skills, memory, and persona that define a blueprint would never reach git. With the `github` cap, `claude-internal/` (a tracked folder at the repo root) captures that internal config — written *from* the env dir, so the live env stays the source of truth for anything generated. Each blueprint mirrors into its own `claude-internal/<name>/` namespace, so multiple blueprints sharing one repo don't clobber each other. It's seeded at placement and refreshed by `/sync`. The persona snapshot is renamed (`persona.CLAUDE.md`) so Claude Code never auto-loads it as a second persona.

**Skills are a strict one-way mirror; memory is a union.** A skill is regenerated from the role on every placement, so a mirrored skill the env no longer seeds is stale output and gets pruned. A memory note is not: on a repo worked from two machines, a note in the mirror with no counterpart in this env dir is the *other machine's*, and pruning it deleted committed work on a launch that had no way to know better. So memory only ever gains files, and a launch that finds mirror-only notes names them and points at `aello restore`. Deleting a note for real takes a `git rm` of the mirror copy — deliberate, rather than a side effect of starting a session.

Reading the mirror back happens in exactly two places:

- **On a clone**, where the mirror exists and the gitignored env dir does not, `aello run` restores the env from the mirror before it seeds anything. This is the reason the folder is tracked at all.
- **On demand, via `aello restore <name>`**, when the env dir already exists — the case `aello run` deliberately will not touch, because a live env must not be contradicted by a snapshot. This is what you run after pulling work another machine pushed. See [`workflows.md`](workflows.md) for the full two-machine loop.

**A project-level `<project>/.claude/settings.json` is silently ignored under aello.** Claude Code reads its settings from `CLAUDE_CONFIG_DIR`, which aello points at the env dir — so the per-project `.claude/` directory you'd use with a normal Claude Code install has no effect here, with no warning. Put hooks, permissions, and env vars in `<project>/.claude-env-<name>/settings.json` instead. This is per blueprint by design: two blueprints in one repo are meant to be able to disagree about their settings.

## Tracked source of truth vs derived artifacts

A recurring rule across aello's `github` cap: **one tracked source of truth, everything else derived one-way and kept out of git.** `claude-internal/`'s *generated* half — the skills and the persona snapshot — is derived from the env dir that way; memory is the documented exception above, because two machines write it and a derived-only rule there deletes one of them. The same discipline governs versioning. The scaffolded `VERSION` file is the single tracked home of a project's version — any other stamp (a README badge, `package.json`'s `version`, a generated `version.ts`) must be **derived from `VERSION` at build time and the derived file gitignored**, never written into a second tracked file.

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

- **Global / persona** — `<env>/CLAUDE.md`. The agent's identity ("you are a coding agent…"). Chosen with `--claude-md`: `coder` for a coding project, `none` for anything else, or a path to a file you maintain. Written once; never overwritten on later runs.

  A persona has a lifecycle. A project starts on one of the two defaults, and once it has grown enough to have real working agreements you can replace that cookie cutter with one written for it — `aello persona <name> --from <file>`. That writes the file, flips the blueprint to **`custom`** so aello stops seeding a template, and records the generation in `<env>/persona.gen` (`gen1 2026-08-03`). So `aello list` tells you which envs have a real persona, and the sidecar tells you which generation each is on. The generation is per env rather than per blueprint, because one blueprint placed in two projects has two personas.
- **Project** — `<project>/CLAUDE.md`. Project-specific facts and instructions, owned by the `maintainer` role. Maintained over time by `/sync`.

Memory is a third, separate channel — nothing to do with the role. On first placement aello seeds a starter working-style memory under `<env>/projects/<encoded-cwd>/memory/` (a `working-style.md` note plus a one-line `MEMORY.md` index), so a fresh env boots with it already in `/context`. It's seeded only when no `MEMORY.md` exists yet, so a re-place never clobbers memory you've accumulated. Thereafter memory is maintained automatically (the PostCompact hook writes transcript summaries).

## Authentication

`aello login` runs `claude setup-token` (a browser/OAuth flow), captures the long-lived `sk-ant-oat…` token, and stores it in `config.toml`. Every `aello run` exports it as `CLAUDE_CODE_OAUTH_TOKEN`. Because this token does **not** rotate, any number of blueprints can run concurrently against it — unlike copying `.credentials.json`, whose rotating refresh tokens invalidate each other across parallel envs.

**An env's auth is aello's to choose, and only aello's.** Every launch strips `CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` from the child's environment before setting whatever the config says. Agents run `aello` from inside an aello env, so inheriting one of those was the normal case rather than an exotic one — and with no token configured, aello printed "Claude will prompt login" while the env quietly authenticated as whoever owned the ambient variable. If you *want* an env on an API key, put it in that env's `settings.json`, not in the shell you launch from.

On a fresh env, aello also marks onboarding complete (`hasCompletedOnboarding` in `.claude.json`) so Claude skips its first-run wizard and goes straight in.

## contextdb (transcripts)

aello seeds three session hooks. **PostCompact** saves each compaction summary; **SessionEnd** captures a session that ends without compacting — `/clear` or a plain exit. The SessionEnd record archives the `/handoff` note (`<blueprint>.HANDOFF.md`), **a copy of the full transcript**, and the transcript's original path; it skips subagent sessions so the tree isn't flooded. The archives land in a unified tree:

```
<contextdb>/<project>/<blueprint>/<timestamp>_<session>.jsonl       # PostCompact
<contextdb>/<project>/<blueprint>/<timestamp>_<session>_end.jsonl   # SessionEnd
<contextdb>/<project>/<blueprint>/<timestamp>_<session>_transcript.jsonl  # its transcript
```

The root is per-machine, defaults to `~/aello/contextdb`, and is configurable from the TUI (`C`). aello passes it to Claude as `AELLO_CONTEXTDB`; if unset, the hooks fall back to a local folder inside the env.

**In practice SessionEnd does all of it.** PostCompact fires only when a session compacts, and a 1M-context session that you end with `/clear` never gets close. Audited on 2026-08-03: contextdb held **265 SessionEnd records and zero PostCompact records** for any aello blueprint — the newest compaction capture was seven weeks old and from a pre-aello setup. PostCompact isn't broken; it's dormant, and it stays seeded for the workflows that do compact.

**Why the transcript is copied and not just referenced.** It used to be a path only. Claude Code deletes its own session files after `cleanupPeriodDays` — **default 30** — and the env dir holding them is gitignored and removed outright by `aello remove --purge`. So the reference silently stopped resolving: in that same audit, 15% of archives already pointed at nothing, with a clean cliff at the 30-day mark (6–14% dead under 30 days, 44% at 30–39). Nothing errored; the archive just quietly stopped being one. Placement now also sets `cleanupPeriodDays` to 365 — filled in only when absent, so a value you chose is left alone — which keeps `--resume` working on old sessions too, something a copy can't do.

**Reasoning is captured too, as summaries.** Every env launches with `--thinking-display summarized`. The API's default is `omitted`, which sends thinking blocks whose text is an empty string — so transcripts recorded a complete account of what was *done* and nothing of what was *thought* (measured 2026-08-03: 2,842 thinking blocks across 53 transcripts, every one empty). `display` only controls visibility — thinking happens and is billed identically either way — so this costs nothing. The raw chain of thought is never returned on any model; a summary is the most that exists. Override for a single run with `aello run <name> -- --thinking-display omitted`.

The record's `transcript_archived` field names the copy beside it, or is empty if the copy failed — so you can tell "archived here" from "only ever a pointer" without checking the filesystem. Transcripts are large (median 1.3 MB, but tens of MB at the tail), so expect contextdb to grow accordingly.

**The archive has a second reader.** `aello tokens` counts usage straight out of these files — no extra hook, and retroactively over everything already recorded. It is also the strongest argument for copying the transcript rather than referencing it: a pointer that has expired is not just a lost transcript, it is a session whose cost can never be accounted for. One trap if you ever parse them yourself: Claude Code writes **one record per content block** and repeats the message's whole `usage` on each, so counting records instead of distinct `message.id`s roughly doubles every figure. See [tokens.md](tokens.md).

**SessionStart** is the third, and it reads rather than writes. On boot it delivers `<blueprint>.HANDOFF.md` (your `/handoff` note to yourself) and `<blueprint>.NOTE.md` (a `/note` left by another env sharing the repo) into the new session, then **deletes** them. That is what makes those skills' promise true — before it existed nothing consumed either file, so they sat at the project root dirtying `git status` and SessionEnd re-archived the same stale note every session. Deleting is safe because SessionEnd has already archived the content.

It also opens **every** session with a short standing block saying the session is running under aello, which blueprint it is, that the env dir is rewritten on every run, and that the seeded skills are yours to type rather than the agent's to run. Nothing else tells a session any of that: the env dir is gitignored, the persona belongs to you and usually doesn't mention aello, and a project `CLAUDE.md` only exists for a maintainer. Without it, agents edited files in `.claude-env-*` that the next launch quietly overwrote.

**UserPromptSubmit** is the fourth, and unlike the others it runs on every *prompt* rather than every session. It carries the four response rules every env gets — be concise, don't be sycophantic, never hand over a plan, and close with one block: the `TL;DR:` line the voice speaks, with 3–4 stand-alone numbered next steps beneath it when anything is left for you to do — for about 300 tokens a turn. Per turn because style decays: an instruction delivered once at session start is buried eighty turns later, which is when padding and reflexive agreement come back. On a hook rather than in the persona because the persona is written once and never clobbered, so editing the template would reach no existing env, while `place` rewrites hook scripts on every run. See [voice.md](voice.md) for the wording and how to change it.

**PreToolUse** is the fifth, and the only one that blocks rather than records. It matches `EnterPlanMode|ExitPlanMode` and denies both, so plan mode is unavailable in every env — the enforcing half of the no-plans rule the prompt hook asks for. It runs only for those two tool names; an unmatched group would spawn Python on every tool call.
