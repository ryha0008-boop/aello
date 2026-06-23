# aello

Isolated Claude Code environments — like Python venvs, but for AI agents. Claude-only, subscription-auth, cross-platform (Linux + Windows x86_64; macOS from source).

## Docs — keep in sync with code changes

- `README.md` — user-facing: install, concepts, quick start, command + capability reference. Update when commands or capabilities change.
- `CHANGELOG.md` — version history of user-facing changes. Every user-facing change gets an entry, in the same commit as the code.
- `docs/` — deeper reference: `concepts.md` (isolation model, auth, contextdb), `capabilities.md` (capability → scaffold/`/sync` mapping, git attribution, deferred work), `migrate.md` (migrating an existing repo onto aello: validated flow + gotchas). **`docs/` is also the in-app help**: it's embedded into the binary (`docs.rs`) and rendered by `aello docs` (CLI) and the TUI reader (`?`), so `docs/` is the single source of truth — add a `.md` and it appears with no code change. Keep these in sync with behavior.
- This file (`CLAUDE.md`) — agent/architecture notes. Maintain as the design evolves.

## Architecture

**Blueprint** — a reusable agent identity in `config.toml`: `name`, `model`, optional `claude_md` (global persona), and `caps` (Capabilities). Reusable across projects.

**Capabilities** (`models.rs::Capabilities`) — five bools chosen at `add` time: `project_md`, `github`, `changelog`, `docs`, `readme`. `#[serde(default)]` so old configs load all-false. Each enabled cap, on placement, scaffolds its file (only if missing) and contributes a section to a generated `/sync` skill. `github` scaffolds the most: the `.claude-env-*` gitignore line, `.gitattributes` (CRLF normalize), a generic `VERSION` + stack-agnostic `.github/workflows/version.yml` patch-bump CI, and the tracked `claude-internal/` mirror (see below).

**`claude-internal/`** — a TRACKED folder at the project root, one-way mirror of the gitignored env dir, so a blueprint's skills/memory/persona reach git. **Namespaced per blueprint** (`claude-internal/<name>/...`) so multiple blueprints sharing one repo don't clobber each other's mirror. Three parts: `claude-internal/<name>/skills/` ← `<env>/skills/`, `claude-internal/<name>/memory/` ← `<env>/projects/<encoded-cwd>/memory/`, and `claude-internal/<name>/persona.CLAUDE.md` ← `<env>/CLAUDE.md` (renamed so Claude Code never auto-loads the snapshot). The live env dir stays the single source of truth; the mirror is written one-way from it (never read back). Seeded at placement (`scaffold_project` → `mirror_env_internal`, taking the blueprint name, github only, NOT added to the `.claude-env-*` gitignore) and refreshed + staged by the github `/sync` step, which self-heals the folder (`mkdir -p`) so already-placed envs adopt it.

**Env dir** — `<project>/.claude-env-<name>/`, the blueprint's `CLAUDE_CONFIG_DIR`. Holds `settings.json`, the global persona `CLAUDE.md`, `.aello.toml` (the placed Instance), `hooks/post-compact.py`, and the generated `skills/sync/SKILL.md`. Gitignored by convention (the `github` cap seeds the `.claude-env-*` ignore line).

**Two CLAUDE.md layers** — global persona (`<env>/CLAUDE.md`, set once, never clobbered) vs project (`<project>/CLAUDE.md`, `--project-md`, maintained by `/sync`). Memory is separate: `place` seeds a starter working-style memory under `<env>/projects/<encoded-cwd>/memory/` on first placement (gated on `MEMORY.md` absence, never clobbered), then it's automatic (PostCompact hook).

**Auth** — `aello login` runs `claude setup-token`, stores a long-lived non-rotating `CLAUDE_CODE_OAUTH_TOKEN` in `config.toml`; every env exports it. Concurrency-safe across many parallel envs (the reason credential-copy was abandoned). On fresh envs, `mark_onboarded` seeds `hasCompletedOnboarding` so Claude skips its first-run wizard.

**contextdb** — only one hook (PostCompact). Transcripts → `<contextdb>/<project>/<blueprint>/<ts>_<session>.jsonl`. Root in `config.toml` (`contextdb`, default `~/aello/contextdb`), passed as `AELLO_CONTEXTDB`.

**Git attribution** — with `github`, `launch.rs` sets `GIT_AUTHOR_*`/`GIT_COMMITTER_*` to `<blueprint> <blueprint@aello.local>`, and the `/sync` github section appends an `Env: <blueprint>` commit trailer. Multi-blueprint repos stay traceable via `git blame` / `git log --author`.

## Module map (`src/`)

