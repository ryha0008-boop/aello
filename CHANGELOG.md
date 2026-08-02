# Changelog

## [Unreleased]

### Added
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
