# aello

Isolated Claude Code environments — like Python venvs, but for AI agents. Claude-only, subscription-auth, cross-platform (Linux + Windows x86_64; macOS from source).

## Docs — keep in sync with code changes

- `README.md` — user-facing: install, concepts, quick start, command + capability reference. Update when commands or capabilities change.
- `CHANGELOG.md` — version history of user-facing changes. Every user-facing change gets an entry, in the same commit as the code.
- `docs/` — deeper reference: `concepts.md` (isolation model, auth, contextdb), `capabilities.md` (capability → scaffold/`/sync` mapping, git attribution, deferred work).
- This file (`CLAUDE.md`) — agent/architecture notes. Maintain as the design evolves.

## Architecture

**Blueprint** — a reusable agent identity in `config.toml`: `name`, `model`, optional `claude_md` (global persona), and `caps` (Capabilities). Reusable across projects.

**Capabilities** (`models.rs::Capabilities`) — five bools chosen at `add` time: `project_md`, `github`, `changelog`, `docs`, `readme`. `#[serde(default)]` so old configs load all-false. Each enabled cap, on placement, scaffolds its file (only if missing) and contributes a section to a generated `/sync` skill. `github` scaffolds the most: the `.claude-env-*` gitignore line, `.gitattributes` (CRLF normalize), and a generic `VERSION` + stack-agnostic `.github/workflows/version.yml` patch-bump CI.

**Env dir** — `<project>/.claude-env-<name>/`, the blueprint's `CLAUDE_CONFIG_DIR`. Holds `settings.json`, the global persona `CLAUDE.md`, `.aello.toml` (the placed Instance), `hooks/post-compact.py`, and the generated `skills/sync/SKILL.md`. Gitignored by convention (the `github` cap seeds the `.claude-env-*` ignore line).

**Two CLAUDE.md layers** — global persona (`<env>/CLAUDE.md`, set once, never clobbered) vs project (`<project>/CLAUDE.md`, `--project-md`, maintained by `/sync`). Memory is separate/automatic (PostCompact hook).

**Auth** — `aello login` runs `claude setup-token`, stores a long-lived non-rotating `CLAUDE_CODE_OAUTH_TOKEN` in `config.toml`; every env exports it. Concurrency-safe across many parallel envs (the reason credential-copy was abandoned). On fresh envs, `mark_onboarded` seeds `hasCompletedOnboarding` so Claude skips its first-run wizard.

**contextdb** — only one hook (PostCompact). Transcripts → `<contextdb>/<project>/<blueprint>/<ts>_<session>.jsonl`. Root in `config.toml` (`contextdb`, default `~/aello/contextdb`), passed as `AELLO_CONTEXTDB`.

**Git attribution** — with `github`, `launch.rs` sets `GIT_AUTHOR_*`/`GIT_COMMITTER_*` to `<blueprint> <blueprint@aello.local>`, and the `/sync` github section appends an `Env: <blueprint>` commit trailer. Multi-blueprint repos stay traceable via `git blame` / `git log --author`.

## Module map (`src/`)

