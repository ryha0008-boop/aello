# Workflows

Task-shaped walkthroughs: the things people actually do with aello, start to finish. Each one is self-contained — jump to the one you need.

Concepts behind them are in [concepts.md](concepts.md); what each role is allowed to do is in [roles.md](roles.md); the skills these workflows type are in [skills.md](skills.md).

---

## Your first environment

From nothing to an agent running in a project.

```sh
aello init                      # login + your first blueprint, interactively
cd ~/code/my-project
aello run coder                 # places .claude-env-coder/ and launches Claude
```

`aello init` asks four things: name, model, persona, role. If you're putting an agent on a real repo it should own, answer `maintainer`.

What the first `aello run` does, in order:

1. Creates `.claude-env-<name>/` — the blueprint's `CLAUDE_CONFIG_DIR`.
2. Writes `settings.json`, the global persona `CLAUDE.md`, and the hooks.
3. Regenerates the seeded skills (`/sync` if the role has one, plus `/handoff`, `/note`, `/twosentences`).
4. Seeds a starter working-style memory — **only if** the env has no `MEMORY.md` yet.
5. Scaffolds the role's project files, **only the ones that are missing**.
6. Launches `claude` with the env's config dir, token, and git identity.

Nothing in step 5 overwrites content you already have. Re-running is safe and is how an env picks up changes — see [Updating an already-placed env](#updating-an-already-placed-env).

## Two agents in one repo

The case aello exists for. One blueprint owns the documentation, the others write code.

```sh
aello add api  --model opus --claude-md coder --role maintainer
aello add web  --model opus --claude-md coder --role contributor
aello add jobs --model opus --claude-md coder --role contributor

cd ~/code/service
aello run api          # in one terminal
aello run web          # in another
```

Each gets its own `.claude-env-<name>/` — separate config, separate memory, separate session history, no shared state to clobber. They see the same working tree.

Why the roles differ: `api` reconciles `README.md`, `docs/`, the project `CLAUDE.md` and `CHANGELOG.md` on `/sync`. `web` and `jobs` commit their own work and add their own `CHANGELOG.md` entries, and their `/sync` skill contains no instructions about the README or `docs/` at all — so they can't drift the docs while the maintainer isn't looking.

Afterwards, the history tells you who did what:

```sh
git log --author=web              # everything the web blueprint committed
git blame src/handler.ts          # who wrote each line, by blueprint
git log --format='%(trailers:key=Env)'
```

**Watch for:** two agents editing the same file at the same time. aello isolates *config*, not the working tree — git is still your merge tool. Give concurrent blueprints separate areas of the codebase, or run them at separate times.

## The session loop: work, checkpoint, hand off

The rhythm of a long piece of work across several sessions.

```
        ┌─ work ─┐
        │        ▼
    /sync ← /handoff ← (repeat)
        │
        ▼
     /clear  →  next session boots with the note
```

1. **Work.** Normal Claude Code.
2. **`/handoff`** when you're about to stop — writes `<name>.HANDOFF.md` at the project root: what shipped, open threads, gotchas.
3. **`/sync`** last — reconciles memory, then the docs the role owns, snapshots the env (including the handoff you just wrote), then commits and pushes. Type it yourself; the agent will not run it on its own.
4. **`/clear`** or exit.
5. **Next session** — the SessionStart hook delivers the handoff note into the new session's context and deletes the file. The agent resumes mid-thought instead of re-reading the diff.

Steps 2 and 3 are different things and you usually want both: `/sync` puts your work in git, `/handoff` puts your *state of mind* in front of the next session. A compaction saves a summary automatically; a `/clear` does not, which is exactly the gap `/handoff` fills.

**`/handoff` before `/sync`, not after.** On one machine the order barely matters — the note sits at the project root either way and the next boot finds it. It matters the moment a second machine is involved: the only copy of the note that ever reaches git is the snapshot `/sync`'s mirror step takes, so a note written *after* `/sync` is never committed and stays on the machine that wrote it. The order above works for both cases, which is why it's the one documented.

## One env, two machines

Working the same blueprint from a desktop and a laptop — and the two need not run the same OS, since every path is derived per machine rather than carried in the mirror.

What crosses between them is the tracked `claude-internal/<name>/` folder: memory, skills, the persona snapshot, and the resume note. What does **not** cross is `config.toml` (so the blueprint is added once per machine), your auth, and the contextdb — transcripts and session history stay on the machine that produced them.

**First time on the second machine:**

```sh
git clone <repo> && cd <repo>
aello add <Name> --model <same> --role <same>   # config.toml is per machine
aello login                                     # auth is per machine
aello run <Name>                                # prints "Restored '<Name>' from claude-internal/"
```

Use the **same blueprint name, with the same casing** — the hooks look for `<Name>.HANDOFF.md` exactly, so a case difference is a note nobody reads on a case-sensitive filesystem. Give it the same role too; the role generates the `/sync` skill, so a mismatch quietly changes what checkpointing does.

**Every switch after that:**

```
  machine A                    git                    machine B
  ─────────                    ───                    ─────────
  work                                                
  /handoff  ──┐                                       
  /sync     ──┴──▶ push ──────────▶ pull ──▶ aello restore <Name>
                                                      aello run <Name>
                                                      work → /handoff → /sync → push
       aello restore <Name> ◀── pull ◀──────────────────────┘
```

1. **Leaving a machine:** `/handoff`, then `/sync`. The push carries your memory, skills and the resume note.
2. **Arriving at a machine:** `git pull`, then **`aello restore <Name>`**, then `aello run <Name>`. The restore copies the pulled memory and skills into the gitignored env dir and puts the resume note back at the project root, where the next launch delivers and deletes it.

**Do not skip the restore.** `aello run` seeds an env from the mirror only when there is no env dir at all — a live env is never contradicted by a snapshot — so on the machine that already has one, pulling alone changes nothing the agent can see. The restore is additive and safe to run whenever you're unsure: memory and skills are merged rather than replaced, a persona that differs is reported and left alone, and only the resume note is overwritten.

If you forget it, nothing is destroyed but nothing is delivered either. The next `aello run` will notice the pulled notes it has no copy of and name them, telling you to restore — memory is deliberately never pruned from the mirror, precisely so a launch cannot delete the other machine's work.

One content-level caveat that aello can't fix for you: memory notes and handoffs written on one OS often name absolute paths (`C:\Users\...`), which read as nonsense on the other. Prefer paths relative to the repo root when a note is meant to be read from both.

## Telling another agent something

When the blueprint you're in breaks — or notices — something another blueprint owns.

```
/note web
```

That writes `web.NOTE.md` at the target's project root: who wrote it, what they were doing, the problem, what the target needs to fix. `web` reads it on its next boot and deletes it.

Two things to get right:

- **Casing must match the blueprint name exactly.** The hook looks for `<Name>.NOTE.md`. On a case-sensitive filesystem, `/note Web` for a blueprint called `web` is a note nobody ever reads.
- **The target need not share your repo.** The skill resolves the target's own project root and writes there. A note dropped in *your* repo for an agent that works in another one is a dead letter that looks delivered.

Each note overwrites the last — it's an inbox holding one current message, not a queue.

## Resuming a session

```sh
aello run coder --resume            # most recent session
aello run coder --resume <id>       # a specific one
```

Resuming re-places the env first (same six steps as above), so a resumed session gets any bundled-file changes from a newer aello. Session ids come from the env's own history; each blueprint has its own.

## Updating an already-placed env

Placement is **idempotent and self-healing**: every `aello run` rewrites the bundled files — hooks, seeded skills, settings registrations — while never touching your persona, your memory, or any project file that already exists.

So the workflow for "I changed the blueprint" is just: run it again.

```sh
aello edit coder --role maintainer     # or --model, --claude-md, --rename
cd ~/code/my-project
aello run coder                        # picks up the change
```

The same is true after upgrading aello itself: a new version's hooks and skill templates reach an env the next time it runs. An env placed months ago catches up in one launch.

**The exception is a skill you hand-edited.** Regeneration will overwrite it. Pin it:

```sh
touch .claude-env-coder/skills/sync/.aello-keep
```

aello then neither regenerates nor deletes that skill. The trade is that it stops tracking the role — you maintain it by hand from then on. Delete the marker to go back.

## Changing what an agent is responsible for

```sh
aello list                                  # ROLE column shows the current one
aello edit web --role maintainer            # promote
aello edit api --role contributor           # demote
```

Takes effect on the next `aello run`. Promoting scaffolds the newly-owned files if they're missing and adds their sections to `/sync`; demoting removes those sections. Neither deletes a file that already exists — dropping the `docs` duty does not drop `docs/`.

## Renaming a blueprint

```sh
cd ~/code/my-project
aello edit oldname --rename newname
```

This moves the placed `.claude-env-oldname/` env dir, its `.aello.toml`, and the `claude-internal/oldname/` mirror **in the current directory only**. aello keeps no registry of where a blueprint has been placed, so if it lives in other projects too, run the same command in each of them. Until you do, those directories keep the old name on disk and `aello run newname` there would quietly scaffold a fresh, empty env beside the old one.

## Putting a project on GitHub

```sh
cd ~/code/my-project
aello github-setup                     # private by default
aello github-setup --public --yes
```

Checks `gh` is authenticated, initializes a repo and an initial commit if there isn't one, then `gh repo create --source=. --remote=origin --push`. If `origin` already exists it reports it and stops. On a machine with no git identity configured, the bootstrap commit uses a synthetic `aello <aello@aello.local>` identity so it still lands.

This is the up-front version of the offer `/sync` makes at runtime when it finds no remote.

## Silencing the voice

Every env speaks the trailing `TL;DR:` line of each response. There's nothing to enable, and the off switch is at runtime — not placement:

```sh
aello voice mute                 # every env, and cut off what's playing now
aello voice mute --project       # just this project
aello voice unmute
aello voice stop                 # stop the current sentence, stay unmuted
aello voice status               # mute state, voice pool, HOOK_VERSION
```

The state is machine-wide and shared by every env, so these work from any directory whether or not a blueprint is placed there. In the TUI, `M` toggles the global mute. Mechanism and troubleshooting: [voice.md](voice.md).

## Checking what an env has spent

```sh
aello tokens                 # every env + the current 5-hour window
aello tokens AlgoMainDev     # one env
aello tokens --sessions      # per-session breakdown
aello tokens --json          # for scripts
```

Nothing needs enabling — it reads the transcripts contextdb has been archiving all along, so it covers history recorded before the feature existed. `T` in the TUI shows the same data with the window across the top, envs down the left, and the selected env's buckets, per-model split and session list on the right.

Read the output knowing two things. The **cost is an estimate at list API rates**, not a bill — an env runs on a subscription, where no per-token charge exists — and the **5-hour percentage is against your own busiest 5-hour block, not your plan's quota**, because the quota appears in no transcript. Both are labelled in the output; [tokens.md](tokens.md) explains why.

Worth knowing before you draw conclusions: cache reads usually dwarf everything else, so a "wordy agent" is almost never where the tokens went.

## Removing an environment

```sh
aello remove coder                       # forget the blueprint, leave the env dir
aello remove coder --purge               # also delete .claude-env-coder/ + claude-internal/coder/ here
aello remove coder --yes --purge         # skip the confirmation
```

`--purge` only touches the **current** project. Env dirs in other projects survive and are unreferenced afterwards — delete them by hand if you want them gone.

## Migrating an existing repo onto aello

Has enough gotchas to deserve its own page: [migrate.md](migrate.md).

---

## Adding a workflow to this page

New sections are welcome. Two conventions keep it usable:

- **Lead with the task, not the feature** — "Two agents in one repo", not "The contributor role". Someone scanning this page is looking for their situation.
- **Say what goes wrong.** The `**Watch for:**` and gotcha notes are the parts that pay for themselves; a walkthrough with no failure mode in it usually means it hasn't been run.

Anything larger than a section belongs in its own `docs/*.md` — drop the file in and it appears in `aello docs`, the TUI reader (`?`) and the docs site automatically, with no code change.
