# Capabilities & `/sync`

Capabilities are chosen per blueprint at creation — via flags on `aello add` or the checklist in the TUI add flow (name → model → persona → **capabilities**) — and can be changed later with `aello edit <name>` (tri-state flags: `--github` / `--no-github`) or the TUI's guided edit (`E`). They're stored on the blueprint and applied every time it's placed with `aello run`.

Each enabled capability does two things on placement:
1. **Scaffolds** its file in the project, only if missing (never overwrites your content).
2. **Adds a section** to a `/sync` skill generated for that blueprint.

| Capability | Scaffolds (if absent) | `/sync` section |
|---|---|---|
| `project_md` | project-root `CLAUDE.md` | reconcile the project CLAUDE.md |
| `github` | `.gitignore` line `.claude-env-*`, `.gitattributes` (CRLF normalize), `VERSION` + `.github/workflows/version.yml` (patch-bump CI), tracked `claude-internal/<name>/` mirror | repo health checks; mirror env config into `claude-internal/<name>/`; commit + push; `Env:` trailer |
| `changelog` | `CHANGELOG.md` (`## [Unreleased]`) | keep CHANGELOG current |
| `docs` | `docs/` directory | reconcile `docs/` |
| `readme` | `README.md` | keep README current |
| `voice` | `hooks/speak.py` + `duck.py` + `win_audio.ps1` in the env; a TL;DR section appended to the persona | — (see below) |

The global persona (`--claude-md`) is separate from capabilities — it writes the env-level `CLAUDE.md` once.

## `voice` — speaking responses aloud

`voice` is the odd one out: it maintains no project file and adds no `/sync` section. It registers a `Stop` hook that speaks each response's trailing `TL;DR:` line through a free Edge neural voice, plus a `SessionEnd` hook that hands the borrowed voice back. Because it contributes nothing to `/sync`, a blueprint with `voice` and nothing else gets **no `/sync` skill at all** — `Capabilities::any()` deliberately ignores it.

**Why the hook is vendored into the env.** It's copied to `<env>/hooks/` and registered as `$CLAUDE_CONFIG_DIR/hooks/speak.py`. The alternative — pointing every env at one checkout of the script — couples unrelated projects to that path: move or rename it and every env goes silent at once, and each newly placed env stays silent until edited by hand. `speak.py` imports `duck` as a sibling and shells out to `win_audio.ps1` beside it, so all three are copied together; a partial copy fails at runtime, not at placement.

**Shared state, per-env script.** The scripts are per-env but their state is not: the voice pool, per-session leases, and mute flags live in one machine-wide folder (`%LOCALAPPDATA%\revoiced` on Windows, `~/Library/Application Support/revoiced` on macOS, `$XDG_DATA_HOME/revoiced` or `~/.local/share/revoiced` elsewhere). That's what makes concurrent envs behave: each session leases a **different** voice, playback serialises behind one machine-wide lock instead of several envs talking over each other, and one mute covers everything.

**The persona has to cooperate.** The hook speaks the `TL;DR:` line and nothing else, so enabling `voice` appends a section to the env's `CLAUDE.md` telling it to end every response with one. It's appended, never rewritten, so enabling `voice` on an existing env adds the section without disturbing a persona you've edited. A response with no TL;DR is blocked once with a request to add one, then allowed through — it can't loop.

**Migrating a hand-wired hook.** If an env already has a `Stop` hook you added yourself pointing at a checkout (`python "C:/…/revoiced/speak.py"`), enabling `voice` **replaces** it with the env-relative one on the next `run`. It isn't added beside it — that would speak every response twice — and it isn't left alone, since that absolute path is the coupling the capability exists to remove. Hooks that aren't a `speak.py` are never touched.

**Turning it off.** `aello edit <name> --no-voice` deregisters both hooks on the next `run` (leaving other hooks alone). For an immediate off switch that needs neither Python nor a placed env, use `aello voice mute` (or `mute --project`, `stop`, `status`) — it writes the shared state directly, so it works from any directory and applies to every env at once.

