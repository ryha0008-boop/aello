# The voice

Every placed env speaks. There is no flag and nothing to enable: placement writes five files into `<env>/hooks/` and registers a `Stop` hook that reads each response's trailing `TL;DR:` line aloud through a free Edge neural voice, plus a `SessionEnd` hook that hands the borrowed voice back to the pool.

This page is the mechanism. For "how do I turn it off", the README is enough.

## Why it is not a role setting

It was one (`--voice`) until it stopped earning the flag. Choosing per blueprint bought nothing — a blueprint that maintains no project file has no reason to be mute — and the one moment you actually want silence is *right now, because a machine started talking*, which a placement flag answers far too slowly. So the flag is gone along with `--no-voice`, and silence lives entirely in `aello voice mute`. A `voice = …` left in an existing config is ignored on load and dropped on the next save.

That decision is why this page is not in `roles.md`.

## Why the hook is vendored into the env

The five files are copied to `<env>/hooks/` and registered as `$CLAUDE_CONFIG_DIR/hooks/speak.py` — never a path to a checkout. The alternative couples unrelated projects to one directory: move or rename it and every env goes silent at once, and each newly placed env stays silent until edited by hand.

`speak.py` imports `duck`, `focus` and `notify` as siblings and shells out to `win_audio.ps1` beside it, so all five travel together and are rewritten on every placement — an env that has fallen behind catches up on its next `run`.

## Why all five, when two are optional

`speak.py` wraps the `focus` and `notify` imports in `try`/`ImportError` so a partial copy still speaks rather than dying on every response. That guard once let aello ship three of the five, and the result was worse than a crash: every env spoke normally while `notify`'s stub quietly returned "not shown", so **no desktop notification was raised anywhere and nothing said so**. It was reported broken twice and measured healthy twice, because the tests ran the repo's copy — which has its siblings — rather than the env copy the hook executes.

Guarded means a partial copy survives, not that the file is optional. Vendor all five. `speak.py --status` now leads with `INCOMPLETE COPY - no …` when a sibling is missing, so the same failure cannot be silent twice.

## Who registers the toast identity

