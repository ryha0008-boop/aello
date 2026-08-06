# Changelog

## [Unreleased]

### Added
- **`HOOK_VERSION` 14: turning Telegram on now reaches sessions that are
  already open.** At 13 the three variables only worked for a terminal started
  after they were set — Windows never pushes a new User-scope variable into a
  running process, so every session already on screen kept sending nothing and
  said nothing about it. 14 falls back to the persisted `HKCU\Environment`
  value when a name is **absent** from the process environment, so no relaunch
  is needed. It only fires on absent, never on present: `REVOICED_TELEGRAM=0`
  in a blueprint still wins, and so does an explicitly empty value, so a
  per-project opt-out survives a machine-wide default. `speak.py --status` now
  names the source (`set`, `set at User scope, picked up from there`, or
  `not set`), which is also the only way to tell 13 from 14 — and only from a
  shell that never inherited the variables, since a fresh one cannot
  distinguish them.

- **The voice hook is re-vendored at `HOOK_VERSION` 13, which adds a Telegram
  sender.** Upstream now delivers the spoken `TL;DR:` line and its mp3 to a
  Telegram chat alongside the local playback, so a response reaches you when
  you are away from the machine. It is opt-in and off unless three environment
  variables are set: `REVOICED_TELEGRAM=1`, `TELEGRAM_BOT_TOKEN` and
  `TELEGRAM_CHAT_ID`. Nothing changes for an env that sets none of them.

  Only `speak.py` moved between 11 and 13 — `duck.py`, `focus.py`, `notify.py`
  and `win_audio.ps1` are byte-identical — but all five are re-vendored as a
  unit anyway, because the guard that makes a partial copy survive is also what
  makes it silent. Unlike the 8 → 10 → 11 sequence, a copy left on 11 is not
  damaging, just deaf to Telegram: the audio-session bug was fixed at 11 and
  nothing here touches `duck.py`. `aello voice status` reports the vendored
  version, and a placed copy answers `python <env>/hooks/speak.py
  --hook-version` before any optional import, so a partial copy still tells you
  the truth.

- **Every response now ends with one block: the `TL;DR:` line, with 3–4
  numbered next steps beneath it.** The per-turn `UserPromptSubmit` hook's last
  rule now fixes the whole shape of an answer: a few sentences of prose, then
  one closing block — a summary of two to four sentences with the actions you
  take next directly under it, and nothing after those. A long reply can be
  skipped entirely and still acted on, which is the point, since a wall of
  findings is not an instruction. The steps are dropped when nothing is waiting
  on you; the `TL;DR:` line never is. ~300 tokens a turn, up from ~190.

  Summary and steps shipped as two separate rules and lasted about an hour:
  the summary said a thing, the steps repeated it, and you had to reconcile two
  closing sections. Merged, the spoken line introduces the list it sits on top
  of, and it grew from two sentences to two-to-four to absorb what the steps
  used to duplicate. The ordering is not taste — `speak.py` matches the last
  `TL;DR:` line and reads to end of line, so the summary must stay on one line
  (a wrapped one is spoken with its tail silently cut off) while numbered steps
  beneath it match nothing. Verified against the real `extract_tldr`.

  The steps have to **stand alone**, and saying so is load-bearing: the first
  wording produced correct steps underneath an essay that still had to be read
  to make sense of them — the exact failure the rule exists to prevent. It now
  states the assumption outright (the user skips every word above the steps, so
  no step may say "as described above") and the concise rule caps the prose at a
  few sentences, with anything that matters moved into a step rather than a
  paragraph. The two only work as a pair.

  The no-plans rule needed rewording to sit beside it: it used to ban "numbered
  proposals", which is exactly what the new rule asks for. It now bans laying
  out what the agent intends to do and waiting for sign-off. The distinction is
  whose hands the actions are in — a plan is the agent's next moves held for
  approval, the steps are yours.