- `main.rs` — clap CLI + dispatch (`add`/`list`/`remove`/`edit`/`run`/`init`/`login`/`github-setup`/`update`/`docs`); `cmd_docs` (prints a bundled doc, or lists them); `cmd_edit` (in-place blueprint edit; `EditArgs` + tri-state `tri` cap flags); `run_blueprint` (shared by CLI `run` and the TUI); `cmd_init` (first-run wizard) + `prompt`/`prompt_bool`/`prompt_optional`; `validate_name`/`validate_model`; Windows `aello.exe.old*` startup sweep.
- `models.rs` — `Blueprint`, `Capabilities`, `Instance`, `Config`.
- `config.rs` — `config.toml` load/save; `contextdb_dir`; `expand_home` (splits on `/` and `\`); `home_dir`.
- `project.rs` — `env_dir`; `place` (writes `.aello.toml`/settings/persona/hook, regenerates `/sync`, always seeds the universal `/handoff` and `/twosentences` skills, seeds starter memory, then scaffolds — memory before scaffold so the mirror captures it); `settings_json`; `mark_onboarded`; `scaffold_project(project, env_dir, blueprint, caps)` (incl. github's `.gitattributes`/`VERSION`/`version.yml` and the `claude-internal/<name>/` mirror); `mirror_env_internal` (takes the blueprint name) + `copy_dir_all` (one-way env → `claude-internal/<name>/`); `seed_memory` (bundled working-style memory + `MEMORY.md` index, only when no `MEMORY.md` yet); `ensure_gitignore_entry` (idempotent); `VERSION_WORKFLOW`.
- `github.rs` — `aello github-setup`: `gh` auth precheck → ensure git repo + initial commit → `gh repo create --source=. --remote=origin --push`; pure `repo_create_args`. The bootstrap commit injects a synthetic `aello <aello@aello.local>` identity via per-invocation `git -c` (`initial_commit_args` + `has_git_identity`) when the machine has no git identity, so it lands on a fresh box; an existing identity is used unchanged.
- `templates.rs` — bundled personas (`coder`, `sysadmin` via `include_str!`), `resolve` (builtin name or path), `render_sync_skill(caps, name)`, `render_handoff_skill(name)` (universal `/handoff` skill), `render_twosentences_skill()` (universal `/twosentences` skill), `BUILTINS`.
- `docs.rs` — embeds the repo's `docs/` at compile time (`include_dir!`); `all()` (docs in reading order, title from first H1), `get(slug)`. Backs both `aello docs` and the TUI reader. New `.md` files appear automatically (no per-file code).
- `launch.rs` — `launch` (sets `CLAUDE_CONFIG_DIR`, `AELLO_CONTEXTDB`, git attribution env, `CLAUDE_CODE_OAUTH_TOKEN`); `git_identity`.
- `auth.rs` — `capture_setup_token` (tees `claude setup-token` stdout so the auth URL shows on headless machines); `extract_token`.
- `sessions.rs` — session listing for resume.
- `tui.rs` — Kinetic Command TUI; add flow is name → model → persona → caps checklist; `E` reuses the model→persona→caps steps in edit mode (an `edit` flag on those modes; name fixed, steps pre-seeded via `model_index`/`persona_index`, final step updates in place). Guard test keeps `PERSONAS` in sync with `templates::BUILTINS`. `?` opens `Mode::Help`, a full-screen reader over `docs.rs` (left doc list, right scrollable content via `render_markdown` — a light markdown→ratatui renderer for headings/bullets/code/inline).
- `update.rs` — self-update from the rolling `latest` release.
- `templates/coder.md`, `templates/sysadmin.md` — bundled personas. `templates/memory-working-style.md` — bundled starter memory (`MEMORY_WORKING_STYLE` in `project.rs`). `src/hooks_post_compact.py` — the PostCompact hook.

## `/sync`

Generated per blueprint from its caps (`templates::render_sync_skill`), seeded to `<env>/skills/sync/SKILL.md` when `caps.any()`. Manual-only (`disable-model-invocation: true`) — replaces the old auto-commit hooks. A no-`github` blueprint gets no git/commit/push sections and no `Bash` tool. Sections, in order: repo health (github), reconcile **memory first** then the enabled docs (two-way), mirror env config into `claude-internal/<name>/` + stage by path (github), commit + rebase-before-push with `Env:` trailer (github).

## `/handoff`

Universal counterpart to `/sync` (`templates::render_handoff_skill`), seeded **unconditionally** for every blueprint at `<env>/skills/handoff/SKILL.md` (no caps gate — even a bare blueprint gets it). Manual-only (`disable-model-invocation: true`), tools `Write, Read, Bash`. At session end it writes a transient, untracked `HANDOFF.md` at the project root so the next session resumes after a full `/clear` (not a compact — a clear leaves no summary, so the note is fully self-contained: read-first pointers, what shipped + commit shas, open threads/next steps, gotchas). Read on boot, then deleted.

## `/twosentences`

Also universal (`templates::render_twosentences_skill`), seeded **unconditionally** for every blueprint at `<env>/skills/twosentences/SKILL.md` alongside `/handoff`. Manual-only (`disable-model-invocation: true`), no tools (empty `allowed-tools` — it's a pure text task). Condenses the previous assistant response into exactly two sentences, output with no other text.

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

- Phase 4 hook toggles — moot (one hook). Blueprint edit shipped both as `aello edit` (CLI; model/persona/caps in place, tri-state cap flags) and in the TUI (`E`, guided edit). Editing a blueprint's *name* (a rename) is still unsupported in both — it would require moving the placed env dir.

Shipped since the original roadmap: aello-driven GitHub setup is now the `aello github-setup` command (`github.rs`); the `github` cap now also scaffolds `.gitattributes` and a generic `VERSION` + `version.yml` patch-bump CI for target projects; `aello init` is the first-run wizard (login + first blueprint, capabilities included). CI actions all target Node 24 (`checkout@v5`, `upload-artifact@v7`, `download-artifact@v8` — the artifact repos jumped majors faster than checkout); the Node-20 deprecation is resolved.
