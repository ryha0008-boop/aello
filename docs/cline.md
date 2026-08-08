# Cline envs

aello drives two CLIs. A blueprint picks one at `add` time and keeps it:

```sh
aello add Researcher --model opus --agent claude    # the default
aello add Runner --model openai/gpt-5.6-luna-pro --agent cline
```

The choice is fixed. The two agents share nothing on disk, so switching one would strand its env rather than convert it.

## The split is total, on purpose

| | Claude Code | Cline |
|---|---|---|
| env dir | `.claude-env-<name>` | `.cline-env-<name>` |
| isolation | `CLAUDE_CONFIG_DIR` env var | `--config` / `--data-dir` flags |
| persona | `<env>/CLAUDE.md` | `<env>/config/rules/*.md` — `CLAUDE.md` is **ignored** |
| auth | one shared subscription token | a provider key, billed per token |
| voice | yes | no |
| per-turn rules | `UserPromptSubmit` hook | a rules file |
| contextdb | three hooks | none |
| memory | automatic (hook) | a directory + a rule the agent follows |
| the four commands | real slash commands | routed from the rules file |

Everything Cline-specific lives in `cline.rs`, and nothing Cline-specific lives anywhere else. `project.rs` and `launch.rs` stay Claude-only. The two CLIs agree on almost nothing, so a shared code path would carry an `if agent == …` at every branch — and the branch that got forgotten would be the one that silently wrote a Claude file into a Cline env, where nothing would ever read it.

## Logging in

`aello login` covers two different accounts and asks which one you mean:

```sh
aello login                  # asks
aello login --agent claude   # runs `claude setup-token`
aello login --agent cline    # prompts for provider, model, key (not echoed), base URL
```

They are stored separately (`oauth_token` and `[cline]` in `config.toml`) and one is never inferred from the other. Setting a Cline login does not touch the Claude token and vice versa.

One Cline credential is shared by every Cline env, exactly as one Claude token is shared by every Claude env. It is installed into each env on **every run**, not cached behind a marker — the key can change in `config.toml`, and a marker recording "this env is authenticated" would go stale exactly then.

⚠️ **A Cline env is metered.** Every turn costs money per token at your provider. A Claude env costs nothing beyond the subscription. That difference is the main reason the two logins are kept apart.

## The credential is installed by `cline auth`, never by writing the file

aello wrote `providers.json` itself for about an afternoon. The file is small and its shape is obvious from a real one, so hand-writing it looked correct — and it *half*-worked, which is the dangerous part. The env placed, the run launched, the provider was reached, and the error came back reading like a bad key.

What had actually happened: **Cline rewrote `providers.json` on its next run and dropped the `apiKey` field entirely**, leaving `provider`, `model` and `tokenSource` behind. The request went out with no credential at all. Measured by diffing the file either side of a run; a key installed by `cline auth` survives that same run untouched. The difference is the writer, not the value.

So placement needs `cline` on `PATH`. That is not a real cost — nothing can run a Cline env without it.

## What a Cline env contains

```
.cline-env-<name>/
  config/rules/     response-rules.md, persona.md, aello.md, memory.md   ← re-sent every request
  skills/           sync/ handoff/ note/ twosentences/ SKILL.md
  memory/           MEMORY.md + one file per thing learned
  data/             providers.json (the key), sessions, logs   ← Cline's own
```

Everything in `config/rules/` is re-sent in the system prompt on **every** request, which is the property that makes it a workable substitute for Claude Code's per-turn hook: it cannot decay by turn eighty.

**The persona is a rules file.** `--claude-md coder` works on a Cline blueprint — the bundled templates are just text, and aello writes the chosen one to `config/rules/persona.md` rather than to a `CLAUDE.md` Cline would ignore. Choosing `none` **removes** an existing one, which matters more here than for Claude: rules apply on every request, so a persona left behind after you switched would keep applying with nothing to take it away. `custom` is the opposite and is left strictly alone — it means the env's own `persona.md` is authoritative. (Both resolve to "aello writes no text", and placement treated them the same until it was pointed out that this deleted a `custom` persona on every run.)

