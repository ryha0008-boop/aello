# Upgrading to 0.2

Read this once per environment, the first time you run aello 0.2 in a project that was set up before it. It takes about a minute and there is probably nothing to do.

> Not to be confused with [migrate.md](migrate.md), which is about putting an *existing repo* onto aello for the first time. This page is about an existing *aello setup* meeting 0.2.

## The one change that matters

A blueprint used to carry five independent capability flags — `project_md`, `github`, `changelog`, `docs`, `readme`. It now carries a single **role**:

| Role | Owns | `/sync` |
|---|---|---|
| `maintainer` | project `CLAUDE.md`, `CHANGELOG.md`, `docs/`, `README.md`, git | full |
| `contributor` | its own code and its own `CHANGELOG.md` entry | git only |
| `standalone` | nothing outside its own session | none — no `/sync` skill |

Full detail in [roles.md](roles.md). The short version of *why*: grouped by repo, the flags were never independent — one blueprint held everything and the rest held git duties only. Three roles keep that distinction and drop 29 combinations nobody used.

## What you need to do

**Almost certainly nothing.** Your config migrates itself the first time 0.2 loads it. Confirm with:

```sh
aello list
```

The last column is now `ROLE`. If it says what you'd expect, you're done.

The mapping applied to your old flags:

- anything that maintained prose (`project_md`, `docs`, or `readme`) → **maintainer**
- anything left holding only git duties (`github`, `changelog`) → **contributor**
- nothing enabled → **standalone**

The old `[blueprints.caps]` table is read once and dropped the next time aello saves the config.

### The one case that changes behaviour

A blueprint that had **some but not all** of the prose capabilities is now a **maintainer** — so `/sync` will start reconciling the files it previously skipped. If a blueprint had `github + changelog + docs` but deliberately not `readme`, it now maintains the README too.

If that isn't what you want:

```sh
aello edit <name> --role contributor
```

Nothing is deleted either way. Dropping a duty does not drop the file.

## What will look different

- **`aello list`** ends in `ROLE` instead of a comma-separated capability list.
- **Your `/sync` skill is regenerated** on the next `aello run`, as always. A contributor's copy no longer contains any README or `docs/` instructions — that's the point, not a bug. If you hand-edited yours, pin it first with an empty `<env>/skills/sync/.aello-keep` (see [skills.md](skills.md)).
- **If your blueprint became `standalone`**, its `/sync` skill is *removed* on the next run. `/handoff`, `/note` and `/twosentences` are unaffected — they're universal.
- **A `/sync` right after upgrading will show skill diffs** in `claude-internal/<name>/`. Expected: the mirror is catching up with the regenerated skills.

## Removed flags

These are gone from `aello add` and `aello edit`:

```
--project-md   --github   --changelog   --docs   --readme
--no-project-md   --no-github   --no-changelog   --no-docs   --no-readme
```

They fail loudly rather than being ignored:

```
$ aello add demo --model opus --github
error: unexpected argument '--github' found
```

So a script or alias still using them will stop, not silently create a blueprint with the wrong scope. Replace with `--role maintainer|contributor|standalone`.

## One real hazard

**Do not run a pre-0.2 aello against a config that 0.2 has already saved.**

An old binary has no `role` field. It reads every blueprint's capabilities as all-false, treats them as standalone, and `place()` then **removes the `/sync` skill** from every env it touches. It won't error — it looks like a normal run.

If you have aello installed on more than one machine, or a checkout you build from, upgrade them all before running any of them. Check with `aello --version`.

## Not changed

- **The voice.** Every env still speaks unconditionally; it was never a capability in 0.2's sense and nothing about it moved. See [voice.md](voice.md).
- **Auth, memory, contextdb, the env dir layout, git attribution** — all identical.
- **`/handoff`, `/note`, `/twosentences`** — still seeded for every blueprint whatever its role.
- **Your persona.** The global `<env>/CLAUDE.md` is never clobbered, by any version.

## Also new in 0.2

- **The docs are online**: <https://ryha0008-boop.github.io/aello/docs/> — generated from the same files the binary embeds, so `aello docs` and the site never disagree.
- **Four new pages**: [workflows.md](workflows.md) (task-shaped walkthroughs), [skills.md](skills.md), [development.md](development.md), [troubleshooting.md](troubleshooting.md).
- `docs/capabilities.md` is now [roles.md](roles.md) — `aello docs roles`.

## 0.2.8: personas became three values

A second migration, same shape as the roles one — it happens on load and there is nothing to do.

`claude_md` used to be a template name (`coder`, `sysadmin`) or a path. It is now `coder`, `none`, `custom`, or a path:

- **absent → `none`.** A blank persona is now stated rather than implied, which is what lets `custom` mean something.
- **`sysadmin` → `custom`.** That template is gone. The env that used it already holds the text — aello never overwrites a persona — so nothing is lost, and `custom` says what is true: the persona lives in the env dir now.
- **`coder` and paths are untouched.** Paths still work, including one file shared by several blueprints.

`custom` means aello writes no persona for that env; the file in the env dir is authoritative. That is what `aello persona <name> --from <file>` sets when you accept a persona written for the project, alongside recording the generation in `<env>/persona.gen`.

**One hazard, and it is the reverse of the usual one.** Once a config has been migrated *and saved*, an aello older than 0.2.8 cannot launch a blueprint whose persona is `none` or `custom` — it reads the value as a filename and aborts with "unusable persona". Merely reading the config is safe; it persists on the next save. Keep the binary current rather than downgrading, since the migration only runs forward.
