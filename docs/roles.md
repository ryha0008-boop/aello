# Roles & `/sync`

A blueprint's **role** is what it is responsible for in a project. It's chosen at creation — `aello add <name> --role <role>`, or the picker in the TUI add flow (name → model → persona → **role**) — and changed later with `aello edit <name> --role <role>` or the TUI's guided edit (`E`). It's stored on the blueprint and applied every time it's placed with `aello run`.

There are three:

| Role | Responsible for | `/sync` |
|---|---|---|
| `maintainer` | the repo's prose and its git history: project `CLAUDE.md`, `CHANGELOG.md`, `docs/`, `README.md` | full — repo health, reconcile memory + all four docs, mirror the env, commit + push |
| `contributor` | its own code and its own changelog entry | git only — repo health, `CHANGELOG.md`, mirror the env, commit + push |
| `standalone` | nothing outside its own session | none — no `/sync` skill is seeded at all |

**One maintainer per repo, any number of contributors.** That is the shape this exists for: several blueprints share a repo, one of them owns the documentation, and the rest commit their work without rewriting prose they don't own. A contributor logs its change in `CHANGELOG.md` — its own entry, in its own commit — but never touches `CLAUDE.md`, `docs/`, or `README.md`.

`standalone` is the right choice for an agent that isn't working on a shared codebase at all: a research assistant, a one-off runner, a sysadmin env. It gets the universal skills (`/handoff`, `/note`, `/twosentences`) like everything else, just no `/sync`.

Each role scaffolds the files it maintains, on placement, **only if missing** — nothing you have written is ever overwritten:

| File / artifact | maintainer | contributor | standalone |
|---|:--:|:--:|:--:|
| `.gitignore` line `.claude-env-*` | ✅ | ✅ | — |
| `.gitattributes` (CRLF normalize) | ✅ | ✅ | — |
| `VERSION` + `.github/workflows/version.yml` (patch-bump CI) | ✅ | ✅ | — |
| tracked `claude-internal/<name>/` mirror | ✅ | ✅ | — |
| `CHANGELOG.md` (`## [Unreleased]`) | ✅ | ✅ | — |
| project-root `CLAUDE.md` | ✅ | — | — |
| `docs/` directory | ✅ | — | — |
| `README.md` | ✅ | — | — |

The global persona (`--claude-md`) is separate from the role — it writes the env-level `CLAUDE.md` once, and no role rewrites it. So is the voice, below: it is not a role setting and there is nothing to enable.

## Upgrading from capabilities

Before 0.2, a blueprint carried five independent booleans (`project_md`, `github`, `changelog`, `docs`, `readme`) set with flags like `--github` / `--no-github`. Those flags are **gone** from `aello add` and `aello edit`; `--role` replaces all ten.

Existing configs migrate themselves on the next load — nothing to run:

- anything that maintained prose (`project_md`, `docs`, or `readme`) → **maintainer**
- anything left holding only git duties (`github`, `changelog`) → **contributor**
- nothing enabled → **standalone**

The old `[blueprints.caps]` table is read once and dropped the next time aello saves the config. Check the result with `aello list`, whose last column is now `ROLE`.

The one behaviour change to know about: a blueprint that previously had *some but not all* of the prose capabilities becomes a maintainer, so `/sync` will start reconciling the files it didn't cover before. If that isn't what you want, `aello edit <name> --role contributor`.

## The voice — not a role setting

Every placed env speaks, unconditionally: there is no flag, no TUI row and nothing to enable. It was a capability (`--voice`) once and stopped earning the flag, which is why it is documented on its own page rather than in this table.

See **[voice.md](voice.md)** for the mechanism — why the hook is vendored into the env, why all five files must be copied, who registers the Windows toast identity, the `HOOK_VERSION` drift check, and what to look at when it doesn't speak.

## The generated `/sync` skill

`/sync` replaces the old auto-commit-every-turn hooks. It's **manual only** (`disable-model-invocation: true`) — nothing happens until you type `/sync` inside Claude.

Crucially, the skill is **generated from the blueprint's role**, not a one-size-fits-all file. A contributor's `/sync` contains no instructions about `README.md` or `docs/` at all, so the agent is never told about work it isn't allowed to do. A standalone blueprint gets no `/sync` file and no `Bash` tool with it.

