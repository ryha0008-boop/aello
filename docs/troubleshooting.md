# Troubleshooting

Failure modes and what they actually mean. Most of these look like something else first — that's why they're here.

## Where things live

Knowing these three paths resolves a lot on its own.

| What | Where |
|---|---|
| aello's config (blueprints, token, contextdb root) | `%APPDATA%\aello\config\config.toml` · `~/.config/aello/config.toml` |
| A placed environment | `<project>/.claude-env-<name>/` |
| An env's memory | `<env>/projects/<encoded-cwd>/memory/` |
| Shared voice state (pool, leases, mutes) | `%LOCALAPPDATA%\revoiced` · `~/Library/Application Support/revoiced` · `$XDG_DATA_HOME/revoiced` |

Note the **extra `config/` level** on Windows: `aello\config\config.toml`, not `aello\config.toml`.

**Don't construct the memory path by hand.** Claude folds every non-alphanumeric character in the project path to `-`, and guessing that encoding reliably produces a second, empty memory directory that nothing ever reads. List `<env>/projects/` and use the directory that's there.

## Claude asks me to log in, even though `aello login` worked

The token authenticates the API, but an interactive `claude` still shows its first-run wizard on a config dir it has never seen. Placement seeds `hasCompletedOnboarding` into `.claude.json` to skip it — so if you're seeing the wizard, you're most likely looking at an env dir that was created outside `aello run`, or one whose `.claude.json` was replaced.

Re-running `aello run <name>` re-seeds it.

## `aello login` looks hung

It shells out to `claude setup-token`, which prints an auth URL. aello tees that output rather than piping it, precisely so the URL is visible on a headless box — if you see nothing at all, the underlying `claude` command is the thing to check.

## My edit to a skill disappeared

Expected: all four seeded skills are rewritten on every `aello run`. Pin the one you edited with an empty `<env>/skills/<skill>/.aello-keep` and it will be left alone. See [skills.md](skills.md).

If the role also mirrors the env into `claude-internal/`, the generated version may already have been committed over yours — your edit is in git history, not in the working tree.

## `aello update` says it upgraded, but nothing changed

`aello update` installs a **published release**. It cannot deliver uncommitted work, branch work, or anything that hasn't been through CI — and it will report success while doing so.

It can also move you *backwards* in one specific way: `docs/` is embedded into the binary, so a release cut before your latest docs commit ships older documentation than your local build.

When you're working on aello, prefer `cargo install --path . --force`.

## `cargo install` fails with "Access is denied" (Windows)

A running `aello.exe` is locked, including the one in your current terminal. Rename it and reinstall; the next launch sweeps the leftovers:

```sh
mv ~/.cargo/bin/aello.exe ~/.cargo/bin/aello.exe.old-1
cargo install --path . --force
```

## The installed binary reports an older version than `Cargo.toml`

Cosmetic, and usually correct: you built locally after the last commit but before CI's automatic patch bump. `git pull --rebase` after CI finishes to sync `Cargo.toml`.

## Windows SmartScreen / macOS "cannot be opened"

The release binaries are **unsigned** — no Apple Developer account, no Windows certificate. On macOS, `install.sh` clears the quarantine attribute for you; doing it by hand is `xattr -d com.apple.quarantine <path>`. On Windows, SmartScreen's "More info → Run anyway" is the path.

## An agent ran `/sync` without being asked

It shouldn't, and there are two independent guards against it — but reading the `SKILL.md` and following its steps *is* running the skill, whichever route was taken. If the agent reports a checkpoint you didn't request, treat the report itself as suspect and check `git log` before believing a commit exists.

## `/note` was never delivered

Three usual causes, in order of likelihood:

1. **The note went to the wrong repo.** A SessionStart hook only reads its own project root. If the target env lives in a different repository, the note must be written at *that* repo's root — one left in yours looks delivered and is a dead letter.
2. **Wrong casing.** The hook matches `<Name>.NOTE.md` exactly. On a case-sensitive filesystem, `web.NOTE.md` for a blueprint named `Web` is silent.
3. **It was superseded.** Notes overwrite rather than queue — a second `/note` to the same target replaces the first.

## The handoff note is still on disk after a session started

Then the SessionStart hook didn't run, and the delivery half of `/handoff` is what's broken — not the writing half. Check that `<env>/settings.json` registers `session-start.py` **and** that the script exists at `<env>/hooks/session-start.py`. A registration pointing at a missing file fails quietly.

`aello run` re-heals both, so the fastest fix is to launch the env once.

## contextdb has no PostCompact files

Expected, and not a fault. `PostCompact` fires only when a session **compacts** — automatically, or via `/compact`. With a 1M context window and a workflow that ends sessions with `/clear`, compaction effectively never happens, so the file that hook would write is never written.