- `main.rs` — clap CLI + dispatch (`add`/`list`/`remove`/`run`/`init`/`login`/`github-setup`/`update`); `run_blueprint` (shared by CLI `run` and the TUI); `cmd_init` (first-run wizard) + `prompt`/`prompt_optional`; `validate_name`/`validate_model`; Windows `aello.exe.old*` startup sweep.
- `models.rs` — `Blueprint`, `Capabilities`, `Instance`, `Config`.
- `config.rs` — `config.toml` load/save; `contextdb_dir`; `expand_home` (splits on `/` and `\`); `home_dir`.
- `project.rs` — `env_dir`; `place` (writes `.aello.toml`/settings/persona/hook, regenerates `/sync`, scaffolds); `settings_json`; `mark_onboarded`; `scaffold_project` (incl. github's `.gitattributes`/`VERSION`/`version.yml`); `ensure_gitignore_entry` (idempotent); `VERSION_WORKFLOW`.
- `github.rs` — `aello github-setup`: `gh` auth precheck → ensure git repo + initial commit → `gh repo create --source=. --remote=origin --push`; pure `repo_create_args`.
- `templates.rs` — bundled personas (`coder`, `sysadmin` via `include_str!`), `resolve` (builtin name or path), `render_sync_skill(caps, name)`, `BUILTINS`.
- `launch.rs` — `launch` (sets `CLAUDE_CONFIG_DIR`, `AELLO_CONTEXTDB`, git attribution env, `CLAUDE_CODE_OAUTH_TOKEN`); `git_identity`.
- `auth.rs` — `capture_setup_token` (tees `claude setup-token` stdout so the auth URL shows on headless machines); `extract_token`.
- `sessions.rs` — session listing for resume.
- `tui.rs` — Kinetic Command TUI; add flow is name → model → persona → caps checklist; guard test keeps `PERSONAS` in sync with `templates::BUILTINS`.
- `update.rs` — self-update from the rolling `latest` release.
- `templates/coder.md`, `templates/sysadmin.md` — bundled personas. `src/hooks_post_compact.py` — the PostCompact hook.

## `/sync`

Generated per blueprint from its caps (`templates::render_sync_skill`), seeded to `<env>/skills/sync/SKILL.md` when `caps.any()`. Manual-only (`disable-model-invocation: true`) — replaces the old auto-commit hooks. A no-`github` blueprint gets no git/commit/push sections and no `Bash` tool. Sections: repo health (github), two-way doc reconcile (per enabled doc), commit + push with `Env:` trailer (github).

## Development rules

- **Every user-facing change gets a `CHANGELOG.md` entry**, in the same commit as the code.
- **Small, scoped commits** — one change per commit; don't bundle unrelated edits.
- **Verify**: `cargo build --release` + `cargo test` must be green before pushing. Add a test for new behavior (templates/place/launch logic is all unit-testable without launching Claude).
- **Simplicity & surgical edits** — minimum code; match surrounding style; touch only what's required.

## Release process

Push to `main` → GitHub Actions:
1. **bump** job increments the patch in `Cargo.toml` (+0.0.1), commits `release: vX.Y.Z [skip ci]`, pushes via `GITHUB_TOKEN` (does not re-trigger CI).
2. **build** jobs (ref: `main`) build `x86_64-unknown-linux-gnu` → `aello-x86_64-linux` and `x86_64-pc-windows-msvc` → `aello-x86_64-windows.exe`.
3. **publish** uploads both to the single permanent rolling `latest` release with `gh release upload --clobber`. Never delete+recreate the release (a draft state breaks `aello update` with a 404).

No version tags (commits ≠ tags). After CI, `git pull --rebase` to sync the bumped `Cargo.toml` before the next local `cargo install`. Minor/major versions are bumped manually in `Cargo.toml`. If a plain push doesn't trigger the workflow, `gh workflow run release.yml --ref main` is the fallback.

## Build & install

```sh
cargo build --release          # writes to target/ — safe while aello is running
cargo test                     # unit tests, no Claude launch needed
cargo install --path . --force # replace ~/.cargo/bin/aello with the local build
```

## Deferred

- Phase 4 "instance edit / hook toggles" — largely moot (one hook; model lives in the blueprint).
- Capability selection in the `aello init` wizard — today it creates the first blueprint with no caps (set them via the TUI or `aello add`).

Shipped since the original roadmap: aello-driven GitHub setup is now the `aello github-setup` command (`github.rs`); the `github` cap now also scaffolds `.gitattributes` and a generic `VERSION` + `version.yml` patch-bump CI for target projects; `aello init` is the first-run wizard. CI already runs Node-20 actions (`@v4`), so no deprecation bump is pending.
