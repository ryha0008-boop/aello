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
| `.gitignore` line `.claude-env-*` | ✅ | ✅ | ✅ |
| `.gitattributes` (CRLF normalize) | ✅ | ✅ | — |
| `VERSION` + `.github/workflows/version.yml` (patch-bump CI) | ✅ | ✅ | — |
| tracked `claude-internal/<name>/` mirror | ✅ | ✅ | — |
| `.githooks/pre-commit` (blocks committed key material) | ✅ | ✅ | — |
| `.github/workflows/ci.yml` (tests + dependency audit) | ✅ | ✅ | — |
| `.github/renovate.json` (update policy) | ✅ | ✅ | — |
| `CHANGELOG.md` (`## [Unreleased]`) | ✅ | ✅ | — |
| project-root `CLAUDE.md` | ✅ | — | — |
| `docs/` directory | ✅ | — | — |
| `README.md` | ✅ | — | — |

The ignore line is the one row every role gets, including `standalone`. It used
to be `github`-only, as tidiness — until it was pointed out that a Claude env
with no shared token configured holds Claude Code's own `.credentials.json`, and
`standalone` is the default role. The line costs nothing in a project with no
git, and a blueprint with no git duties still shares the working tree with one
that has them.

The global persona (`--claude-md`) is separate from the role — it writes the env-level `CLAUDE.md` once, and no role rewrites it. The single exception is `aello persona`, which exists to replace one deliberately and then sets the blueprint to `custom` so nothing writes over it again. So is the voice, below: it is not a role setting and there is nothing to enable.

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
- **Mirror env config** — copy the env's `skills/`, `commands/`, `memory/`, persona and (if one was written this session) the `<name>.HANDOFF.md` resume note into the tracked per-blueprint `claude-internal/<name>/` folder (see below), staged by explicit path. Self-heals the folder (`mkdir -p`) so already-placed envs adopt it. Because the handoff snapshot is taken here, **`/handoff` has to run before `/sync`** — a note written afterwards misses the commit and stays on this machine.
- **Commit + push** — stage **only the files touched this session** (by explicit path, never `git add -A`), commit with a clear message ending in an `Env: <blueprint>` trailer, then `git pull --rebase origin <branch>` (absorbs the release CI's auto-bump so the push fast-forwards) and push to `origin`.

### `claude-internal/` — version-controlling the env

The env dir (`.claude-env-<name>/`) is gitignored — it holds credentials and per-machine state — so the skills, commands, memory, and persona that define a blueprint would otherwise never reach git. Any role with git duties fixes this with **`claude-internal/`**, a tracked folder at the repo root that mirrors the live env dir:

```
claude-internal/
└── <name>/            # one namespace per blueprint sharing the repo
    ├── skills/            # mirror of <env>/skills/ — one-way, pruned
    ├── commands/          # union with <env>/commands/ — never pruned
    ├── memory/            # union with <env>/projects/<cwd>/memory/ — never pruned
    ├── persona.CLAUDE.md  # snapshot of <env>/CLAUDE.md, renamed so it never auto-loads
    └── handoff.md         # snapshot of <name>.HANDOFF.md — lowercase, so *.HANDOFF.md
                           #   ignore rules don't catch it; this is how a resume note
                           #   reaches another machine
```

For generated content the live env dir is the **single source of truth** — `claude-internal/<name>/` is only written *from* it, and a mirrored skill the blueprint no longer seeds gets pruned. **Memory and commands are the exceptions, and deliberately so.** Memory has two writers as soon as a second machine is involved, so the mirror only ever gains notes: a note present in the mirror and absent from this env dir is the other machine's committed work, and deleting it on a launch destroyed it silently, staging the deletion in the same breath. Commands (`<env>/commands/*.md`, your own slash commands) are pruned from neither side for a simpler reason: aello writes none of them, so there is no generated version to fall back on and the mirror is the only copy that crosses a machine. A launch that finds mirror-only notes or commands prints their names and points at `aello restore`. Removing one for real is a `git rm`.

Reading the mirror back into an env happens in two places:

- **A clone.** When `aello run` finds the mirror present and no env dir beside it, it restores the env — skills, commands, memory, persona and any waiting resume note — before seeding anything, then carries on mirroring as usual. Without that step the first run on a second machine seeded a bare env and the prune pass deleted every tracked memory note and hand-kept skill the bare env did not have, silently, since the mirror is written from code that had no reason to look.
- **`aello restore <name>`.** The same thing for an env dir that already exists — run it after pulling work another machine pushed, because `aello run` deliberately will not touch a live env from a snapshot. It is additive: memory and skills are unions, a persona that differs is reported and left alone (`aello persona` is how you replace one on purpose), and only the resume note is replaced, since the local one has already been snapshotted into git by this machine's own `/sync`.

The folder is **namespaced per blueprint** so multiple blueprints sharing one repo don't clobber each other's mirror. The persona snapshot is deliberately **not** named `CLAUDE.md` (which Claude Code would auto-load as a second persona). The folder is seeded at placement and refreshed by every `/sync`; it is **not** covered by the `.claude-env-*` gitignore line, so it commits normally.

The skill is re-generated on every `aello run`, so changing a blueprint's role updates its `/sync` on the next placement. A `standalone` blueprint gets no `/sync` skill at all.

### What stops a secret reaching the mirror

The mirror is a session's own memory, and memory notes are exactly where a session writes down a credential it just used. `/sync` stages the folder **by path**, so nothing in that chain reads what is in it — and at least one repo on this account is public, which makes it a publish rather than a backup.

Two guards, deliberately narrow:

- **A `pre-commit` hook**, seeded to `.githooks/pre-commit` alongside the other `github` scaffolding, which blocks armored private keys, PuTTY keys, real `.env` files (`.env.example` passes), certificate and keystore bundles, `.netrc`/`.pgpass`/`.htpasswd`, port-knock sequences, and non-placeholder provider API keys. It is enabled with `git config core.hooksPath .githooks`, which aello re-runs **on every placement** — that setting is per-clone local config and does not travel with a pull, so a fresh clone otherwise has the file and no guard. If the repo already points `core.hooksPath` somewhere else, aello leaves it alone.
- **A step in `/sync`** telling the agent to read the mirror for credentials *before* staging it and to refuse rather than warn — the hook catches what reaches the index, but by then the finding is already in a commit message and a mirror refresh.

### Sending the mirror somewhere else

The guards above stop a *credential* reaching git. They do nothing about the rest of the mirror, which is the whole of an env's memory, persona and handoff — and in a **public** repo that is a publish, not a backup.

Deleting the mirror is not the answer: being in git is exactly what makes an env restorable from a second machine (`aello restore`). So the destination moves instead.

```sh
git clone git@github.com:you/aello-internal.git ~/aello-internal   # private
aello edit TechnicalDirector --mirror-dir ~/aello-internal
aello edit TechnicalDirector --mirror-dir -                        # back to the default
```

`--mirror-dir` takes a **path to an existing git working tree**, not a URL — aello never clones or pushes on your behalf, `/sync` is what commits. It is rejected at `edit` time if the path is missing or is not a git repo, because a mirror writing into a plain directory looks identical to one that worked: files appear, nothing is ever committed, and the memory quietly stops crossing machines. The `<blueprint>/` component is still appended, so several blueprints can share one destination.

With a destination set, the generated `/sync` **drops the in-project `git add claude-internal/…` line entirely** and grows a commit-and-push step against the destination repo instead. It does not fall back to mirroring here if the destination is missing — it stops and says so, since a fallback is the exact leak the setting exists to prevent. Add `claude-internal/` to the public repo's `.gitignore` and `git rm -r --cached` whatever is already tracked; existing history is unaffected, so check it separately if that matters.

Without a destination, `/sync` runs `gh repo view --json visibility` before staging and **stops if the repo is public**, naming `--mirror-dir` as the way forward. "Cannot tell" — no `gh`, no GitHub remote — is reported as unanswered rather than assumed private. That is the safe-by-default half: a future public repo is covered without anyone having to remember.

**What they deliberately do not check: IP addresses, hostnames, machine paths and domains.** Those are identifiers, not secrets, and flagging them produces a long report that gets skimmed once and then bypassed with `--no-verify` — at which point the real check is gone too. The scope is credentials that spend money and passwords whose only value is being unknown.

The hook file carries an `aello-pre-commit v<N>` marker. A copy with an older marker is upgraded on the next placement, so a widened pattern reaches projects scaffolded months ago; a `pre-commit` **without** the marker is somebody's own hook and is never touched. It is written with LF regardless of your checkout, and `.githooks/* text eol=lf` is appended to the project's `.gitattributes` — hooks are run by `sh`, a CRLF one silently fails to execute, and the file has no extension so a `*.sh` rule does not cover it.

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

## Dependency hygiene

A repo that grew organically has its tests and its dependency audit running **only where somebody remembers to type them** — the developer's desktop, never the server. That is not a property of any one project, so aello establishes it rather than letting each repo rediscover it.

The policy, decided once so agents stop re-deriving it per project:

> Ranges in the manifest, exact versions in the lock. Deploys install from the lock and never resolve. Nothing automerges. **A lock is verified by installing it, not by reading it.**

Three pieces, all seeded for roles with git duties:

- **`.github/workflows/ci.yml`** — tests plus `pip-audit` / `npm audit` on every push and PR. **Stack-agnostic like `version.yml`, but by detecting at run time rather than at seed time**: aello does not know a project's ecosystem when it places into it, and a guess baked in at placement seeds a workflow that fails forever in every repo of the other kind. A repo with neither a Python nor a Node manifest runs the job and reports that there was nothing to do — a true answer, not a green tick earned by skipping. **The audit fails the build**; an advisory that only prints is one nobody reads. Pin `python-version` to the interpreter the project is *deployed* on, not the developer's — a lock compiled against one and installed on another can resolve differently, which makes CI green and the server wrong.
- **`.github/renovate.json`** — grouped minor/patch weekly, majors always their own PR, security updates off-schedule, **nothing automerged**, editing the manifest and never a generated lockfile. It does nothing at all until the Renovate **GitHub App** is installed, which is a manual step aello cannot perform; placement says so once rather than reporting it configured. **Installing it is necessary and not sufficient** — Mend's onboarding wizard defaults to *Scan Only*, which sets Renovate to `silent`: it runs jobs on schedule and creates no PRs, no issues and no dependency dashboard, which from outside is indistinguishable from never having installed it. See [troubleshooting.md](troubleshooting.md). Install "Renovate" — mend.io's wizard offers "Mend Application Security" first, which needs a paid licence.
- **A `/sync` step that asserts a lock exists and refuses to create one.** Python → `uv.lock` or a *compiled* `requirements.txt`; Node → a committed `package-lock.json`. A manifest with no lock **is the finding**. Compiling a lock changes what installs, and on a live system that is a deploy — not something a checkpoint does unasked.

Both files are written **only when absent**, so a project that has tuned its own keeps it. Neither is regenerated from the role the way a skill is.

### What "a lockfile exists" does not mean

A hand-written `requirements.txt` listing only the packages the code imports directly is the **same finding as having none**, even though the file is there. Everything transitive stays unpinned and resolves to whatever is newest that day. Measured in one 14-month-old project on 2026-08-11: a **beta** release of a signing library had installed itself into the code path that signs every order, and nobody chose it. In the same repo a pin read off the developer's desktop was *older* than the server, so installing the manifest moved production backwards on every run. Check whether the transitive set is pinned, not whether the file is present.

Adding CI to that project found two more on its first two runs — one suite silently collecting **384 of 420 tests** because an undeclared test-only dependency made an import raise rather than degrade (the suite still printed OK), and four tests that passed only by reading a *gitignored* file which happened to exist on that machine, with a skip guard that checked the wrong verdict and so never fired. Neither is exotic; both are invisible until a second machine runs the suite.

**A committed lock is not a deployed dependency.** If a project's deploy script contains no install step, the lock reaches the server and changes nothing until someone installs it by hand — and a running daemon keeps its loaded modules until it is restarted. Do not report "the lock is committed" as "the dependency is deployed".

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
