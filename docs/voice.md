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

If they disagree, that env is behind and its next `run` refreshes it. Being behind is not always harmless: a copy below 11 lowers other applications' volumes and can fail to put them back, permanently, and a copy below 15 does not run the sweep that repairs what a restore could not reach — see [troubleshooting.md](troubleshooting.md). Two unit tests keep the vendored copy honest: one compares the recorded constant against the vendored `speak.py`, and one digests **all five** files — the constant lives in `speak.py` alone, so a re-vendor touching only `duck.py` or `win_audio.ps1` would otherwise slip past. The digest test prints the new value when it fails.

## Putting back what the duck could not (`HOOK_VERSION` 15, widened at 16)

The hook lowers other applications while it speaks and restores them afterwards, but both the restore and the station's recovery work from the **live audio session enumeration** — so neither can reach an application that has gone quiet or exited. Two holes follow, and no guard on that path closes either:

- An application that goes quiet and then **exits** is dropped by the restore's liveness filter and stays lowered, with no record left that it ever was.
- When it comes back it carries a new process id, so it is a *fresh* key whose stored 0.15 is read as its normal. The next duck lands on 0.0225.

A reboot is the first case for everything at once. Measured upstream on this machine: the duck wrote at 11:15:39, the machine went down at 11:16:05, and three applications came back at 15% with the duck's own record gone.

From 15 the hook reads the volumes **Windows has persisted** instead — the only view that outlives the session, the process and the reboot — and puts back what is still down. `REVOICED_SWEEP` takes three values:

| Value | Claims |
|---|---|
| `0` | nothing; the sweep never runs |
| `signature` | only values the current duck level could have produced (`level`, `level²`, … down to the 0.01 clamp) |
| anything else (**default**) | every stored volume between 0 and full |

The wide default is deliberate and answers a hole the narrow rule has: the signature is computed from the duck level *as it is now*, so raising it in the station from 15% to 25% orphans every 0.15 already on disk — claimed by nothing, reported by nothing. aello does not set `REVOICED_SWEEP` — set it yourself, machine-wide or per blueprint, and use `signature` where some application is meant to be quiet.

**An exact `0` is never touched in either mode.** A duck is clamped at 0.01 and never reaches zero, so a zero is somebody's own mute — and on this machine that somebody is often Wispr Flow, which drops everything to 0 while you dictate and puts it back within seconds.

⚠️ **revoiced is not the only ducker here, and that is not fixable from this side.** Dictating *while* an env is speaking has both of them sampling each other: revoiced records 1.0 and lowers to 0.15, the other ducker then samples **0.15** as normal and zeroes, and whichever restores last wins — when it is the other one, the machine lands back at 0.15. That is why the damage kept returning between reboots. Nothing in the hook can prevent it; the sweep is what closes it, one turn later, and it is the strongest argument for the wide default: the other ducker restores whatever it happened to sample, and any level but 0.15 would be orphaned by the narrow rule.

**Timing is load-bearing.** The sweep runs at the *start* of a turn, before this turn's duck, not after the previous restore — the registry lags the live session by several seconds (a stored value still read 0.15 five seconds after the duck record was gone), so a sweep taken straight afterwards finds its own duck and "repairs" it. It also refuses while the speaker lock is fresh or a duck record exists, for the same reason: a duck in progress is indistinguishable from one that failed. A scan taken mid-line upstream reported three applications as damaged and all three were correct.

Ask a placed copy directly with `python <env>/hooks/speak.py --sweep`: it prints the duck level, the mode, every stored volume below full marked `CLAIM` or `kept `, and what it repaired. `speak.py --status` prints a `volume repairs` line summarising the last 50 turns — `none` is the healthy reading, and a count over turns is the only trustworthy form of it.

⚠️ **A `--sweep` reading from a copy below 24 is worthless in either direction.** The command reaches for two `duck.py` functions that the guarded-import stub never carried, and the top-level `except Exception: sys.exit(0)` swallowed the resulting `AttributeError` — so an incomplete copy printed two lines, exited 0, and never ran the check at all, output-identical to a machine with nothing to repair. From 24 it names the missing sibling before printing any figure. Take the version first, then the sweep.

Two more holes were open until 22, both of which made this path quieter than it looked: the live-session repair matched **0 of 49** stored entries against 6 live sessions (the two sides spell an endpoint differently and were compared by substring), so every repair fell through to the unverified registry write; and the sweep switched itself off whenever `duck.json` merely *existed*, which is exactly the state a failed restore leaves behind.

Repairs go through the live session where there is one, because setting a session's volume writes through to every stored entry for that executable. Only where no session exists does the hook patch the persisted float itself, and **that half is unverified**: the value provably persists (measured 0.15 → 1.0, still 1.0 afterwards), but whether the audio engine honours it at that application's next launch, rather than overwriting it from a cached copy, is not known. If it does not, the registry fallback is decoration and only the session path is real.

## Telegram (opt-in, `HOOK_VERSION` 13 and up)

From 13, `speak.py` also sends the spoken line and its mp3 to a Telegram chat, so a response reaches you away from the machine. It is off unless **all three** variables are set:

| Variable | Meaning |
|---|---|
| `REVOICED_TELEGRAM` | `1` to enable; `0`, empty, or unset is off |
| `TELEGRAM_BOT_TOKEN` | the bot's API token |
| `TELEGRAM_CHAT_ID` | the chat to deliver to |

aello does not set these. A blueprint that wants to differ can override any of them, and `REVOICED_TELEGRAM=0` is a real opt-out.

From **14**, a name that is *absent* from the process environment falls back to the persisted `HKCU\Environment` value on Windows, so setting these machine-wide reaches sessions that are already open. That fallback exists because 13 did not: Windows only seeds a process environment at creation, so at 13 the variables worked in terminals started afterwards and were silently inert in every session already running — on, and sending nothing. The fallback fires **only on absent**, never on present, so a blueprint's `0` or an explicitly empty value still wins.

`python <env>/hooks/speak.py --status` prints a `telegram` line naming the source of each value — `set`, `set at User scope, picked up from there`, or `not set`. That line is the check that proves the variables reached the process; `--hook-version` only proves the code is there. To tell 13 from 14 you must run it from a shell that never inherited the variables — a fresh shell has them either way and reports the same on both.

**An empty value was on until 17.** `REVOICED_TELEGRAM=` set to nothing opted *in*, while the docstring one line above promised the opposite — the fallback correctly returned `""` for a present-but-empty name and `"" != "0"` is true. It is off from 17. Worth knowing how that hid: PowerShell's `$env:X = ''` **deletes** the variable rather than emptying it, so testing it that way measures the *absent* case and reports a pass. It takes a subprocess with an explicit empty value.

**A send that fails says so from 18.** Before that, a timeout, a revoked token, a wrong chat id and an API `ok:false` all produced exactly nothing — no history entry, no stderr, no retry, and the line still spoken locally, so nothing about the session looked wrong. 18 records it in the shared `state.json` as `telegram_error`, cleared by the next send that works, so it reads "right now" rather than "ever". Both `speak.py --status` and **`aello voice status`** print it; absent is the healthy reading. It is on the state file rather than the history entry because `record()` has already appended by the time the send is attempted — deliberately, so a 30-second upload cannot sit between a finished turn and the station showing it — and putting it on the entry would mean rewriting the whole history on the hook path, once per turn, in every env. That is a key **revoiced owns**: aello only reads it, and the round-trip test in `voice.rs` pins that a mute does not erase it.

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