## The four commands, and why they are routed rather than registered

**Cline has no user-defined slash commands.** Measured: `/canary`, with a workflow *and* a skill installed in every candidate location (`<workspace>/.cline/workflows/`, `<workspace>/.cline/skills/`, `<config>/workflows/`, `<config>/skills/`), came back as ordinary prose. The only slash commands the binary advertises are connector built-ins — `/abort`, `/clear`, `/exit`, `/start`, `/whereami`. Cline's own skills and workflows resolve under `<workspace>/.cline/<plugin>/` as plugin artifacts, not as something a user types.

So `config/rules/aello.md` routes them instead: it lists each command against the absolute path of its `SKILL.md`, and the agent opens the file with an ordinary tool call. `/sync`, `/handoff`, `/note <name>` and `/twosentences` all work this way, and each skill carries the same "only the user runs this" banner a Claude env's does.

**Verified end to end** against a real provider on 2026-08-06: `/sync now` opened the memory index, looked for `AGENTS.md` and correctly reported nothing to change; `/handoff please` wrote a properly name-prefixed `<name>.HANDOFF.md` at the project root.

The launch path itself was re-measured on 2026-08-08 against the installed binary, using a deliberately invalid key: `aello run` placed the env, `cline auth` installed the key into `<env>/data/settings/providers.json`, and the request reached the provider, which rejected it. Everything before the provider is therefore exercised even without a working credential — which is also the cheapest way to check that a new machine is wired up correctly.

⚠️ **In one-shot mode (`-p`) a command needs a trailing word** — `-p "/sync now"`, not `-p "/sync"`. Cline refuses *any* one-word prompt with "Unknown command or unquoted prompt", the four commands included. That is why the router matches a **prefix** rather than an exact message, and aello refuses a one-word prompt itself with the real reason rather than letting Cline's message mislead you. The interactive TUI has a genuine `/` menu and is not affected.

An early test appeared to show `-p "/twosentences"` working on its own. It had in fact been rewritten by Git Bash into `C:/Program Files/Git/twosentences` — which contains a space, so Cline accepted it and the model *guessed* the skill from the path. Worth knowing if you test from a POSIX shell on Windows: set `MSYS_NO_PATHCONV=1`, or a leading-slash argument is silently turned into a path.

### Could they be real slash commands?

Only by shipping a **JavaScript plugin**. The TUI's `/` menu is built from five built-ins plus `listRuntimeCommands()`, which comes from installed plugins — and `cline plugin install` rejects a folder of markdown with "No plugin entry files found": a plugin is an npm-style package with a JS entry point. It also installs to `<project>/.cline/plugins`, i.e. **workspace-scoped**, so one plugin would be shared by every env in a repo rather than isolated per blueprint. That is a node dependency and a hole in the isolation model, for cosmetics — hence the router.

⚠️ `/sync` **has no git step** for a Cline env. It reconciles two things — the memory directory, then the project's `AGENTS.md` (Cline's equivalent of a project `CLAUDE.md`; it does not read `CLAUDE.md`) — and stops. Committing stays yours. If you want Cline blueprints committing too, that is a git section added to the `sync` body in `cline.rs`.

## Memory is aello's, because Cline has none

Confirmed against the binary: `memory-bank`, `memoryBank`, `cline_docs`, `MEMORY.md` and `rememberThis` are all absent, and the one `auto-memory` hit belongs to the Claude Code settings schema Cline embeds as a dependency.

So aello builds it: a `memory/` directory with a `MEMORY.md` index, seeded once and never rewritten, plus a rule telling the agent to read the index when it starts and to write durable findings as files. There is no hook that could load it automatically — `TaskStart` fires but cannot inject — so the instruction has to live in the rules, where it is re-sent every request. The agent does the reading and writing with ordinary tool calls.

