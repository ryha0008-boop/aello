# Capabilities & `/sync`

Capabilities are chosen per blueprint at creation — via flags on `aello add` or the checklist in the TUI add flow (name → model → persona → **capabilities**) — and can be changed later with `aello edit <name>` (tri-state flags: `--github` / `--no-github`) or the TUI's guided edit (`E`). They're stored on the blueprint and applied every time it's placed with `aello run`.

Each enabled capability does two things on placement:
1. **Scaffolds** its file in the project, only if missing (never overwrites your content).
2. **Adds a section** to a `/sync` skill generated for that blueprint.

| Capability | Scaffolds (if absent) | `/sync` section |
|---|---|---|
| `project_md` | project-root `CLAUDE.md` | reconcile the project CLAUDE.md |
| `github` | `.gitignore` line `.claude-env-*`, `.gitattributes` (CRLF normalize), `VERSION` + `.github/workflows/version.yml` (patch-bump CI), tracked `claude-internal/` mirror | repo health checks; mirror env config into `claude-internal/`; commit + push; `Env:` trailer |
| `changelog` | `CHANGELOG.md` (`## [Unreleased]`) | keep CHANGELOG current |
| `docs` | `docs/` directory | reconcile `docs/` |
| `readme` | `README.md` | keep README current |

The global persona (`--claude-md`) is separate from capabilities — it writes the env-level `CLAUDE.md` once.

## The generated `/sync` skill

`/sync` replaces the old auto-commit-every-turn hooks. It's **manual only** (`disable-model-invocation: true`) — nothing happens until you type `/sync` inside Claude.

Crucially, the skill is **generated from the blueprint's capabilities**, not a one-size-fits-all file. A blueprint with no `github` gets a `/sync` with **no git, commit, or push sections at all** (and no `Bash` in `allowed-tools`) — it just reconciles whatever docs are enabled, locally. This keeps the agent from being told about a workflow it doesn't have.

What `/sync` does when invoked (only the enabled parts):
- **Repo health** (github) — confirm it's a git repo, check for an `origin` remote (offer `gh repo create` if missing, with confirmation), report branch / ahead-behind / status.
- **Reconcile memory, then docs** — memory is refreshed **first** (its `MEMORY.md` index and per-fact files), then each enabled, existing doc gets a two-way staleness pass: add what's missing, fix what's wrong, delete what no longer applies. Reports per file: updated / fresh / skipped.
- **Mirror env config** (github) — one-way copy of the env's `skills/`, `memory/`, and persona into the tracked `claude-internal/` folder (see below), staged by explicit path. Self-heals the folder (`mkdir -p`) so already-placed envs adopt it.
- **Commit + push** (github) — stage **only the files touched this session** (by explicit path, never `git add -A`), commit with a clear message ending in an `Env: <blueprint>` trailer, then `git pull --rebase origin <branch>` (absorbs the release CI's auto-bump so the push fast-forwards) and push to `origin`.

### `claude-internal/` — version-controlling the env

The env dir (`.claude-env-<name>/`) is gitignored — it holds credentials and per-machine state — so the skills, memory, and persona that define a blueprint would otherwise never reach git. The `github` cap fixes this with **`claude-internal/`**, a tracked folder at the repo root that is a **one-way mirror** of the live env dir:

```
claude-internal/
├── skills/            # mirror of <env>/skills/
├── memory/            # mirror of <env>/projects/<cwd>/memory/
└── persona.CLAUDE.md  # snapshot of <env>/CLAUDE.md, renamed so it never auto-loads
```

The live env dir stays the **single source of truth** — `claude-internal/` is only ever written *from* it, never read back into it. The persona snapshot is deliberately **not** named `CLAUDE.md` (which Claude Code would auto-load as a second persona). The folder is seeded at placement and refreshed by every `/sync`; it is **not** covered by the `.claude-env-*` gitignore line, so it commits normally.

The skill is re-generated on every `aello run`, so changing a blueprint's capabilities updates its `/sync` on the next placement. If all capabilities are disabled, no `/sync` skill is seeded.

## Git attribution

With `github` enabled, `aello run` sets, for the launched Claude process:

```
GIT_AUTHOR_NAME    = <blueprint>
GIT_AUTHOR_EMAIL   = <blueprint>@aello.local
GIT_COMMITTER_NAME = <blueprint>
GIT_COMMITTER_EMAIL= <blueprint>@aello.local
```

So every commit a blueprint makes is attributed to it — both author and committer, independent of your machine's global git config. Combined with the `Env: <blueprint>` commit trailer, this makes multi-agent history fully traceable:

```sh
git log --author=reviewer          # everything the "reviewer" blueprint committed
git blame path/to/file             # who-wrote-what, by blueprint
git log --format='%(trailers:key=Env)'
```

This is the point of running several blueprints in one repo: when something breaks, `git blame` tells you which agent did it.

The seeded `VERSION` + `.github/workflows/version.yml` are **generic and stack-agnostic** — meant for *target* projects. The workflow patch-bumps `VERSION` on every push to `main` and commits it back with `[skip ci]` (a `GITHUB_TOKEN` push doesn't re-trigger CI). Bump minor/major by hand. Delete either file if a project manages versions another way.

### Convention: `VERSION` is the single source of truth — derive, don't duplicate

In a `github`-cap project, **`VERSION` is the one tracked place the version lives.** Any other version stamp a project needs — a README badge, `package.json`'s `version` field, a generated `version.ts`/`__version__`, etc. — must be **derived from `VERSION` at build time and the derived artifact gitignored.** Never write a version stamp into a second *tracked* file.

Why this is a hard rule and not a style preference: the github cap auto-bumps `VERSION` on every push (`version.yml`). If a project also stamps the version into a tracked file, that file goes stale the instant CI bumps `VERSION` — it now disagrees with `VERSION` and shows up as a dirty working-tree change. And the generated `/sync` **cannot** rescue it: `/sync` stages **only the files the agent actually touched this session** (never `git add -A`), by design, so a build-regenerated stamp the agent never edited is never staged. The drifted artifact strands dirty forever — every build re-dirties it, every `/sync` correctly leaves it alone.

So the fix is structural, not a `/sync` carve-out (auto-staging build output would quietly weaken that staging guarantee). Keep the derived stamp **out of git**:

- ✅ `VERSION` (tracked) → build reads it → writes `version.ts` / badge / `package.json` field → **`version.ts` etc. is gitignored**.
- ❌ `VERSION` (tracked) **and** `lib/version.ts` (tracked) both holding the version → drifts on every CI bump, never reconcilable by `/sync`.

**Precedent:** the `env-console` project did exactly the ❌ form (a tracked `lib/version.ts` plus `package.json`'s `version`), drifted on every build, and was fixed by gitignoring the derived `version.ts` (the "Option A" fix) so `VERSION` stayed the only tracked source.

## GitHub setup — `aello github-setup`

`aello github-setup` creates the GitHub repo for the current project and pushes it, so you don't have to do it by hand before a blueprint can `/sync`:

1. Prechecks `gh` is installed and authenticated (`gh auth status`).
2. Initializes a git repo and an initial commit if the directory has none.
3. If an `origin` remote already exists, reports it and stops.
4. Otherwise creates the repo with `gh repo create` (private by default; `--public` for public), sets `origin`, and pushes `main`.

Flags: `--name <repo>` (default: directory name), `--public`, `--yes` (skip confirmation). This is the aello-driven counterpart to the repo creation `/sync` only *offers* at runtime.
