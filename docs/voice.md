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

If they disagree, that env is behind and its next `run` refreshes it. Two unit tests keep the vendored copy honest: one compares the recorded constant against the vendored `speak.py`, and one digests **all five** files — the constant lives in `speak.py` alone, so a re-vendor touching only `duck.py` or `win_audio.ps1` would otherwise slip past. The digest test prints the new value when it fails.

## Shared state, per-env scripts

The scripts are per-env; their state is not. The voice pool, per-session leases and mute flags live in one machine-wide folder:

| OS | Path |
|---|---|
| Windows | `%LOCALAPPDATA%\revoiced` |
| macOS | `~/Library/Application Support/revoiced` |
| Linux | `$XDG_DATA_HOME/revoiced` (or `~/.local/share/revoiced`) |

That shared state is what makes concurrent envs behave: each session leases a **different** voice, playback serialises behind one machine-wide lock instead of several envs talking over each other, and one mute covers everything. `aello voice` writes that file directly — no Python, no placed env, works from any directory — preserving keys it does not own, since the hook owns the rest of the file.

## Something has to ask for the TL;DR line

The hook speaks that line and nothing else, so something must instruct the agent to write one. Placement registers a `UserPromptSubmit` hook running `user-prompt-submit.py`, which injects the instruction on **every prompt** — along with the two other response rules every env carries (see below). It is healed into an existing env the same way the voice hooks are, so an env placed before it existed picks it up on its next run.

Enforcement does not depend on that instruction: a response with no `TL;DR:` line is blocked once with a request to add one, then allowed through, so it cannot loop. The injected text only saves the round trip.

**Why per turn and not in the persona.** The persona is written once, never clobbered, and is the file most likely to be rewritten wholesale — an awkward place for the voice's only input. It is also delivered once per session, and a style instruction given at turn one is buried by turn eighty. `place()` rewrites the hook script on every run, so a change to the wording reaches an env placed months ago without touching anything you own.

If you unregister the hook by hand, `place` falls back to appending the TL;DR section to the persona, so the voice never goes silent for want of an instruction.

## The three response rules

The same hook carries three rules, injected together on every prompt in every env (~150 tokens per turn):

- **Be concise** — no preamble, no filler, no hedging, no restating the question.
- **No sycophancy** — don't open with praise or agreement, don't validate an unchecked premise, don't soften a finding to be agreeable; say plainly when the user is wrong, and say "I don't know" when that's the answer.
- **End with `TL;DR: <two sentences>`** — the line the voice speaks.

They live together because all three are about how a single response is written, which is why they are delivered per turn rather than per session. Editing `src/hooks_user_prompt_submit.py` changes them everywhere on each env's next run; a unit test pins the wording so a rule cannot be dropped by accident.

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