The trade against Claude Code's memory is honest: Claude's is maintained by a hook whether or not the agent cooperates, and this one is an instruction the agent follows. It will not be as reliable.

## Why a Cline env is quieter

Not an omission, and not laziness. Cline's hook system does not carry what aello's features need, which was measured rather than assumed: marker hooks for `TaskStart`, `TaskComplete`, `SessionShutdown`, `UserPromptSubmit` and `PostToolUse` were placed in **both** `<config>/hooks/` and a `--hooks-dir` path, in one run that made a tool call.

- Exactly one fired: `<config>/hooks/TaskStart.py`.
- **`--hooks-dir` fired nothing at all**, despite `--help` describing it as additional hooks.
- The payload is identifiers only — `taskId`, `agent_id`, `workspaceInfo`, `hookName` — with **no prompt, no response and no transcript path**.

So there is no end-of-response event to speak from, and no per-turn event to inject into. The voice and contextdb have no mechanism at all. The four response rules do survive, as `<env>/config/rules/response-rules.md`: Cline re-sends its rules in the system prompt on every request, which arrives at the same non-decaying property the per-turn hook gives a Claude env. That `<config>/rules/` is the directory actually read was measured with a canary rule against three candidate locations — it won over `<data>/rules/` and the project's own `AGENTS.md`.

## The env dir is always gitignored

`.cline-env-*` is added to the project's `.gitignore` at placement, **unconditionally**. So is `.claude-env-*` now — the asymmetry this section used to describe is gone.

It was justified here on the grounds that "a Claude env holds no secret: auth arrives as an environment variable at launch". That premise is false whenever no shared token is configured: Claude Code then writes its own `.credentials.json` inside the env dir, and `standalone` — the default role — was the one role that never got the ignore line. Both lines are unconditional as of 2026-08-06.

The reason for Cline's line was never wrong, only narrower than it looked: a Cline env holds the API key in plaintext at `data/settings/providers.json`, so an unignored one is a credential a single `git add -A` away from a public repo. A standalone blueprint's key leaks exactly as well as a maintainer's — and `aello github-setup` now refuses to commit anything staged from inside either env dir, rather than trusting the ignore line alone.

## Resuming

Cline resumes by session id only — there is no "continue the most recent". `aello run <name> --resume` with no id is refused rather than silently starting a fresh session; pass an id:

```sh
aello run Runner --resume 1786038854858_rwh2q
```

Session state lives in `<env>/data/sessions/` and `<env>/data/db/sessions.db`.

## Known limits

- **The `claude-code` provider cannot use tools.** Cline can authenticate against a Claude subscription and will hold a conversation, but every tool call is rejected before execution — *"The Claude Code CLI executes its own tools; AI SDK tools cannot be auto-bridged at the provider layer."* Measured against `--auto-approve true`, a `CLAUDE_CONFIG_DIR` with `permissions.defaultMode = bypassPermissions`, and a seeded `.claude.json`; none of the three helped. So a Cline env that edits files needs a metered provider. This is upstream, not configuration — re-test it after a Cline upgrade.
- **Cline refuses any one-word prompt**, `/sync` included. Add a word: `-p "/sync now"`. aello says so before launching rather than letting Cline's message read like a quoting mistake.
- **`/sync` and the memory rule are instructions, not enforcement.** Nothing in Cline blocks an agent that ignores them, where Claude Code's equivalents ride hooks aello controls.
- `aello edit` does not change an agent, by design.
- **One file still lands in the shared tree.** An isolated run creates `~/.cline/cli-node-extra-ca-certs.pem` (a ~187 KB node CA bundle) whenever it is absent, whatever `--config` and `--data-dir` say. Measured 2026-08-08 by deleting it and re-running: it came back byte-identical, while the run's own sessions, database, logs and credential all stayed inside the env dir and the shared tree's session count did not move. No credentials or per-env state cross over, so the isolation that matters holds — but "nothing outside the env is touched" would be too strong a claim.