**Prerequisites.** Python 3 on `PATH`. Without `edge-tts` (`pip install edge-tts`) it falls back to the OS voice (SAPI / `say` / `spd-say` / `espeak`). Linux playback needs one of `mpv`, `ffplay`, `mpg123`, `cvlc`; macOS and Windows are covered by the OS. Ducking other audio while it speaks is Windows-only (`pycaw`) and a no-op elsewhere.

**When it doesn't speak.** Check `aello voice status` first — a global or per-project mute is the usual answer. Beyond that, the hook appends a line to `history.jsonl` in its state dir for every response it handles, recording the project, the voice used, the text, and the audio file. An entry naming a real voice means synthesis worked and the problem is playback; `system fallback voice` means `edge-tts` wasn't found and it used the OS voice; **no entry at all** means the hook never ran or the response had no `TL;DR:` line to speak. Enabled envs only pick the hook up on their next `aello run`, so an env still in a session started beforehand stays silent until restarted.

## The generated `/sync` skill

`/sync` replaces the old auto-commit-every-turn hooks. It's **manual only** (`disable-model-invocation: true`) — nothing happens until you type `/sync` inside Claude.

Crucially, the skill is **generated from the blueprint's capabilities**, not a one-size-fits-all file. A blueprint with no `github` gets a `/sync` with **no git, commit, or push sections at all** (and no `Bash` in `allowed-tools`) — it just reconciles whatever docs are enabled, locally. This keeps the agent from being told about a workflow it doesn't have.

What `/sync` does when invoked (only the enabled parts):
- **Repo health** (github) — confirm it's a git repo, check for an `origin` remote (offer `gh repo create` if missing, with confirmation), report branch / ahead-behind / status.
- **Reconcile memory, then docs** — memory is refreshed **first** (its `MEMORY.md` index and per-fact files), then each enabled, existing doc gets a two-way staleness pass: add what's missing, fix what's wrong, delete what no longer applies. Reports per file: updated / fresh / skipped.
- **Mirror env config** (github) — one-way copy of the env's `skills/`, `memory/`, and persona into the tracked per-blueprint `claude-internal/<name>/` folder (see below), staged by explicit path. Self-heals the folder (`mkdir -p`) so already-placed envs adopt it.
- **Commit + push** (github) — stage **only the files touched this session** (by explicit path, never `git add -A`), commit with a clear message ending in an `Env: <blueprint>` trailer, then `git pull --rebase origin <branch>` (absorbs the release CI's auto-bump so the push fast-forwards) and push to `origin`.

### `claude-internal/` — version-controlling the env

The env dir (`.claude-env-<name>/`) is gitignored — it holds credentials and per-machine state — so the skills, memory, and persona that define a blueprint would otherwise never reach git. The `github` cap fixes this with **`claude-internal/`**, a tracked folder at the repo root that is a **one-way mirror** of the live env dir:

```
claude-internal/
└── <name>/            # one namespace per blueprint sharing the repo
    ├── skills/            # mirror of <env>/skills/
    ├── memory/            # mirror of <env>/projects/<cwd>/memory/
    └── persona.CLAUDE.md  # snapshot of <env>/CLAUDE.md, renamed so it never auto-loads
```

The live env dir stays the **single source of truth** — `claude-internal/<name>/` is only ever written *from* it, never read back into it. It is **namespaced per blueprint** so multiple blueprints sharing one repo don't clobber each other's mirror. The persona snapshot is deliberately **not** named `CLAUDE.md` (which Claude Code would auto-load as a second persona). The folder is seeded at placement and refreshed by every `/sync`; it is **not** covered by the `.claude-env-*` gitignore line, so it commits normally.

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
2. Initializes a git repo and an initial commit if the directory has none. The bootstrap commit falls back to a synthetic `aello <aello@aello.local>` identity when the machine has no git `user.name`/`user.email`, so it lands even on a freshly configured box (an existing git identity is used unchanged).
3. If an `origin` remote already exists, reports it and stops.
4. Otherwise creates the repo with `gh repo create` (private by default; `--public` for public), sets `origin`, and pushes `main`.

Flags: `--name <repo>` (default: directory name), `--public`, `--yes` (skip confirmation). This is the aello-driven counterpart to the repo creation `/sync` only *offers* at runtime.
