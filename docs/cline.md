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
aello login --agent cline    # prompts for provider, model, key, base URL
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

**The persona is a rules file.** `--claude-md coder` works on a Cline blueprint — the bundled templates are just text, and aello writes the chosen one to `config/rules/persona.md` rather than to a `CLAUDE.md` Cline would ignore. Choosing `none` **removes** an existing one, which matters more here than for Claude: rules apply on every request, so a persona left behind after you switched would keep applying with nothing to take it away.

## The four commands, and why they are routed rather than registered

**Cline has no user-defined slash commands.** Measured: `/canary`, with a workflow *and* a skill installed in every candidate location (`<workspace>/.cline/workflows/`, `<workspace>/.cline/skills/`, `<config>/workflows/`, `<config>/skills/`), came back as ordinary prose. The only slash commands the binary advertises are connector built-ins — `/abort`, `/clear`, `/exit`, `/start`, `/whereami`. Cline's own skills and workflows resolve under `<workspace>/.cline/<plugin>/` as plugin artifacts, not as something a user types.

So `config/rules/aello.md` routes them instead: it lists each command against the absolute path of its `SKILL.md`, and the agent opens the file with an ordinary tool call. `/sync`, `/handoff`, `/note <name>` and `/twosentences` all work this way, and each skill carries the same "only the user runs this" banner a Claude env's does.

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

`.cline-env-*` is added to the project's `.gitignore` at placement, **unconditionally** — not gated on the blueprint's role the way the `.claude-env-*` line is.

That asymmetry is deliberate. A Claude env holds no secret: auth arrives as an environment variable at launch. A Cline env holds the API key in plaintext at `data/settings/providers.json`, so an unignored one is a credential a single `git add -A` away from a public repo. A standalone blueprint's key leaks exactly as well as a maintainer's.

## Resuming

Cline resumes by session id only — there is no "continue the most recent". `aello run <name> --resume` with no id is refused rather than silently starting a fresh session; pass an id:

```sh
aello run Runner --resume 1786038854858_rwh2q
```

Session state lives in `<env>/data/sessions/` and `<env>/data/db/sessions.db`.

## Known limits

- **The `claude-code` provider cannot use tools.** Cline can authenticate against a Claude subscription and will hold a conversation, but every tool call is rejected before execution — *"The Claude Code CLI executes its own tools; AI SDK tools cannot be auto-bridged at the provider layer."* Measured against `--auto-approve true`, a `CLAUDE_CONFIG_DIR` with `permissions.defaultMode = bypassPermissions`, and a seeded `.claude.json`; none of the three helped. So a Cline env that edits files needs a metered provider. This is upstream, not configuration — re-test it after a Cline upgrade.
- **Cline refuses a one-word prompt.** `aello run x -p "hi"` comes back with *"Unknown command or unquoted prompt"* — Cline reads a single word as a possible subcommand. It is not a quoting problem on your side, and aello now says so before launching. Use more than one word.
- **`/sync` and the memory rule are instructions, not enforcement.** Nothing in Cline blocks an agent that ignores them, where Claude Code's equivalents ride hooks aello controls.
- `aello edit` does not change an agent, by design.
