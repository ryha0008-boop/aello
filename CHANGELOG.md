# Changelog

## [Unreleased]

### Fixed
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