`SessionEnd` is what actually fills contextdb in that workflow: it fires on `/clear`, logout and plain exit, and archives the `/handoff` note plus a copy of the transcript.

## A contextdb record points at a transcript that isn't there

Records written before the transcript was *copied* stored only its path, and Claude Code deletes its own session files after `cleanupPeriodDays` — **30 by default**. So old references stop resolving, silently.

Two things changed: SessionEnd now copies the transcript next to the record (`<ts>_<session>_transcript.jsonl`, named in the record's `transcript_archived` field), and placement sets `cleanupPeriodDays` to 365. The retention is only filled in when the key is **absent**, so if you deliberately keep it short, that stands — and old records whose transcript is already gone cannot be recovered.

## A transcript wasn't archived (`transcript_archived` is empty)

The record kept the original path but the copy didn't happen. On Windows the usual cause is path length: Claude Code stores transcripts under `<project>/.claude-env-<name>/projects/<encoded-cwd>/`, and the encoded cwd repeats the whole project path — so a deeply nested project can push it past the 260-character `MAX_PATH` limit, which is enforced unless long paths are enabled.

aello opts out of that limit, so a current build handles it. If you see it on an older build, either move the project somewhere shallower or enable long paths (`LongPathsEnabled`). The same limit is why PowerShell can't traverse those directories either.

## An env has no memory of a previous session

Two separate things get confused here:

- **Placement seeds a starter memory only when there's no `MEMORY.md` yet** — deliberately, so a re-place never clobbers accumulated memory.
- **Print mode (`-p`) skips memory but does still fire hooks.** A headless run isn't a substitute for an interactive one when you're testing memory behaviour.

## After renaming, the old env is still there

`aello edit <old> --rename <new>` only touches the **current directory**. aello keeps no registry of where a blueprint has been placed, so any other project keeps the old name on disk — and `aello run <new>` there will quietly scaffold a fresh, empty env beside the old one rather than failing. Run the rename in each project.

## Two blueprints, one env dir

Blueprint names are compared case-insensitively at `add` time, because `.claude-env-<name>` collides case-insensitively on Windows and default macOS filesystems — two names differing only in case would share one directory and clobber each other. If `aello add` rejects a name as colliding, that's why.

## Nothing speaks

The voice has its own troubleshooting section, including the partial-copy trap where a missing sibling file silently disables desktop notifications everywhere: [voice.md](voice.md).

Quick first checks: `aello voice status` (mute state and `HOOK_VERSION`), and whether the response actually ended with a `TL;DR:` line — that line is the only thing the hook speaks.

## Plan mode is refused

Working as intended. Every env registers a `PreToolUse` hook matching `EnterPlanMode|ExitPlanMode` that denies both, so an agent asked to plan will report the tool coming back with "Plan mode is disabled in every aello environment" instead of entering it. The per-turn rules say the same thing in words, which is what also stops a numbered proposal written as ordinary prose.

To find out whether the block is the thing that fired, look for `<env>/hooks/plan-blocked.log` — a line is appended there each time it denies. **An empty or absent log is the interesting case**: it means the deny never ran and the injected text alone is carrying the rule. That half is deliberately unproven — `claude -p` never calls `ExitPlanMode` even under `--permission-mode plan`, so it could not be tested headlessly.

To undo it for one env, remove the `PreToolUse` group from `<env>/settings.json` — but note `place` heals it back on the next `aello run`, so the real change is to `src/hooks_pre_tool_use.py` and the registration in `project.rs`.

## Something else

Open an issue at <https://github.com/ryha0008-boop/aello/issues>. The useful ones state what you ran, what happened, and what you expected — and say which of the two copies you measured, the checkout or the placed env.

## `blueprint '<name>' has an unusable persona`

The binary is older than 0.2.8 and the config has already migrated.

From 0.2.8 a blueprint's persona is `coder`, `none`, `custom`, or a path. Older
binaries know only `coder`, `sysadmin` and paths, so they read `none` or
`custom` as a filename, fail to open it, and abort the launch rather than
starting an agent with no persona.

Update the binary (`aello update`, or `cargo install --path . --force` from a
checkout). Downgrading is not a fix — the config migrates forward on every load
and never back.

## A persona I generated was replaced by the stock template

The blueprint's `claude_md` is still `coder` (or a path), so aello reseeds a
template whenever the env has no `CLAUDE.md`. Accepting a persona is what stops
that:

```sh
aello persona <name> --from <file> --project <dir>
```

That writes the file, sets `claude_md = "custom"` so aello writes nothing over
it again, and records the generation in `<env>/persona.gen`. Check with
`aello list` — the blueprint should read `custom`.

Note that `place` never *overwrites* an existing persona, so this only bites an
env whose `CLAUDE.md` was deleted or never written.
