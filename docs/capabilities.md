# Capabilities & `/sync`

Capabilities are chosen per blueprint at creation — via flags on `aello add` or the checklist in the TUI add flow (name → model → persona → **capabilities**). They're stored on the blueprint and applied every time it's placed with `aello run`.

Each enabled capability does two things on placement:
1. **Scaffolds** its file in the project, only if missing (never overwrites your content).
2. **Adds a section** to a `/sync` skill generated for that blueprint.

| Capability | Scaffolds (if absent) | `/sync` section |
|---|---|---|
| `project_md` | project-root `CLAUDE.md` | reconcile the project CLAUDE.md |
| `github` | `.gitignore` line `.claude-env-*`, `.gitattributes` (CRLF normalize), `VERSION` + `.github/workflows/version.yml` (patch-bump CI) | repo health checks; commit + push; `Env:` trailer |
| `changelog` | `CHANGELOG.md` (`## [Unreleased]`) | keep CHANGELOG current |
| `docs` | `docs/` directory | reconcile `docs/` |
| `readme` | `README.md` | keep README current |

The global persona (`--claude-md`) is separate from capabilities — it writes the env-level `CLAUDE.md` once.

## The generated `/sync` skill

`/sync` replaces the old auto-commit-every-turn hooks. It's **manual only** (`disable-model-invocation: true`) — nothing happens until you type `/sync` inside Claude.

Crucially, the skill is **generated from the blueprint's capabilities**, not a one-size-fits-all file. A blueprint with no `github` gets a `/sync` with **no git, commit, or push sections at all** (and no `Bash` in `allowed-tools`) — it just reconciles whatever docs are enabled, locally. This keeps the agent from being told about a workflow it doesn't have.

What `/sync` does when invoked (only the enabled parts):
- **Repo health** (github) — confirm it's a git repo, check for an `origin` remote (offer `gh repo create` if missing, with confirmation), report branch / ahead-behind / status.
- **Reconcile docs** — for each enabled, existing doc, a two-way staleness pass: add what's missing, fix what's wrong, delete what no longer applies. Reports per file: updated / fresh / skipped.
- **Commit + push** (github) — stage, commit with a clear message ending in an `Env: <blueprint>` trailer, push to `origin`.

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

## GitHub setup — `aello github-setup`

`aello github-setup` creates the GitHub repo for the current project and pushes it, so you don't have to do it by hand before a blueprint can `/sync`:

1. Prechecks `gh` is installed and authenticated (`gh auth status`).
2. Initializes a git repo and an initial commit if the directory has none.
3. If an `origin` remote already exists, reports it and stops.
4. Otherwise creates the repo with `gh repo create` (private by default; `--public` for public), sets `origin`, and pushes `main`.

Flags: `--name <repo>` (default: directory name), `--public`, `--yes` (skip confirmation). This is the aello-driven counterpart to the repo creation `/sync` only *offers* at runtime.
