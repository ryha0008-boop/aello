# Changelog

## [Unreleased]

### Added
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
