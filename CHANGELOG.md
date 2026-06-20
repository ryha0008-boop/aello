# Changelog

## [Unreleased]

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
