---
name: cline
description: "Cline on this machine — it is the CLI not the VS Code extension, how its config/rules/hooks/providers map onto Claude Code's, what is genuinely absent (memory, ignore file), which of its mechanisms are measured working and which only look wired"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4742e7a2-20a8-4f4b-bb8d-1bae2f0fd8ea
  modified: 2026-08-06T19:16:01.729Z
---

Investigated 2026-08-06 for a possible "aello for Cline". Everything below was
measured on this machine unless marked otherwise — several confident answers
from reading the bundle turned out to be wrong, so prefer running the thing.

1. **It is the Cline *CLI*, not the VS Code extension.** `npm i -g cline`,
   currently **3.0.51**, shim at `%APPDATA%\npm\cline.cmd` → a ~141 MB
   `@cline/cli-windows-x64/bin/cline.exe`. There is **no** Cline extension in
   `~/.vscode/extensions` (11 extensions, none of them Cline). Anyone reasoning
   from "the Cline extension" is describing a different product surface. State
   dir `~/.cline`, sessions in `~/.cline/data/sessions/<id>/*.messages.json`,
   provider config in `~/.cline/data/settings/providers.json`. On this machine
   the provider is **openrouter / `openai/gpt-5.6-luna-pro`**, i.e. metered
   spend per token — every test prompt costs the user money.

2. **A running `cline.exe` breaks its own upgrade, and npm exits 0 anyway.**
   Found the install with the package present but **no binary**: the shim
   pointed at a `bin/cline.exe` that did not exist, so nothing launched.
   `npm i -g cline@latest` printed `npm warn cleanup Failed to remove … EPERM …
   unlink … cline.exe`, **exit code 0**, and only worked because the lock
   happened to release. Three `cline` processes were running at the time. Same
   shape as aello's own locked-exe problem ([[aello-dev-gotchas]] #1): kill the
   processes first, and check the exe exists afterwards rather than trusting
   the exit code.

3. **Isolation is CLI flags, not an environment variable — and grepping for env
   vars produced a confidently wrong answer.** I enumerated all 25 `CLINE_*`
   variables in the bundle, found no config-dir override, and told the user
   Cline could not be isolated. Wrong: `--config <dir>` (default `~/.cline`),
   `--data-dir <dir>` and `--hooks-dir <dir>` do exactly that, and `--config`
   was measured creating a completely fresh tree — own `providers.json`, own
   sessions db, own logs, no hub. **So an aello-for-Cline is buildable**:
   `cline --config <env>/config --data-dir <env>/data` per blueprint. Auth is
   per-config-dir though, so each env needs its own credentials — unlike aello,
   where one Claude token is shared. Read `--help` before concluding a feature
   is absent.

4. **Rules — measured with four canary tokens in one run.** Loaded:
   `<project>/AGENTS.md`, `<project>/.clinerules` (file *or* directory), and
   `<project>/.cline/rules/*.md`. **`CLAUDE.md` is ignored.** Global rules live
   in `~/.cline/rules/*.md` and apply to every project and session; there is no
   per-session toggle over them. Constants: `RULES_CONFIG_DIRECTORY_NAME =
   "rules"`, `RULES_FILE_NAME = "AGENTS.md"`, plus `skills`, `workflows`,
   `hooks`, `plugins`, `agents`.