On Windows a toast is shown *as* a registered application, and one sent under an identity Windows does not know is dropped with no error — the same silent shape, one layer out. Launching an env runs the vendored `notify.py --register` (no test toast, idempotent), so toasts work on a machine that has never started the [revoiced](https://github.com/ryha0008-boop/revoiced) station.

Registration has two halves with different owners:

- The **identity** is just a name a toast is shown under. Any copy may claim it.
- The **`revoiced:` protocol** is a promise to *run* something — it points at an `action.py` beside whichever copy registered it, and an env's `hooks/` has none, since aello vendors the five hook-path files and not the station.

Upstream splits them accordingly, so a vendored copy claims the identity and leaves the protocol alone, and cannot take a working toast button away from the station. Before that split, registering from an env would have repointed both buttons at a missing file on every launch.

The call is deliberately **not** cached behind a marker file. The handler command embeds absolute paths to `pythonw.exe` and `action.py`, so a Python upgrade or a moved repo invalidates it — a marker would be a cache that goes stale exactly when the truth changes. Re-registering on every launch is what self-heals it.

## Knowing when a copy is behind

`speak.py` carries a module-level `HOOK_VERSION`, bumped upstream whenever one of the five files changes — station-only work leaves it alone, so a mismatch always means real drift in code you run.

- `aello voice status` prints the version **aello vendored**.
- `python <env>/hooks/speak.py --hook-version` prints the version **that env runs**, and prints before any optional import, so even a partial copy answers.

If they disagree, that env is behind and its next `run` refreshes it. Being behind is not always harmless: a copy below 11 lowers other applications' volumes and can fail to put them back, permanently — see [troubleshooting.md](troubleshooting.md). Two unit tests keep the vendored copy honest: one compares the recorded constant against the vendored `speak.py`, and one digests **all five** files — the constant lives in `speak.py` alone, so a re-vendor touching only `duck.py` or `win_audio.ps1` would otherwise slip past. The digest test prints the new value when it fails.

## Telegram (opt-in, `HOOK_VERSION` 13 and up)

From 13, `speak.py` also sends the spoken line and its mp3 to a Telegram chat, so a response reaches you away from the machine. It is off unless **all three** variables are set:

| Variable | Meaning |
|---|---|
| `REVOICED_TELEGRAM` | `1` to enable; anything else, or unset, is off |
| `TELEGRAM_BOT_TOKEN` | the bot's API token |
| `TELEGRAM_CHAT_ID` | the chat to deliver to |

aello does not set these. A blueprint that wants to differ can override any of them, and `REVOICED_TELEGRAM=0` is a real opt-out.

From **14**, a name that is *absent* from the process environment falls back to the persisted `HKCU\Environment` value on Windows, so setting these machine-wide reaches sessions that are already open. That fallback exists because 13 did not: Windows only seeds a process environment at creation, so at 13 the variables worked in terminals started afterwards and were silently inert in every session already running — on, and sending nothing. The fallback fires **only on absent**, never on present, so a blueprint's `0` or an explicitly empty value still wins.

`python <env>/hooks/speak.py --status` prints a `telegram` line naming the source of each value — `set`, `set at User scope, picked up from there`, or `not set`. That line is the check that proves the variables reached the process; `--hook-version` only proves the code is there. To tell 13 from 14 you must run it from a shell that never inherited the variables — a fresh shell has them either way and reports the same on both.

## Shared state, per-env scripts

The scripts are per-env; their state is not. The voice pool, per-session leases and mute flags live in one machine-wide folder:

| OS | Path |
|---|---|
| Windows | `%LOCALAPPDATA%\revoiced` |
| macOS | `~/Library/Application Support/revoiced` |
| Linux | `$XDG_DATA_HOME/revoiced` (or `~/.local/share/revoiced`) |

That shared state is what makes concurrent envs behave: each session leases a **different** voice, playback serialises behind one machine-wide lock instead of several envs talking over each other, and one mute covers everything. `aello voice` writes that file directly — no Python, no placed env, works from any directory — preserving keys it does not own, since the hook owns the rest of the file.

## Something has to ask for the TL;DR line

The hook speaks that line and nothing else, so something must instruct the agent to write one. Placement registers a `UserPromptSubmit` hook running `user-prompt-submit.py`, which injects the instruction on **every prompt** — along with the three other response rules every env carries (see below). It is healed into an existing env the same way the voice hooks are, so an env placed before it existed picks it up on its next run.

Enforcement does not depend on that instruction: a response with no `TL;DR:` line is blocked once with a request to add one, then allowed through, so it cannot loop. The injected text only saves the round trip.

**Why per turn and not in the persona.** The persona is written once, never clobbered, and is the file most likely to be rewritten wholesale — an awkward place for the voice's only input. It is also delivered once per session, and a style instruction given at turn one is buried by turn eighty. `place()` rewrites the hook script on every run, so a change to the wording reaches an env placed months ago without touching anything you own.

If you unregister the hook by hand, `place` falls back to appending the TL;DR section to the persona, so the voice never goes silent for want of an instruction.

## The four response rules

The same hook carries four rules, injected together on every prompt in every env (~300 tokens per turn):

- **Be concise** — no preamble, no filler, no hedging, no restating the question; the prose stays at a few sentences, and anything that matters goes in a step rather than a paragraph.
- **No sycophancy** — don't open with praise or agreement, don't validate an unchecked premise, don't soften a finding to be agreeable; say plainly when the user is wrong, and say "I don't know" when that's the answer.
- **No plans** — never hand over a plan for approval and never use plan mode; ask a short question or do the work, and where the choice is genuinely the user's, offer concrete options to pick from.
- **Close with one block and nothing after it** — a single `TL;DR: <two to four sentences>` line giving the outcome, then 3–4 numbered next steps beneath it whenever anything is left for you to do. The steps must **stand alone**: the rule assumes you read nothing above the block, so anything you need is in a step and no step may say "as described above". The steps are dropped when nothing is waiting on you; the TL;DR line never is.

That fixes the shape of every answer: a few sentences of prose, then the closing block. Summary and steps were two separate rules for about an hour, and that was one section too many — the summary said a thing, the steps repeated it, and you had to reconcile them. Merged, the spoken line introduces the list it sits on top of.

**Standing alone is the whole point, and it is the half that gets lost.** The first version produced correct steps under an essay that still had to be read to make sense of them — the failure it was written to prevent, since a wall of text is not an instruction. That is why the concise rule caps the prose too: the two only work as a pair.

⚠️ **The TL;DR stays on one line and the steps stay below it.** `extract_tldr` in `speak.py` matches `^…TL;DR:\s*(.+?)$` and takes the last match, so a summary wrapped onto a second line is spoken with its tail silently cut off. Numbered steps underneath match nothing, which is what makes this ordering safe — verified by running a sample response through the real `extract_tldr`, not by reading the regex.

They live together because all five are about how a single response is written, which is why they are delivered per turn rather than per session. Editing `src/hooks_user_prompt_submit.py` changes them everywhere on each env's next run; a unit test pins the wording so a rule cannot be dropped by accident.

⚠️ **The steps rule and the no-plans rule are one careless sentence apart.** Both are about numbered lists, and they point opposite ways. The line is whose hands the actions are in: a plan is what the *agent* is about to do, held for approval; the steps are what the *user* does next. That is why the no-plans rule no longer forbids "numbered proposals" (it forbids laying out what you intend to do and waiting for sign-off) and the steps rule says "their actions, not yours" outright. Reword either one and check it still reads as the opposite of the other.

**The no-plans rule has a second half.** Placement also registers a `PreToolUse` hook (`pre-tool-use.py`) matching `EnterPlanMode|ExitPlanMode`, which denies both outright — so plan mode is unavailable rather than merely discouraged. Both halves are needed and neither is redundant: the hook stops the tool, and only the injected text stops a proposal written as ordinary prose, which is what a plan usually looks like. The matcher is not decoration — an unmatched `PreToolUse` group runs on *every* tool call, which is a Python spawn per `Read` in every env.

⚠️ **Verified halfway, deliberately on the record.** A `PreToolUse` deny provably blocks a tool and hands its reason back to the model — measured against `Read`, which returned the reason string instead of the file — and the matcher provably scopes it, since a `Glob` in the same run never reached the hook. What is *not* verified is that the two plan tools emit a `PreToolUse` event at all: `claude -p` never calls `ExitPlanMode` under `--permission-mode plan`, so print mode cannot answer it. That is why a denial appends a line to `plan-blocked.log` beside the script. Nothing reads that file and deleting it costs nothing; it exists so the first real denial settles the question. If it stays empty across envs that have obviously wanted to plan, the block is not firing and the injected text is doing all the work.

## Migrating a hand-wired hook

If an env already has a `Stop` hook you added yourself pointing at a checkout (`python "C:/…/revoiced/speak.py"`), placement **replaces** it with the env-relative one on the next `run`. Not added beside it — that would speak every response twice — and not left alone, since that absolute path is the coupling vendoring exists to remove. Hooks that are not a `speak.py` are never touched.

## When it doesn't speak

Check `aello voice status` first; a global or per-project mute is the usual answer.

Beyond that, the hook appends to `history.jsonl` in its state dir for every response it handles, recording the project, the voice used, the text, and the audio file:

- an entry naming a **real voice** — synthesis worked, so the problem is playback;
- `system fallback voice` — `edge-tts` wasn't found and the OS voice was used;
- **no entry at all** — the hook never ran, or the response had no `TL;DR:` line.

An env picks up hook changes on its next `aello run`, so an env still in a session started beforehand stays as it was until restarted.

## Prerequisites

Python 3 on `PATH`. Without `edge-tts` (`pip install edge-tts`) it falls back to the OS voice — SAPI on Windows, `say` on macOS, `spd-say`/`espeak` on Linux. Linux playback also needs one of `mpv`, `ffplay`, `mpg123` or `cvlc`; macOS (`afplay`) and Windows (.NET) are covered by the OS. Ducking other applications' audio while it speaks is Windows-only, needs `pycaw`, and is a no-op elsewhere.