### Fixed
- **The ducking fix at 10 was incomplete, and 10 is still damaging.** The
  vendored voice hooks move to `HOOK_VERSION` 11. Upstream keyed its record of
  the pre-duck volume on the process id, and **one process owns several audio
  sessions** — a browser runs a media stream and a notification stream under one
  pid, and `GetAllSessions()` yields a control for each. The second reading
  overwrote the first while both were being lowered, so the restore raised one
  and left the other down with its record already deleted: the same 1.0 → 0.15 →
  0.0225 → floor ratchet as at 8, just needing two sessions to trigger. Measured
  on this machine *after* the 41-env re-vendor at 10: five applications at
  exactly 0.15 from one duck. Records are keyed on `InstanceIdentifier` now,
  with the pid kept for the liveness check, and version 11 still honours the
  pid-keyed records that copies on 10 left behind rather than stranding the
  applications they describe. `restore()` also no longer runs unlocked when the
  lock is merely contended, which let the station's poll delete a record while a
  worker was still inside the lowering loop.

  Also at 11: `import duck` in `speak.py` is guarded like `focus` and `notify`,
  so a partial vendor reports through `MISSING` instead of raising at module
  scope — that failure killed `--hook-version` and `--status` too, leaving an
  env silent with every diagnostic built for it dead as well. The lock file
  carries an owner token (a holder that was legitimately stolen from used to
  delete its successor's live lock), `state.tmp` is per-process, and a
  `state.json` that cannot be parsed is never written over.

- **`aello voice` no longer erases the voice pool when the state file is
  corrupt.** `read_state` treated an unparseable `state.json` exactly like a
  missing one — empty defaults — and the next `mute`/`unmute`/`M` wrote those
  back, taking every preset and lease with them. Absent still reads as empty;
  present-but-unparseable now fails with the path and leaves the bytes alone for
  the hook to recover from. Found by reading upstream's fix for the same bug on
  the Python side, since both processes write this one file.

- **Ducked audio comes back up.** The vendored voice hooks move from
  `HOOK_VERSION` 8 to 10, which carries an upstream fix for a bug that
  permanently lowered the machine's per-application volumes. `duck.py` deleted
  its record of the original volumes whenever it could not enumerate audio
  sessions — which was every poll from a second thread, since comtypes
  initialises COM per thread — so the next duck read an already-lowered volume
  as normal and lowered it again. Windows keeps per-application volume in the
  registry against the executable's path, so this outlived the session, the
  process and the reboot: applications ended up at 0.15, then 0.0225, then the
  0.01 floor, with nothing on disk recording what normal had been.

  Upstream also takes a lock on the record, writes it *before* lowering
  anything, and keeps entries it could not restore instead of dropping the whole
  store. Measured here before the re-vendor: one application sitting at 0.15
  with no `duck.json` beside it. Existing envs adopt the fix on their next
  `aello run` — until then their copy is still the one doing the damage.

  Also in the bump: a `spoken` lifetime counter that revoiced keeps in the
  shared `state.json` (aello's keys are untouched), and a `REVOICED_HISTORY`
  default of 1000 rather than 200, so the shared data directory grows to roughly
  92 MB of audio instead of 18.5 MB.

### Added
- **No plans, anywhere.** The per-turn `UserPromptSubmit` hook gains a fourth
  rule — never present a plan for approval, never use plan mode; ask a short
  question or do the work, and where the choice is genuinely yours, offer
  concrete options to pick from. A plan handed over for sign-off goes unread,
  and the round trip buys nothing a question wouldn't.

  It ships with an enforcing half: a bundled `PreToolUse` hook matching
  `EnterPlanMode|ExitPlanMode` that denies both, so plan mode is unavailable
  rather than discouraged. Neither half is redundant — the hook stops the tool,
  and only the injected text stops a numbered proposal written as ordinary
  prose, which is what a plan usually looks like. The matcher is load-bearing:
  an unmatched `PreToolUse` group runs on *every* tool call, a Python spawn per
  `Read`.

  Verified as far as it can be, and no further: a `PreToolUse` deny does block a
  tool and return its reason (measured against `Read`), and the matcher does
  scope it (a `Glob` in the same run never reached the hook). Whether the two
  plan tools emit a `PreToolUse` event at all is **unproven** — `claude -p`
  never calls `ExitPlanMode` under `--permission-mode plan`, so print mode
  cannot answer it. A denial therefore appends to `plan-blocked.log` beside the
  script; the first line to appear settles it, and an empty log across envs that
  have wanted to plan means the text is doing all the work.

  Both reach existing envs on their next `run`, like the other hooks.

### Added
- **`aello persona <name> --from <file>`** — installs an agreed global persona
  into a placed env: replaces its `CLAUDE.md`, sets `claude_md = "custom"` so
  aello stops seeding a template over it, and bumps the generation recorded in
  `<env>/persona.gen` (`gen1 2026-08-03`). The only command that overwrites a
  persona; `run` still never does.

### Changed
- **The persona choice is now three values: `coder`, `none`, `custom`.** A
  coding project starts on `coder`, anything else on `none`, and both become
  `custom` the first time a generated persona is accepted — after which the
  env's own `CLAUDE.md` is authoritative. Existing configs migrate on load: a
  missing `claude_md` becomes an explicit `none`, so a blank persona reads as a
  decision rather than an oversight.

  A **path** is still accepted for the blueprints that point at a persona file
  you maintain, including the ones sharing a single file between several envs.

### Removed
- **The `sysadmin` persona template.** Barely used, and close enough to `coder`
  to not earn its own slot. The one blueprint on it migrates to `custom`: its
  env already holds the text, since aello never overwrites a persona, so nothing
  is lost — and calling it `coder` would have claimed text that env doesn't have.

### Added
- **Every env now carries three response rules, injected on every prompt.** A
  bundled `UserPromptSubmit` hook asks for concise answers (no preamble, filler
  or hedging), rules out sycophancy (no opening praise or agreement, no
  validating an unchecked premise, no softening a finding — say plainly when the
  user is wrong, and say "I don't know" when that's the answer), and requires the
  trailing `TL;DR:` line the voice speaks. ~150 tokens per turn.

  Per turn rather than per session because style decays: an instruction given at
  turn one is buried by turn eighty. On a hook rather than in the persona because
  the persona is written once and never clobbered, so editing it would reach no
  existing env — while `place()` rewrites the hook script every run, so all
  existing envs adopt this on their next launch with no backfill.

  The TL;DR instruction moves here from the persona as a result. Envs already
  carrying that section keep it (the persona is yours; aello won't edit it), so
  they simply have it in both places. Unregister the hook by hand and the persona
  append comes back, so the voice can't go silent for want of an instruction.

### Removed
- **`AUDIT-2026-07-08.md` is no longer in the repo.** Audits are working notes
  and were ruled out of git, and `AUDIT-*.md` was added to `.gitignore` — but the
  file was already tracked, and `.gitignore` has no effect on a tracked file, so
  it stayed published the whole time. Untracked now (the local copy is kept and
  archived outside the repo).
- **The dead `skills/sync/SKILL.md` at the repo root**, a hand-written `/sync`
  from before `templates::render_sync_skill` existed. Nothing has read it since;
  the generated skill is per blueprint and lives in the env dir.

### Fixed
- Stale `--github` / `--project-md` / "capability checklist" references in
  `CONTRIBUTING.md`, `docs/concepts.md`, `scripts/demo-recording.md`, the PR
  template and two source comments — all describing flags removed in 0.2.0.
- The release process in `CLAUDE.md` and `docs/development.md` still said the
  bump job always adds a patch. It hasn't since the change that made `v0.2.0`
  possible: a version with no tag yet is published exactly as written.

### Added
- **Every env now launches with `--thinking-display summarized`**, so each turn's
  reasoning is captured as a readable summary instead of an empty block. The
  API's default is `omitted`: thinking blocks arrive with empty text and only an
  opaque signature, which meant contextdb archived a full record of what was done
  and nothing of what was thought — 2,842 thinking blocks across 53 transcripts,
  every one empty. `display` controls visibility only; thinking happens and is
  billed identically, so this costs nothing. The raw chain of thought is never
  returned by any model — a summary is the ceiling. Override for one run with
  `aello run <name> -- --thinking-display omitted`.
- The README links the live docs site and carries a docs badge.
- **`docs/upgrading.md`** (`aello docs upgrading`) — the page to point an
  environment at the first time it meets 0.2: what migrates itself, the one case
  that changes behaviour, the removed flags, and the pre-0.2-binary hazard.
- **The SessionStart hook now opens every session by saying it is running under
  aello** — which blueprint, that the env dir is rewritten on every run
  (`.aello-keep` to pin a skill), that the seeded skills are the user's to type,
  and that commits are attributed automatically. Nothing else told a session any
  of this: the env dir is gitignored, the persona belongs to the user and usually
  doesn't mention aello, and a project `CLAUDE.md` only exists for a maintainer —
  so agents edited files the next launch silently overwrote. It lives on the hook
  rather than in the persona because `place()` rewrites the hook every run, so it
  reaches an env placed months ago without touching anything the user owns.
  ~257 tokens per session.

### Fixed
- **contextdb archived transcripts by *path*, and the path stopped resolving.**
  Claude Code deletes its own session files after `cleanupPeriodDays` (default
  **30**), and the env dir holding them is gitignored and removed by
  `aello remove --purge`. An audit on 2026-08-03 found **15% of 265 archives
  already pointed at nothing**, with a clean cliff at the 30-day mark — 6–14%
  dead under 30 days, 44% at 30–39. Nothing ever errored; the archive quietly
  stopped being one.

  SessionEnd now **copies** the transcript to `<ts>_<session>_transcript.jsonl`
  beside the record and reports the result in a new `transcript_archived` field,
  and placement sets `cleanupPeriodDays` to 365 — self-healed into existing envs
  only when the key is absent, so a value you chose is left alone. Expect
  contextdb to grow: transcripts are 1.3 MB at the median and tens of MB at the
  tail.

  The copy also opts out of Windows' 260-character `MAX_PATH` (`\?\` prefix).
  The encoded-cwd directory repeats the whole project path, so a deep project
  pushes the transcript past the limit and the copy fails — measured at 325
  chars, where the archive quietly degraded back to a pointer.

  The same audit confirmed **PostCompact is dormant, not broken** — it fires only
  on compaction, which a 1M-context session ended with `/clear` never reaches
  (265 SessionEnd records to zero PostCompact ones for any aello blueprint). It
  stays seeded for workflows that do compact.

## [0.2.0]

First stable line. The blueprint interface is settled: a blueprint is a name, a
model, a persona and a **role**, and that shape is what the rest of the tool is
built on. Existing configs migrate themselves — see below.

### Changed
- **The five capability flags are now three roles.** `maintainer` (owns the
  project `CLAUDE.md`, `CHANGELOG.md`, `docs/`, `README.md` and git),
  `contributor` (commits, pushes, and logs its own change in `CHANGELOG.md` —
  never touches the other three) and `standalone` (no `/sync` at all). One flag
  replaces ten: `aello add --role <role>` and `aello edit --role <role>`;
  `--project-md` / `--github` / `--changelog` / `--docs` / `--readme` and their
  `--no-` counterparts are **removed**, and the TUI's capability checklist is a
  role picker. `aello list`'s last column is now `ROLE`.

  The flags were never independent in practice. Grouped **by repo** — the unit
  that matters — every multi-blueprint project has exactly one blueprint holding
  everything and the rest holding only git duties; 32 of the 32 combinations
  nobody used were surface area with no behaviour behind it. Grouped fleet-wide
  they look all-or-nothing instead, which is the reading that nearly got the
  distinction deleted altogether: it is the whole multi-agent point and it is now
  the middle role rather than an accident of five checkboxes.

  **Existing configs migrate themselves** on the next load, with nothing to run:
  anything maintaining prose (`project_md`, `docs`, `readme`) becomes a
  maintainer, anything left holding only git becomes a contributor, nothing
  enabled becomes standalone. The old `[blueprints.caps]` table is read once and
  dropped the next time aello saves. The one behaviour change: a blueprint that
  had *some but not all* of the prose capabilities is now a maintainer, so
  `/sync` covers the files it previously skipped — `aello edit <name> --role
  contributor` if that isn't what you want.
- `docs/capabilities.md` is now `docs/roles.md` (`aello docs roles`).

### Added
- **A documentation site.** `/docs` on the landing page is generated from this
  repo's `docs/` directory at build time — the same files the binary embeds — so
  there is no second copy to drift. Sidebar of pages, per-page contents, and
  `*.md` cross-links rewritten to routes. Deployed to GitHub Pages by
  `.github/workflows/pages.yml` on any push to `main` that touches `site/**` or
  `docs/**`. The landing page gains a Workflows section linking into it, and its
  capability cards are now role cards.
- **Four new reference docs**, bundled into the binary like the rest — readable
  with `aello docs <name>`, in the TUI reader (`?`), and on the docs site:
  - `workflows.md` — task-shaped walkthroughs: your first env, two agents in one
    repo, the work → `/sync` → `/handoff` → `/clear` loop, resuming, updating an
    already-placed env, renaming, GitHub setup, removal. Ends with the
    conventions for adding another one.
  - `skills.md` — the four seeded skills in detail: why they're manual-only
    twice over, why they're regenerated on every run, and `.aello-keep`.
  - `development.md` — working on aello itself: the test-the-deployed-copy rule,
    re-vendoring the voice hook, the release pipeline and its sharp edges.
  - `troubleshooting.md` — failure modes and what they actually mean, starting
    with where everything lives on disk.
- **Voice hook re-vendored at `HOOK_VERSION = 8` — history now records what you
  asked, not just what was answered.** Upstream `revoiced` reads it from the
  transcript the hook already opens to find the `TL;DR:` line, so there is no new
  hook event, no new file, and no `settings.json` change. Two upstream env vars,
  both defaulting to on: `REVOICED_PROMPTS=0` disables capture outright and
  `REVOICED_PROMPT_MAX` (4000) truncates a pasted log; aello sets neither and
  passes the upstream defaults through. Existing envs pick it up on their next
  `run`, and `python <env>/hooks/speak.py --hook-version` reports it — printing
  before any optional import, so even a partial copy answers.

  It landed across versions 6, 7 and 8 in one evening, both later bumps fixing
  bugs found from this side by running upstream's own `user_prompts()` against a
  live transcript **from a vendored copy** rather than the revoiced checkout: 7
  stopped `<command-args>` being discarded as wrapper noise (a slash command with
  arguments recorded as a bare `/note`, and `/loop 5m /foo` as `/loop`), and 8
  stopped an edited-then-sent message being recorded twice (the transcript holds
  both the short draft and the long final, and the dedup only caught exact
  repeats, not prefixes). Only `speak.py` moved in all three.

### Fixed
- **The README is for users again, and `docs/` is for developers.** The README had
  filled up with detail that only makes sense once you've read the source — the
  voice section alone ran to Windows toast-identity registration, per-OS state
  directories and five-file vendoring, most of it a near-duplicate of a `docs/`
  page that covered the same ground better. That section is now what a user needs
  (it speaks, here's how to silence it, here's what to install), Concepts lost the
  transcript path templates and hook internals, and the mechanism moved to a new
  **`docs/voice.md`** — its own page, since the voice explicitly isn't a
  capability and was sitting inside `capabilities.md` anyway. New pages under
  `docs/` need no code change: they appear in `aello docs` and the TUI reader on
  their own.
- **The seeded skills now say they are yours to run, not the agent's.**
  `disable-model-invocation: true` stops the model calling one as a *tool*, but
  nothing stopped an agent opening the `SKILL.md` with `Read` and carrying out
  the steps by hand — which produces the worse outcome, because you believe a
  checkpoint ran when it did not. Every generated skill now opens with a banner
  saying that following the instructions **is** running the skill, whichever
  route you took to them. The frontmatter closes the tool path; the banner closes
  the reading path.
- **`place` no longer re-adds the TL;DR section to a persona that has moved it
  elsewhere.** The instruction that makes the voice work lived in the global
  persona — the one file most likely to be rewritten wholesale. An env can now
  carry it on a `UserPromptSubmit` hook instead (registered by hand in that env's
  `settings.json`, injected on every turn), and `place` leaves the persona alone
  when it sees that hook. Without it the append is unchanged, so nothing moves
  for an env that has not opted in.
- **`/sync` no longer tells you to commit the transient env files.** Its staging
  rule is "stage only what you created or modified this session" — and a
  `<blueprint>.HANDOFF.md` written by `/handoff` is exactly that, so the rule as
  written swept in a file that is untracked on purpose and deleted on the next
  boot. `/sync` now excludes `*.HANDOFF.md` and `*.NOTE.md` explicitly, and
  `/handoff` says so on its own side too.
- **`/note` handles a target env in another repo.** It assumed the target shared
  this repo: it sanity-checked `.claude-env-<target>` at *this* project root,
  reported a missing one as a probable typo, and then wrote the note here anyway
  — where the target's SessionStart hook, which only reads its own project root,
  would never see it. It now resolves the target's project root first and writes
  there, treats "cannot locate it" as the only typo case, and reports the full
  path it wrote. It also asks for the blueprint's canonical casing (the hook
  matches `<Name>.NOTE.md` exactly, so wrong casing is a silent dead letter on a
  case-sensitive filesystem), and no longer says "append a note" one paragraph
  above the step that says to overwrite.
- **`/sync` stopped guessing the memory directory.** It named
  `projects/<this-project>/memory/`, inviting a hand-built path; Claude encodes
  the cwd by folding every non-alphanumeric character to `-`, so a guess creates
  a second empty memory dir nothing reads. It now says to list
  `$CLAUDE_CONFIG_DIR/projects/` and use what is there.
- **`/sync`'s pre-push rebase no longer cites a CI that may not exist.** The
  reason given was aello's own `release: vX [skip ci]` auto-bump, which only runs
  if the scaffolded `.github/` was actually committed. The advice is right for
  any repo touched from more than one place; the rationale now says that instead.

### Added
- **`.aello-keep` — pin a hand-edited skill.** The four seeded skills (`/sync`,
  `/handoff`, `/note`, `/twosentences`) are rewritten on every run, so a project
  that needed a different workflow — a `/sync` that also deploys, say — silently
  lost its version on the next `aello run`, and the `claude-internal/` mirror
  then committed the generated one over it. An empty `.aello-keep` beside a
  skill (`.claude-env-<name>/skills/sync/.aello-keep`) makes `place` leave it
  alone: not regenerated, and not deleted if the blueprint later drops all its
  capabilities. Per env dir, so the same blueprint used in another project still
  gets the generated skill; every other file in the env keeps self-healing.
- **`--voice` capability — spoken responses.** A blueprint with `--voice` gets a
  `Stop` hook that reads each response's trailing `TL;DR:` line aloud through a
  free Edge neural voice, and a `SessionEnd` hook that returns the voice it
  leased. The hook (and its two helper files) is copied **into the env** and
  registered as `$CLAUDE_CONFIG_DIR/hooks/speak.py`, so it never points at a
  checkout elsewhere on disk — a newly placed env speaks with no hand-editing,
  and moving an unrelated directory can't silence it. The persona gains a section
  asking for the TL;DR line the hook speaks (appended, never clobbered, so
  enabling voice on an existing env works). State — voice pool, per-session
  leases, mutes — is machine-wide, so concurrent envs each get a different voice
  and queue behind one playback lock instead of talking over each other.
  Available as `--voice` / `--no-voice` on `add` and `edit`, in the `init`
  wizard, and as a row in the TUI capability checklist. Enabling it on an env
  whose `Stop` hook was wired up by hand against a checkout **replaces** that
  entry rather than adding beside it, so migrating doesn't speak every response
  twice; unrelated hooks are left alone.
- **`aello voice`** — the off switch: `mute` (optionally `--project`), `unmute`,
  `stop` (cut off the current sentence), and `status`. It writes the shared state
  directly, so it needs neither Python nor a placed env and works from any
  directory — which is exactly where you are when a machine you didn't expect to
  talk starts talking.
- **Prebuilt macOS binaries** — `aello-aarch64-macos` (Apple Silicon) and
  `aello-x86_64-macos` (Intel) now ship with every release, so macOS installs
  with the same one-liner as Linux and `aello update` works there. The binaries
  are unsigned; the installer strips the quarantine attribute for you, and the
  README documents the manual `xattr -d com.apple.quarantine` step.
- **`M` mutes the voice from the TUI** — the same machine-wide switch as
  `aello voice mute`, on a single key, so you don't have to leave the TUI (or
  find a script path) when a machine you didn't expect to talk starts talking.
  It silences every env and cuts off the sentence already playing; while muted
  the footer reads `VOICE: MUTED`, so a quiet machine isn't mistaken for a
  broken hook.
- **A landing page**, in `site/` — a static Next.js build covering the install
  one-liner, the three-command quick start, the capability table, and the
  universal skills. Nothing in the CLI depends on it; `npm run build` in `site/`
  emits plain HTML in `site/out/` for any static host.

### Changed
- **Every env speaks — the voice is no longer a capability.** `--voice` and
  `--no-voice` are gone from `add` and `edit`, the row is gone from the TUI
  checklist and the `init` wizard, and every placed env now gets the speak hooks
  and the TL;DR persona section unconditionally. Choosing it per blueprint bought
  nothing, and the moment you actually want silence — a machine that has started
  talking — is a runtime decision: `aello voice mute` (or `M` in the TUI) covers
  every env at once and is reversible in one keystroke. Existing envs pick the
  hooks up on their next `aello run`, and a `voice = …` left in your config is
  ignored on load and dropped on the next save.
- **The voice hooks are re-vendored from upstream `revoiced`** (`a86023a`). A
  voice-enabled env now gets **its own voice per environment** rather than per
  working directory: several blueprints sharing one repo no longer sound alike,
  and an agent that works in a subfolder stays the same speaker instead of
  becoming a second project. Identity is read from `CLAUDE_CONFIG_DIR`, which
  aello already exports, and falls back to the old directory-keyed behaviour when
  it is absent. Also picked up: em dashes in a `TL;DR:` are no longer spoken as
  mojibake on Windows, and a mute set while the hook was mid-write is no longer
  silently dropped. Still three files — upstream's optional `focus`/`notify`
  siblings are station-side and deliberately not vendored.
- Releases are now **versioned**. Every build publishes an immutable `vX.Y.Z`
  release (binaries + `SHA256SUMS`) alongside the existing rolling `latest` tag,
  so you can pin or roll back to a specific version and package managers have a
  stable URL to point at. `install.sh` and already-installed binaries keep using
  the rolling tag exactly as before — nothing to change on your side.
- `aello remove <name>` now **prompts for confirmation** before deleting a
  blueprint (`--yes` skips the prompt). A new `--purge` flag also deletes the
  placed `.claude-env-<name>/` env dir and its `claude-internal/<name>/` mirror
  in the current project; without it the on-disk dirs are left as-is and you're
  told they remain.
- The `/handoff` note is now written to `<blueprint>.HANDOFF.md` (prefixed with
  the blueprint name) instead of a shared `HANDOFF.md`, so multiple blueprints in
  one repo each keep their own handoff without overwriting each other. The
  SessionEnd hook archives the matching per-blueprint file.

### Fixed
- **Desktop notifications now work on a machine that has never run the revoiced
  station.** Windows drops a toast sent under an unregistered AppUserModelID with
  no error at all, and only the station registered one — so on such a machine
  every env was silently notification-less, the same failure as the incomplete
  vendor one layer out. Launching an env now runs the vendored
  `notify.py --register` (no test toast, idempotent). At `HOOK_VERSION = 5` that
  claims the toast identity and leaves the `revoiced:` protocol to a copy with an
  `action.py` to serve it — an env has none, so it can't take a working toast
  button away from the station.
- **"Skip this one" now skips that one.** A toast stays in Windows' notification
  centre for three days, and the skip link named no turn — so pressing the button
  on an old notification cut off whatever happened to be speaking at that moment,
  which across dozens of envs is usually another env's line. Re-vendored the voice
  hook at `HOOK_VERSION = 4`: the URI carries the turn id and the station refuses
  a stale one. Vendored at `HOOK_VERSION = 5`; existing envs pick it up on their
  next `run`. The drift check now
  covers **all five** vendored files rather than only `speak.py` — `HOOK_VERSION`
  lives in `speak.py` alone, so a re-vendor touching just `duck.py` or
  `win_audio.ps1` used to slip past it.
- **Desktop notifications work again — the voice hook was vendored incomplete.**
  `speak.py` imports four siblings; aello copied two of them. The other two,
  `focus.py` and `notify.py`, are imported behind a `try`/`ImportError` guard, so
  nothing failed: every env spoke normally while `notify`'s stub reported "not
  shown" and **no desktop notification was ever raised, in any env**, with
  nothing anywhere saying so. Placement now writes all five files
  (`speak.py`, `duck.py`, `focus.py`, `notify.py`, `win_audio.ps1`), so any
  existing env picks them up on its next `run`. `focus.py` also restores the
  window tracking that lease reaping and the toast's **Go to terminal** button
  depend on.
- **A vendored copy that has fallen behind is now visible.** `speak.py` carries a
  `HOOK_VERSION`, bumped upstream whenever one of the five hook-path files
  changes; aello records the version it vendored, a unit test fails if a
  re-vendor moves one without the other, and `aello voice status` prints it
  alongside the mute state. `python <env>/hooks/speak.py --status` prints what
  that env actually runs, so the two can be compared. The upstream re-vendor also
  brings a hard timeout on a stuck player (one wedged process used to hold the
  machine-wide speaker lock and silence every env) and a `win_audio.ps1` that no
  longer drops a line when the audio duration resolves slowly.
- **`aello update` no longer re-downloads and reinstalls a version you already
  have.** It never compared versions, so every run pulled the whole multi-MB
  asset and rewrote the running binary — which on Windows means renaming the live
  exe aside, and hard-fails when a second aello is running. `--force` reinstalls
  deliberately. A failed release-API fetch is now an error instead of a silent
  success, so `aello update && …` can't carry on as though it were current; all
  three network calls have timeouts (a stalled server used to hang it forever);
  and the Windows replace stages the new binary before moving the old one aside,
  so a failed write can't leave a truncated exe at the install path.
- **A blueprint whose persona file has moved or been deleted now fails on `run`
  instead of launching without it.** `add` and `edit` both reject an unusable
  persona, but `run` warned to stderr — moments before Claude's alternate screen
  wiped it — and carried on with no persona at all.
- **`--model " opus "` no longer reaches `settings.json` verbatim.** The value was
  validated as trimmed and lowercased, then the raw string was stored.
- Over-long blueprint names are rejected with a clear message instead of failing
  later inside `create_dir_all` with a raw OS error.
- `aello init` re-reads the config after its prompts, so a token captured by an
  `aello login` in another terminal while you were answering is no longer
  discarded.
- **A `Stop` hook of your own whose command merely contains `speak.py`** (say
  `tools/my_speak.py`) is no longer deleted on every `aello run` — the match is
  anchored on a path boundary now. It took any unrelated command in the same
  group with it.
- **`aello voice mute --project` no longer cuts off another project's audio.** The
  stop token is machine-wide; only a global mute should use it.
- Concurrent writes to the shared voice state can no longer corrupt it: aello and
  the hook were staging through the same fixed temp filename.
- **The TUI docs reader (`?`) can reach the end of a page.** The scroll cap
  under-counted wrapped rows, making the last lines of longer docs unreachable;
  `Home`/`End` (and `g`) now jump to either end.
- The TUI no longer leaves your terminal in raw mode on the alternate screen if
  it fails to start after switching screens.
- The TUI reports when a delete leaves the env dir and `claude-internal/` mirror
  behind, when an edit's target vanished mid-flow (it used to claim success), and
  why creating a contextdb folder failed instead of silently closing the box.
- Editing a blueprint in the TUI no longer discards a full `claude-*` model id or
  a custom persona path just because you scrolled the picker and came back.
- `aello edit --rename` now says that only the current project was updated, since
  a blueprint placed elsewhere keeps its old env dir there.
- `aello login` distinguishes `claude setup-token` failing from its output being
  unparseable, instead of inviting you to paste a token that was never issued.
- **`claude` installed via npm is found on Windows.** `claude.cmd` is a shim, and
  only `claude.exe` was ever looked for, so a working install reported "claude is
  not installed".
- The transcript hooks no longer crash with a traceback when the contextdb
  directory can't be created.
- **The `/handoff` note is now actually read on boot, and then deleted.** The
  skill has always told the agent the note is "read on boot, then deleted" and
  stamped a banner saying so, but nothing ever read it — there was no
  `SessionStart` hook, so the file sat at the project root dirtying `git status`
  and being re-archived verbatim by every later session. A new `SessionStart`
  hook delivers it (and `<name>.NOTE.md`, the cross-env inbox from `/note`) into
  the session, then removes the file. Deleting is safe because `SessionEnd`
  already archived the content to contextdb.
- **`aello edit <name> --rename` accepts a case-only change** (`coder` →
  `Coder`). On Windows and macOS the destination "already exists" because on a
  case-insensitive filesystem it *is* the source, so the rename bailed while
  naming the source as the obstruction — the feature was unreachable on both
  platforms aello ships binaries for. The case-flip is now routed through a temp
  name.
- **`--rename` carries the `<name>.HANDOFF.md` and `<name>.NOTE.md` files with
  it.** Both are addressed by blueprint name and both consumers key off the new
  one, so a pending resume note or cross-env inbox was left orphaned under a name
  nothing would ever look for again. An existing file at the destination is never
  clobbered.
- **The tracked `claude-internal/<name>/` mirror is cleared when the `github`
  capability is dropped.** It was only ever written from inside the `github`
  branch, so turning the cap off froze the folder in git forever — still carrying
  a git-flavoured `/sync` skill the blueprint no longer had — with
  `aello remove --purge` the only way to clear it. Another blueprint's mirror in
  the same repo is left untouched.
- A hook event whose value in `settings.json` isn't an array is now left alone
  rather than overwritten, matching every other self-heal, and a `settings.json`
  that doesn't parse as JSON now says so on stderr instead of skipping in silence.
- **`aello edit <name> --model` now actually reaches an env that has already been
  placed.** `settings.json` is the only thing that tells Claude Code which model
  to use, and it was written only when it didn't already exist — so editing the
  model updated `config.toml`, `.aello.toml` and `aello list` while the env kept
  running the model it was first placed with, indefinitely, and `edit` still
  reported "Changes apply on the next `aello run`". The model is now merged into
  the existing file on placement, key by key, so hand-added settings like
  `effortLevel` and `enabledPlugins` survive untouched.
- **`Ctrl`+letter no longer triggers the plain-letter command in the TUI.** Key
  presses were matched on the letter alone, ignoring modifiers, so **`Ctrl+U`
  started an unprompted self-update** that replaced the running binary,
  `Ctrl+S` silently wrote the contextdb path to disk, and `Ctrl+D` opened the
  delete modal. `Ctrl+C` now quits from every mode instead of doing whatever the
  bare letter did, and a chord typed into a name field is ignored rather than
  inserting the letter.
- **Uppercase keys work.** The footer advertises `[F] [S] [A] …` and the delete
  modal says `[Y] CONFIRM`, but only lowercase was bound — so pressing Shift+Y as
  instructed, or using any command with Caps Lock on, did nothing at all and gave
  no feedback. Command keys are now case-insensitive. Text entry still keeps its
  case, so capitalised blueprint and folder names are unaffected.
- The SessionEnd self-heal no longer skips envs that already have a **third-party
  `SessionEnd` hook**. It bailed whenever the `SessionEnd` key existed at all, so
  adding any hook of your own permanently blocked aello from installing its own
  transcript-archiving hook — envs placed before the feature existed would
  silently never gain it. The check is now keyed on aello's own command, and its
  hook group is appended alongside yours.

### Docs
- `docs/capabilities.md` gains a "when it doesn't speak" section for `--voice`:
  check `aello voice status` for a mute, then read the hook's `history.jsonl` —
  a real voice name means synthesis worked, `system fallback voice` means
  `edge-tts` wasn't found, and no entry at all means the hook never ran or the
  response had no `TL;DR:` line.
- `docs/concepts.md` now warns that a project-level `<project>/.claude/settings.json`
  is **silently ignored** under aello (`CLAUDE_CONFIG_DIR` points at the env dir),
  and says to use `<project>/.claude-env-<name>/settings.json` instead.
- Added a dedicated **macOS (build from source)** install section to the README
  (`cargo install --git …`, Rust-toolchain prerequisite, and the caveat that
  `aello update` only ships Linux/Windows binaries so macOS updates by rebuilding).

### Security
- `aello update` now verifies the downloaded binary's **SHA-256** against a
  `SHA256SUMS` asset published with the release before installing it, so a
  hijacked release asset or a TLS-intercepted download can't silently replace
  your binary. Releases without the manifest still update (the check is skipped
  with a note). The download is also now size-capped (128 MiB) so a malicious
  endpoint can't stream unbounded data and OOM the process.
- A hand-edited blueprint name in `config.toml` is now re-validated before it's
  used to build filesystem paths. `validate_name` only ran on config *writes*;
  read paths interpolated the stored name straight into `.claude-env-<name>` /
  `claude-internal/<name>`, so a name like `../../evil` could escape the project
  dir when placing (or, under `remove --purge`, deleting). `run` and
  `remove --purge` now re-gate the name at the sink.
- `aello login` / `aello init` no longer echo the token line to their own
  stdout. The capture loop tees `claude setup-token`'s output so the auth URL
  shows on a headless box, but it now redacts the line carrying the `sk-ant-…`
  token — previously running login under any stdout capture (`| tee`, a CI job
  log, `script`, tmux) persisted the long-lived token in cleartext in that log.
- On Unix, `config.toml` (which holds the plaintext, non-rotating OAuth token)
  and its directory are now written owner-only (`0600`/`0700`) instead of the
  default world-readable `0644`, so another local user or a low-privilege
  process can no longer read the token. No-op on Windows.

### Fixed
- The TUI now restores your terminal (raw mode, alternate screen, cursor) if it
  panics, instead of leaving the shell unusable, and undoes raw mode if it can't
  enter the alternate screen. The session-resume list also no longer risks a
  panic truncating a non-ASCII session id (it now truncates by character), and
  the in-app docs reader's scroll uses saturating arithmetic. A stale `/sync`
  skill that can't be removed when a blueprint drops all capabilities now
  surfaces the error instead of being silently re-committed.
- The `github` cap no longer appends a near-duplicate `.gitignore` line when a
  `.claude-env-*/` (trailing-slash) entry already exists. And aello no longer
  alphabetically reorders the keys in Claude-owned JSON (`.claude.json`,
  `settings.json`) when it reads and rewrites them, keeping git diffs clean.
- `aello edit --rename` is now transactional. It previously renamed the env dir
  first and only then checked whether the `claude-internal/<new>/` mirror
  collided — so a collision (or any fs error) left the env dir already moved but
  the config not saved, and `run <old>` re-scaffolded a fresh env, orphaning the
  renamed one. Both destinations are now pre-checked before any move, and a
  failed mirror move rolls the env-dir move back.
- **Editing a blueprint in the TUI (`E`) no longer downgrades its model or
  drops a custom persona.** The curated pickers can't represent a full
  `claude-*` model id, the `default` alias, or a custom persona path, so opening
  a CLI-configured blueprint and saving — even just to toggle one capability —
  used to rewrite its model to `opus` and its persona to `none`. The edit flow
  now preserves the original model/persona unless you actually change that
  picker, and shows a `KEEPING = …` hint when the stored value is off-list.
- **Config/token loss on a transient read error is prevented.** `config::load()`
  previously turned *any* I/O error (not just "file missing") into an empty
  default `Config`; since every command is `load → mutate → save`, one momentary
  file lock — routine on Windows from OneDrive, an AV scanner, or the Search
  Indexer holding `config.toml` — made the next `save()` overwrite it with an
  empty default, destroying every blueprint and the non-rotating OAuth token. It
  now defaults **only** when the file genuinely doesn't exist and propagates all
  other errors so the command aborts instead of clobbering your config.
- Blueprint names are now restricted to **ASCII** alphanumerics (plus `-`/`_`).
  Names like `café` or full-width characters previously slipped past
  `validate_name` yet made fragile, cross-platform-hostile `.claude-env-<name>/`
  directory names; the error message already promised "letters, digits".
- Blueprint names that are **Windows reserved device names** (`CON`, `PRN`,
  `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, any case) are now rejected at
  creation. The `github` cap creates a bare `claude-internal/<name>/` component,
  which Windows refuses for these names — previously the error surfaced only
  late, as an opaque OS failure during placement.
- Adding (or renaming to) a blueprint whose name differs from an existing one
  **only in case** (e.g. `Coder` vs `coder`) is now rejected. Both map to the
  same `.claude-env-<name>/` dir on Windows/macOS default filesystems, so
  running one after the other silently clobbered the other's state.
- Project paths containing an underscore or space are now encoded correctly for
  session/memory lookup. Claude Code folds **every** non-alphanumeric character
  to `-` (so `…\human_behavior` → `…-human-behavior`), but aello only folded
  `\ / : .` — leaving `_`/spaces intact pointed seeded memory and `--resume` at
  a directory Claude never reads, a silent no-op for those projects.
- `aello init` now aborts on end-of-input instead of silently accepting every
  default, so a non-interactive or closed stdin can no longer auto-create a
  blueprint you never confirmed. `--model` also rejects a bare `claude-`.
- Login-token capture is more robust to changes in `claude setup-token` output:
  it scans from the end (the token is printed after the auth URL) and trims
  surrounding quotes/punctuation so a trailing character can't truncate it.
- The in-app docs reader (`?` in the TUI) can now scroll to the bottom of long
  docs — the scroll limit accounts for line wrapping instead of stopping at the
  unwrapped line count, which cut off every doc whose lines wrap.
- The tracked `claude-internal/<blueprint>/` mirror is now a true one-way sync:
  files deleted or renamed in the env (for example the `sync` skill after the
  `github` cap is dropped) are pruned from the mirror instead of lingering in
  git forever, and symlinks in the env are skipped rather than followed.
- `aello update` now rejects an implausibly small download (a truncated transfer
  or an HTML error page) instead of replacing the binary with it and bricking the
  install.
- `config.toml` is now written atomically (temp file + rename), so an
  interrupted save can no longer truncate it and lose the stored login token.
- Session resume (`--resume`) and seeded starter memory now work when the
  project path contains a `.`. aello's project-directory encoding now maps `.`
  to `-` exactly like Claude Code, so it no longer points resume and the starter
  memory at a directory Claude never reads.

### Added
- **`install.sh`** — a `curl -fsSL … | sh` one-line installer for Linux x86_64.
  Detects OS/arch, downloads the matching asset from the rolling `latest` release
  into `~/.local/bin` (override with `AELLO_BIN_DIR`), guards against a truncated
  download, and prints a PATH hint; unsupported platforms exit with a
  build-from-source pointer rather than a partial install.
- **`aello edit <name> --rename <new>`** — renames a blueprint. The new name is
  validated and rejected if it's already taken; if the blueprint is placed in the
  current project, its `.claude-env-<name>/` env dir and `claude-internal/<name>/`
  mirror are moved to the new name and the placed instance still launches.
- **`aello completions <shell>`** — prints a shell completion script (bash, zsh,
  fish, powershell, elvish) to stdout, generated from the CLI definition so it
  stays in sync. See the README for how to load it per shell.
- **`/note` skill** — seeded for *every* blueprint (like `/handoff` and
  `/twosentences`). `/note <env-name>` leaves a note for **another** environment
  sharing the repo: it writes what you were doing, the problem, and what that env
  needs to fix to `<env-name>.NOTE.md` at the repo root (the target env's inbox),
  which the target reads and then deletes. A fresh note overwrites the last, and
  each is attributed to the authoring blueprint. Unlike `/handoff` (a note to
  yourself), this is a message across environments — for when two blueprints
  split one project and one hits something on the other's side.
- **Open-source project foundation.** aello is now dual licensed under MIT and
  Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `Cargo.toml` `license` field), with
  full crate metadata (`repository`, `homepage`, `keywords`, `categories`) for
  crates.io. Added `CONTRIBUTING.md` (dev loop + conventions, pointing to
  `CLAUDE.md` for architecture), GitHub issue forms (bug report / feature
  request) and a pull-request template, and Contributing/License sections in the
  README.

### Fixed
- `aello github-setup` now always lands its bootstrap "Initial commit" even on a
  machine with no global git `user.name`/`user.email`. Previously `git commit`
  aborted with *"Author identity unknown"* on a fresh repo. The bootstrap commit
  now falls back to a synthetic `aello <aello@aello.local>` identity (injected
  per-invocation via `git -c`, mirroring aello's per-env attribution) only when
  no identity is configured — an existing git identity is used unchanged and
  nothing is written to the user's git config.

### Added
- **SessionEnd hook — contextdb now captures `/clear` and plain-exit sessions.**
  Previously the only transcript hook was PostCompact, which fires only on
  compaction; a session ended with `/clear` (or a plain exit) never compacts, so
  its context never reached contextdb — a `/clear`-heavy workflow recorded
  nothing. aello now also seeds a **SessionEnd** hook that, on the main session
  ending, archives the `/handoff` note (`HANDOFF.md`, otherwise deleted on next
  boot) plus a pointer to the full transcript, to
  `<contextdb>/<project>/<blueprint>/<ts>_<session>_end.jsonl`. It skips subagent
  session-ends so the tree isn't flooded. Existing envs self-heal the hook into
  their `settings.json` on the next `aello run` (a user-edited settings file is
  preserved; the hook is only inserted when absent).
- **TUI registry now filters to blueprints placed in the current directory.**
  Launch `aello` in a project and the list shows only the blueprints whose env
  is already placed there (`.claude-env-<name>/` exists) — so a per-project
  blueprint workflow stays uncluttered. Press `F` to toggle showing all
  blueprints; if none are placed here yet, the full list shows as before. The
  registry title and footer count reflect the active filter (`PLACED HERE · N OF M`).
- `/twosentences` — a new **universal** skill (seeded for every blueprint, like
  `/handoff`, regardless of capabilities). Invoke it manually to condense your
  previous response into exactly two sentences. Lands in every env on the next
  `aello run`.
- **In-app docs reader.** Press `?` in the TUI for a full-screen reference
  reader, or run `aello docs` (lists the docs) / `aello docs <name>` (prints one)
  from the CLI. The reader renders the repo's `docs/` (lightly styled markdown:
  headings, bullets, code, inline `code`/**bold**/links) with `↑/↓` to scroll and
  `Tab`/`←→` to switch docs. The docs are embedded into the binary at compile
  time, so `docs/` is the single source of truth — adding a `.md` there makes it
  appear in the reader with no code change. Ships a new user-facing
  `docs/migrate.md` (migrating an existing repo onto aello: the validated flow +
  the gotchas, chiefly that `/sync` won't bootstrap the CI scaffolding for you).
- `/handoff` — a new **universal** skill (seeded for every blueprint regardless
  of capabilities, unlike `/sync`). At session end it writes a self-contained
  `HANDOFF.md` resume note at the project root so the next session continues
  seamlessly after a full `/clear` (which, unlike a compact, leaves no summary).
  The note captures read-first pointers (env persona + memory), what shipped
  this session with commit shas, open threads / next steps, and gotchas —
  assuming the next session boots with zero prior context. Transient and
  untracked: read on boot, then deleted. Manual-only (`disable-model-invocation`).
- `/sync` now version-controls each env's **internal config**, not just project
  docs. A tracked `claude-internal/<blueprint>/` folder at the repo root is a
  one-way mirror of the gitignored `.claude-env-<name>/` dir:
  `claude-internal/<name>/skills/`, `claude-internal/<name>/memory/`, and
  `claude-internal/<name>/persona.CLAUDE.md` (a snapshot of the env persona,
  renamed so Claude Code never auto-loads it). The mirror is **namespaced per
  blueprint** so multiple blueprints sharing one repo don't clobber each other's
  config. The live env dir stays the single source of truth. Placement seeds the
  folder (tracked — not gitignored), and the github `/sync` step self-heals it
  (`mkdir -p`) so already-placed envs adopt it, mirrors the env into it, and
  stages it by explicit path before committing. Re-place a blueprint
  (`aello run`) to pick it up.

### Documentation
- Documented the **`VERSION` single-source-of-truth** convention for `github`-cap
  projects: derive any other version stamp (badge, `package.json` field,
  `version.ts`, …) from `VERSION` at build time and **gitignore the derived
  artifact** — never stamp a version into a second tracked file. Because the
  github cap auto-bumps `VERSION` on every push and `/sync` correctly stages only
  what the agent touched (never `git add -A`), a duplicated stamp drifts on every
  CI bump and strands dirty forever. The fix is structural (derive + gitignore),
  not a `/sync` carve-out. Added to `docs/capabilities.md` and `docs/concepts.md`
  with the `env-console` precedent. Docs only — no code change.

### Changed
- `/sync` reconcile order: memory is now refreshed **first**, before the other
  docs, so the checkpoint (and the new `claude-internal/` mirror) captures what
  the env learned this session.
- `/sync` skill (github blueprints): the commit step now runs
  `git pull --rebase origin <branch>` **after committing, before pushing**, so
  it integrates the release CI's auto-bump commit and the push fast-forwards.
  Previously each `/sync` left local one commit behind, and the next push was
  rejected until a manual rebase. Re-place a blueprint (`aello run`) to pick up
  the new skill text.

## [0.1.34]

### Changed
- Reworked the bundled starter working-style memory: it now captures that the
  user doesn't read plans — surface concrete decisions to choose from ("which
  of these?") and ask short, ask often — replacing the old go-slow / verify
  wording. Affects newly placed envs (existing memories are never clobbered).

## [0.1.33]

### Added
- Fresh placements now seed a starter memory so a new env boots with the
  user's working-style note already loaded in `/context`: a bundled
  `working-style.md` memory plus a one-line `MEMORY.md` index pointing at it,
  under `projects/<encoded-cwd>/memory/`. Seeded only when there is no
  `MEMORY.md` yet — a re-place over an established memory leaves it untouched.

## [0.1.32]

### Changed
- `/sync` skill: the commit step now stages **only the files the blueprint
  created or modified this session** (by explicit path) instead of `git add -A`.
  A blanket stage swept unrelated untracked files — other tooling's scaffolding
  or another env's in-flight work — into a blueprint's commit. Re-place a
  blueprint (`aello run`) to regenerate its `SKILL.md` from the new template.

## [0.1.30]

### Added
- `aello edit <name>` — change an existing blueprint's model, persona, or
  capabilities in place. Capability flags are tri-state: `--github` enables,
  `--no-github` disables, omitting both leaves it unchanged. Changes apply on
  the next `aello run`; the global persona in an already-placed env is never
  re-clobbered.
- TUI: `E` edits the selected blueprint through the same guided steps as add,
  pre-filled with its current model, persona, and capabilities (name fixed).

## [0.1.26]

### Added
- The `github` capability now also scaffolds `.gitattributes` (`* text=auto`,
  CRLF normalization), a generic `VERSION` file, and a stack-agnostic
  `.github/workflows/version.yml` that patch-bumps `VERSION` on every push to
  `main` and commits it back with `[skip ci]`. All seeded only if absent.
- `aello github-setup` — drives GitHub repo creation for the current project:
  prechecks `gh` auth, makes an initial commit if needed, then `gh repo create`
  (private by default; `--public`), sets `origin`, and pushes. `--name`, `--yes`.
- `aello init` — first-run wizard: logs in if there's no shared token, then walks
  you through creating your first blueprint (name, model, persona, capabilities).

## [0.1.23]

### Added
- `README.md` (install, concepts, command + capability reference) and `docs/`
  (`concepts.md`, `capabilities.md`).

## [0.1.20]

### Added
- Built-in CLAUDE.md persona templates `coder` and `sysadmin`. `--claude-md coder`
  resolves to a bundled template; any other value is still treated as a file path.
- Per-blueprint capabilities (`--project-md`, `--github`, `--changelog`, `--docs`,
  `--readme`), selectable on `aello add` and via a checklist in the TUI add flow.
  On `run`, each enabled capability scaffolds its file (CHANGELOG/README/docs/,
  project CLAUDE.md) if missing and adds its section to a generated `/sync` skill.
- `/sync` is now generated per blueprint from its capabilities — a no-GitHub
  blueprint gets no git/commit/push sections. `list` shows a `SYNC` column.
- Per-env git attribution: `run` sets `GIT_AUTHOR_*`/`GIT_COMMITTER_*` to the
  blueprint identity (`<name> <name@aello.local>`), and the GitHub `/sync` section
  appends an `Env: <name>` commit trailer — so `git blame`/`git log` reveal which
  blueprint made each change.
- With the `github` capability, `run` seeds/appends a `.claude-env-*` line to the
  project's `.gitignore` (idempotent), so env dirs and their credentials are never
  committed.

## [0.1.19]

### Fixed
- `aello login` now streams `claude setup-token` output live, so the auth URL is
  visible on headless machines (e.g. a VPS with no browser). Previously stdout was
  piped to capture the token, which swallowed the URL and made login appear to hang.