5. **The response rules aello uses now live at
   `C:\Users\H\.cline\rules\response-rules.md`** — concise / no sycophancy / no
   plans / one closing `TL;DR:` block with 3–4 steps, the same four the aello
   per-turn hook carries ([[aello-voice-capability]] #15). Verified over six
   live runs including a real project: replies came back terse with a one-line
   TL;DR. Unlike aello there is no per-turn injection, but Cline re-sends rules
   in the system prompt every request, so they do not decay the way a
   one-shot SessionStart injection would. Only single-turn behaviour was
   measured; a long multi-turn session was not.

6. **Cline's hooks fire but do not inject — this is the trap.** Hook files are
   named after the event (`TaskStart.py`, `UserPromptSubmit.ps1`, …) in a
   `hooks/` dir; the basename is matched case-insensitively and the accepted
   extensions are `"" .sh .bash .zsh .js .mjs .cjs .ts .mts .cts .py .ps1`.
   Events: `TaskStart` `TaskResume` `TaskCancel` `TaskComplete` `TaskError`
   `PreToolUse` `PostToolUse` `UserPromptSubmit` `PreCompact`
   `SessionShutdown`. Measured: `TaskStart.py` **does** run (612-byte payload,
   in both one-shot and `-z` hub mode) — but its documented output keys
   `contextModification` **and** `context` never reached the model; a canary
   token was absent from two runs. `UserPromptSubmit` **never fired at all**,
   and `prompt_submit` appears **zero** times in this machine's entire
   `~/.cline/data/logs/hooks.jsonl` history. So a hook that is obviously
   installed and obviously executing can still be doing nothing. Untested:
   whether `UserPromptSubmit` fires in the interactive TUI (needs a real TTY).

7. **No memory system, and no ignore file.** `grep -a` over the binary (sanity-
   checked: `clinerules` returns 2 hits) finds `clineignore` **0** times, and
   the only `auto-memory`/`autoMemory` strings belong to **Claude Code's own
   settings schema**, which Cline embeds as a dependency — not a Cline feature.
   These are the two real gaps against Claude Code; everything else has an
   equivalent.

8. **There *is* a `claude-code` provider, so Cline can use a Claude
   subscription.** `-P claude-code -m opus|sonnet|haiku|fable`; it shells out
   to the Claude Code CLI ("the Claude Code CLI executes its own tools"), reads
   `CLAUDE_CONFIG_DIR` and `~/.claude/.credentials.json`. I told the user twice
   it could not before checking the binary. Not proven end to end: on this
   machine it fails with *OAuth session expired and could not be refreshed*,
   and it also reports **`C:\Users\H\.claude\.claude.json` missing** with a
   backup at `.claude\backups\.claude.json.backup.1782431513114`. Note that a
   child process inherits the *aello* env's `CLAUDE_CONFIG_DIR`, which has no
   `.credentials.json`, so an unset-and-retry is part of any test.

9. **`--auto-approve` defaults to `true`** — Cline CLI approves every tool call
   unless told otherwise, which Claude Code never does. Worth knowing before
   pointing it at anything that matters.

10. **Only in Cline, no Claude Code CLI equivalent:** any provider (OpenRouter,
    Bedrock, Vertex, Ollama, LM Studio, opencode, openai-codex-cli, dify);
    `cline schedule` (cron); `cline connect <channel>`; `cline kanban` /
    `cline dashboard`; `-z/--zen` background sessions on a persistent hub
    daemon; `--acp`; `cline doctor`; `cline history export` to standalone HTML;
    the `hooks.jsonl` audit log; agent teams as a first-class concept. Also
    `-s/--system` **replaces** the system prompt (Claude Code appends), and
    `cline config` requires a TTY even with `--json`.

12. **The `claude-code` provider authenticates fine on aello's shared token — and then cannot use tools at all.** Measured 2026-08-06 while scoping an aello integration, in a fully isolated tree (`--config`/`--data-dir` under the scratchpad). Supplying aello's `oauth_token` from `%APPDATA%\aello\config\config.toml` as `CLAUDE_CODE_OAUTH_TOKEN`, with `CLAUDE_CONFIG_DIR` unset, returned the canary immediately — so **#8's "OAuth session expired" was simply the absence of a token, not an expired one**, and one aello token serves every Cline env exactly as it serves every Claude env. That kills the "each env needs its own credentials" worry in #3 *for this provider only*. **But every tool call is rejected before execution**: `Tool call Write was rejected before execution: Claude requested permissions to write to …, but you haven't granted it yet`, followed by `[abort] aborted by another client`. The AI SDK warning says why — *"The feature 'tools' is not supported. The Claude Code CLI executes its own tools; AI SDK tools cannot be auto-bridged at the provider layer and will be ignored."* Cline's approval layer has no path to approve a tool the Claude Code CLI owns. **Three levers tried, none worked:** `--auto-approve true` explicitly (it already defaults true), a `CLAUDE_CONFIG_DIR` holding `settings.json` with `permissions.defaultMode = bypassPermissions` plus an allow-list, and a `.claude.json` with `hasCompletedOnboarding`. So on a Claude subscription **Cline is chat-only** — it will talk, reason and refuse to lie about it ("I won't falsely claim the file was created"), but it cannot edit a repo. Real work needs a metered provider, which is exactly the per-token spend aello exists to avoid. This is an upstream limitation, not a configuration mistake; re-test it after a Cline upgrade before designing around it.

13. **Isolation confirmed, and `--data-dir` is the load-bearing flag.** Same session: `--config <dir> --data-dir <dir>` produced `data/settings/providers.json`, `data/db/sessions.db`, `data/sessions/<id>/`, `data/logs/cline.log` — and `~/.cline` had **no** file modified in the previous 10 minutes. The `--config` dir stayed empty except for what I put there, so provider config follows `--data-dir`, not `--config`.

14. **Only `TaskStart` fires, only from `<config>/hooks/`, and its payload carries no content — so nothing aello does with hooks survives a port.** Measured by dropping identical marker hooks for `TaskStart`, `TaskComplete`, `SessionShutdown`, `UserPromptSubmit` and `PostToolUse` into **both** `<config>/hooks/` and the `--hooks-dir` path, in one run that made a tool call. Exactly one fired: `<config>/hooks/TaskStart.py`. **`--hooks-dir` fired nothing at all**, despite `--help` describing it as additional hooks — so the flag is either additive-and-broken or means something else. The `TaskStart` payload is identifiers only: `taskId`, `sessionContext.rootSessionId`, `workspaceRoots`, `workspaceInfo.rootPath`, `agent_id`, `parent_agent_id`, `hookName: "agent_start"` — **no prompt, no response, no transcript path**, and `clineVersion` is an empty string. Consequences for an aello port, all three of them structural: the **voice** cannot ride a hook (nothing fires at the end of a response, and `TaskStart` has no text anyway), the **four response rules** cannot ride a per-turn hook (`UserPromptSubmit` still never fires — #6 said the same) and must go in a rules file instead (#5, measured working), and **contextdb** has no session-end event, so capture would have to read `<data>/sessions/<id>/` after the process exits rather than hook it. Reading the sessions dir is arguably better than a hook, but it is a different mechanism, not a port.

15. **Cline is integrated into aello rather than being its own project — shipped 2026-08-06 (`9332815`).** The user's call, and the framing was theirs: aello gets bigger and more functional, and the two halves are split so nothing mixes. `--agent claude|cline` on `add`, `.cline-env-<name>` beside `.claude-env-<name>`, everything Cline in `src/cline.rs`, `aello login` asking which account. Existing blueprints migrate by serde default alone. **They also chose the metered path deliberately** after being shown that the subscription cannot drive an editing Cline env, and that Cline uses provider keys/OAuth of its own — so aello now stores a credential that costs money per token, which is a change in what aello *is*, not just a new field. **Voice on Cline was offered and declined**: aello launches the process and could have read the response off `--json` and spoken it, but the user said no voice on Cline envs.

    ⚠️ **The trap here, and it is this repo's exact shape: writing `providers.json` by hand half-works.** aello did that first — the file is small and an example is right there in `~/.cline`. The env placed, the run launched, the provider was reached, and it returned an error that read like a bad key. What actually happened is that **Cline rewrote `providers.json` on its next run and dropped `apiKey` entirely**, leaving `provider`/`model`/`tokenSource`, so the request went out with no credential. Found by diffing the file either side of a run rather than by trusting that the run "reached the provider". A key installed by `cline auth -p … -k … -m … --data-dir …` survives that same run untouched — **the difference is the writer, not the value**. `cline auth` is fully non-interactive, so aello shells out to it on every run.

    **Cline's own dirs, measured:** provider settings follow `--data-dir` (`<data>/settings/providers.json`), rules follow `--config` (`<config>/rules/*.md`) — two different flags, and swapping them fails silently in both directions. `<config>/rules/` beat `<data>/rules/` *and* a project `AGENTS.md` in a three-way canary. `~/.cline` had no file touched during any isolated run, so the isolation is real.

    **A Cline env dir must be gitignored unconditionally**, unlike aello's role-gated `.claude-env-*` line: it holds the API key in plaintext, so a standalone blueprint leaks exactly as well as a maintainer. That asymmetry is the kind of thing that reads as an inconsistency later — it is not.

16. **Cline has NO user-defined slash commands and NO memory — both measured, and both had to be built around (`2429773`).** These are the two gaps that shape everything aello does for a Cline env.

    **Slash commands:** `/canary` was installed as a workflow *and* a skill in all four candidate locations (`<workspace>/.cline/workflows/`, `<workspace>/.cline/skills/`, `<config>/workflows/`, `<config>/skills/`) and came back as ordinary prose. The only slash commands in the binary are connector built-ins — `/abort`, `/clear`, `/exit`, `/start`, `/whereami`, `/schedule`, `/mute` — for `cline connect` channels. Cline's own skills/workflows resolve under `<workspace>/.cline/<plugin>/{skills,workflows}` as **plugin** artifacts (`skillsPath`/`workflowsPath` are plugin-relative), not as something a user types. So `/sync` cannot be *registered*; aello **routes** it from a rules file that names each command against its `SKILL.md` absolute path.

    **Memory:** `memory-bank`, `memoryBank`, `cline_docs`, `MEMORY.md`, `rememberThis` = **0 hits**; the single `auto-memory` hit is Claude Code's embedded settings schema (confirming #7 by a second route). aello seeds `<env>/memory/MEMORY.md` once and supplies the discipline as a rule.

    **Both are instructions the agent follows, not hooks aello enforces** — strictly weaker than the Claude equivalents. Worth saying out loud whenever someone assumes parity.

17. **`<config>/rules/*.md` is the only channel that works, and it is a good one.** Cline re-sends rules in the system prompt on **every request**, which independently achieves what aello's per-turn `UserPromptSubmit` hook achieves for Claude — no decay by turn eighty. Everything a Cline env gets rides it: response rules, persona, the standing aello block + command router, and the memory rule. The persona is the same bundled template text, just written to `rules/persona.md` (Cline ignores `CLAUDE.md` — #4). **The removal branch matters more than the write:** rules apply every request, so a persona left behind after switching to `none` keeps applying with nothing to clear it.

    ⚠️ **Forward-slash every path you interpolate into a rule.** The templates join with `/` while Windows `Path` yields `\`, so a substituted path came out `C:\…\skills/sync/SKILL.md` — functional, but matched by nothing, and only a test comparing router text to the real path caught it.

    ⚠️ **Cline refuses a ONE-WORD prompt** (`-p "hi"` → *"Unknown command or unquoted prompt: hi"*) because it reads a bare word as a possible subcommand. It is not a shell-quoting problem. Every test written before this used a sentence, which is exactly why it survived to a real run.

18. **The routing works — verified against a real OpenRouter model, and the first "verification" was a false positive.** 2026-08-06 (`c7b911e`). `/sync now` opened the memory index, looked for `AGENTS.md`, correctly reported nothing to change; `/handoff please` wrote a name-prefixed `ClineReal.HANDOFF.md` with the sections the skill asks for. Model: `openai/gpt-oss-120b` on OpenRouter (~$0.04/$0.17 per Mtok — pick tool-capable cheap models by filtering `supported_parameters` for `tools` on `/api/v1/models`). The tool-less `claude-code` provider cannot test any of this: opening a skill file *is* a tool call.

    ⚠️ **Cline refuses ANY one-word prompt** — `hi`, `/sync`, `/twosentences` alike — with *"Unknown command or unquoted prompt"*. So in one-shot `-p` mode every command needs a trailing word (`/sync now`), and the router must match a **prefix**, not an exact message. The TUI has a real `/` menu and is unaffected (untested — needs a TTY).

    ⚠️⚠️ **The false positive is the transferable lesson.** An early test showed `-p "/twosentences"` apparently working: the agent read the skill file and said "According to the skill file…". Git Bash had rewritten the argument to `C:/Program Files/Git/twosentences` — which contains a space, so Cline accepted it, and the model **guessed** the skill from the path. Real evidence, wrong conclusion, and it was reported to the user as verified. **Set `MSYS_NO_PATHCONV=1` / `MSYS2_ARG_CONV_EXCL='*'` whenever a leading-slash argument goes through a POSIX shell on Windows**, or you measure the shell instead of the thing.

19. **Real slash commands would mean shipping a JavaScript plugin — decided against.** The TUI `/` menu = five built-ins (`config`, `settings`, `mcp`, `fork`, `team`) **plus `listRuntimeCommands()`**, which comes from installed plugins; each runtime command carries `name`/`instructions`/`description`. But `cline plugin install <dir>` on a folder of markdown fails with **"No plugin entry files found"** — a plugin is an npm-style package with a JS entry (`index.js`/`.mjs`). And `--cwd` installs to **`<project>/.cline/plugins`**, workspace-scoped, so one plugin would be shared by every env in a repo rather than isolated per blueprint. A node dependency plus a hole in the isolation model, for cosmetics. Revisit only if Cline gains config-dir-scoped or markdown-only plugins.

    **`@file` is not a separate mechanism.** The TUI hint string is *"/ for slash commands, @ for file mentions, Ctrl+P for menu"*, but in one-shot mode naming `@canary.txt` simply made the model issue an ordinary `read_files` call. In the TUI the `@` is a picker for convenience. Nothing to build on that a plain path in a rule does not already give.

11. **The user already has a 53 KB `claude-code-vs-cline-comparison.md`** at
    `C:\Users\H\Desktop\ai-tools\cline8\`, dated 2026-08-04, alongside
    `cline-complete-guide.md`. Its executive summary was accurate on the points
    independently verified here. Read it before re-deriving the product
    comparison; this note only covers what running the thing revealed.