What `/sync` does when invoked (only the parts the role covers):
- **Repo health** — confirm it's a git repo, check for an `origin` remote (offer `gh repo create` if missing, with confirmation), report branch / ahead-behind / status.
- **Reconcile memory, then docs** — memory is refreshed **first** (its `MEMORY.md` index and per-fact files), then each doc the role owns gets a two-way staleness pass: add what's missing, fix what's wrong, delete what no longer applies. Reports per file: updated / fresh / skipped. For a contributor this is `CHANGELOG.md` alone.
- **Mirror env config** — one-way copy of the env's `skills/`, `memory/`, and persona into the tracked per-blueprint `claude-internal/<name>/` folder (see below), staged by explicit path. Self-heals the folder (`mkdir -p`) so already-placed envs adopt it.
- **Commit + push** — stage **only the files touched this session** (by explicit path, never `git add -A`), commit with a clear message ending in an `Env: <blueprint>` trailer, then `git pull --rebase origin <branch>` (absorbs the release CI's auto-bump so the push fast-forwards) and push to `origin`.

### `claude-internal/` — version-controlling the env

The env dir (`.claude-env-<name>/`) is gitignored — it holds credentials and per-machine state — so the skills, memory, and persona that define a blueprint would otherwise never reach git. Any role with git duties fixes this with **`claude-internal/`**, a tracked folder at the repo root that is a **one-way mirror** of the live env dir:

```
claude-internal/
└── <name>/            # one namespace per blueprint sharing the repo
    ├── skills/            # mirror of <env>/skills/
    ├── memory/            # mirror of <env>/projects/<cwd>/memory/
    └── persona.CLAUDE.md  # snapshot of <env>/CLAUDE.md, renamed so it never auto-loads
```

The live env dir stays the **single source of truth** — `claude-internal/<name>/` is only ever written *from* it, never read back into it. It is **namespaced per blueprint** so multiple blueprints sharing one repo don't clobber each other's mirror. The persona snapshot is deliberately **not** named `CLAUDE.md` (which Claude Code would auto-load as a second persona). The folder is seeded at placement and refreshed by every `/sync`; it is **not** covered by the `.claude-env-*` gitignore line, so it commits normally.

The skill is re-generated on every `aello run`, so changing a blueprint's role updates its `/sync` on the next placement. A `standalone` blueprint gets no `/sync` skill at all.

## Git attribution

For a maintainer or a contributor, `aello run` sets, for the launched Claude process:

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

In a repo worked by a maintainer or contributor, **`VERSION` is the one tracked place the version lives.** Any other version stamp a project needs — a README badge, `package.json`'s `version` field, a generated `version.ts`/`__version__`, etc. — must be **derived from `VERSION` at build time and the derived artifact gitignored.** Never write a version stamp into a second *tracked* file.

Why this is a hard rule and not a style preference: the scaffolded CI auto-bumps `VERSION` on every push (`version.yml`). If a project also stamps the version into a tracked file, that file goes stale the instant CI bumps `VERSION` — it now disagrees with `VERSION` and shows up as a dirty working-tree change. And the generated `/sync` **cannot** rescue it: `/sync` stages **only the files the agent actually touched this session** (never `git add -A`), by design, so a build-regenerated stamp the agent never edited is never staged. The drifted artifact strands dirty forever — every build re-dirties it, every `/sync` correctly leaves it alone.

So the fix is structural, not a `/sync` carve-out (auto-staging build output would quietly weaken that staging guarantee). Keep the derived stamp **out of git**:

- ✅ `VERSION` (tracked) → build reads it → writes `version.ts` / badge / `package.json` field → **`version.ts` etc. is gitignored**.
- ❌ `VERSION` (tracked) **and** `lib/version.ts` (tracked) both holding the version → drifts on every CI bump, never reconcilable by `/sync`.

**Precedent:** the `env-console` project did exactly the ❌ form (a tracked `lib/version.ts` plus `package.json`'s `version`), drifted on every build, and was fixed by gitignoring the derived `version.ts` (the "Option A" fix) so `VERSION` stayed the only tracked source.

## GitHub setup — `aello github-setup`

`aello github-setup` creates the GitHub repo for the current project and pushes it, so you don't have to do it by hand before a blueprint can `/sync`:

1. Prechecks `gh` is installed and authenticated (`gh auth status`).
2. Initializes a git repo and an initial commit if the directory has none. The bootstrap commit falls back to a synthetic `aello <aello@aello.local>` identity when the machine has no git `user.name`/`user.email`, so it lands even on a freshly configured box (an existing git identity is used unchanged).
3. If an `origin` remote already exists, reports it and stops.
4. Otherwise creates the repo with `gh repo create` (private by default; `--public` for public), sets `origin`, and pushes `main`.

Flags: `--name <repo>` (default: directory name), `--public`, `--yes` (skip confirmation). This is the aello-driven counterpart to the repo creation `/sync` only *offers* at runtime.
