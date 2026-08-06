---
name: cline
description: "Cline on this machine — it is the CLI not the VS Code extension, how its config/rules/hooks/providers map onto Claude Code's, what is genuinely absent (memory, ignore file), which of its mechanisms are measured working and which only look wired"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4742e7a2-20a8-4f4b-bb8d-1bae2f0fd8ea
  modified: 2026-08-06T09:13:14.084Z
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

11. **The user already has a 53 KB `claude-code-vs-cline-comparison.md`** at
    `C:\Users\H\Desktop\ai-tools\cline8\`, dated 2026-08-04, alongside
    `cline-complete-guide.md`. Its executive summary was accurate on the points
    independently verified here. Read it before re-deriving the product
    comparison; this note only covers what running the thing revealed.
