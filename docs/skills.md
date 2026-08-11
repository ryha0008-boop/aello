# Seeded skills

Every placed env gets four slash commands written into `<env>/skills/<name>/SKILL.md`. Three are universal; `/sync` depends on the blueprint's [role](roles.md).

| Skill | Seeded for | What it is |
|---|---|---|
| [`/sync`](#sync--project-checkpoint) | maintainer, contributor | Checkpoint: reconcile memory + the docs this role owns, then commit and push |
| [`/handoff`](#handoff--a-note-to-your-next-session) | every blueprint | A self-contained resume note for your *next* session |
| [`/note`](#note--a-message-to-another-environment) | every blueprint | A message to a *different* environment |
| [`/twosentences`](#twosentences) | every blueprint | Condense the last response to exactly two sentences |

## They are yours to run, not the agent's

All four are **manual-only**, and that is enforced twice:

- `disable-model-invocation: true` in the frontmatter blocks the Skill-tool path.
- A banner under the H1 blocks the other one — an agent that opens the `SKILL.md` with `Read` and works through the steps **has run the skill**, whichever route it took.

The second exists because the first wasn't enough. A `/sync` was once carried out by hand, unasked, and reported as though the command had run — which is worse than not doing it, because you're then told a checkpoint exists when it doesn't. If an agent thinks a checkpoint is due, the correct behaviour is to say so and let you type it.

## They are regenerated on every run

`aello run` rewrites all four, unconditionally. That's what lets a role change, or a newer aello, reach an env placed months ago.

The cost: a skill you hand-edited is silently restored on the next run — and if the role mirrors the env into `claude-internal/`, the generated version gets committed over your custom one. Pin it instead:

```sh
touch <env>/skills/<skill>/.aello-keep
```

An empty marker file beside the skill. `place` then neither regenerates **nor removes** it — the removal branch matters too, since a hand-written `/sync` isn't stale just because the blueprint became `standalone`. The marker lives in the env dir rather than in `config.toml` so it travels with the env and is visible where the editing happens. A kept skill stops tracking the role; delete the marker to go back to the generated version.

---

## `/sync` — project checkpoint

Generated per blueprint from its role, so it contains only what that blueprint may actually do. A contributor's copy has no README or `docs/` instructions in it at all; a standalone blueprint has no `/sync` file and no `Bash` in `allowed-tools`.

Sections, in order:

1. **Repo health** — confirm it's a git repo, check for `origin` (offer `gh repo create` if missing, never without confirmation), report branch, ahead/behind, and a short status.
2. **Reconcile memory, then docs** — memory **first**, so the checkpoint captures what the session learned before anything else is written. Then each doc the role owns gets a two-way pass: add what's missing, correct what's wrong, delete what no longer applies. Reported per file as updated / already-fresh / skipped.
3. **Mirror the env** — copy `skills/`, memory, the persona and any resume note written this session into the tracked `claude-internal/<name>/` folder (or wherever `--mirror-dir` points, for a repo whose memory should not be public), staged by explicit path after a read for credentials. Skills are pruned to match the env; memory is only ever added to, because on a repo worked from two machines a mirror-only note is the other machine's.
4. **Commit + push** — stage only what this session touched, by explicit path; commit with an `Env: <name>` trailer; `git pull --rebase` before pushing so the push fast-forwards.

Two rules inside it are worth knowing about, because they're the ones that bite:

- **Never `git add -A`.** A blanket stage sweeps up another env's in-flight work and unrelated tooling scaffolding.
- **Never stage `*.HANDOFF.md` or `*.NOTE.md`.** They're created during the session and deleted on the next boot, so "stage what you created this session" would otherwise catch them. They are deliberately not gitignored — only `.claude-env-*` is — so nothing else stops it. This is about the files **at the project root**: the mirror step's `claude-internal/<name>/handoff.md` snapshot is tracked on purpose, and is the only way a resume note reaches a second machine.

Full detail, including `claude-internal/` and the `VERSION` convention: [roles.md](roles.md).

## `/handoff` — a note to your next session

Writes `<name>.HANDOFF.md` at the project root. Sections: read-first pointers (persona, memory index), what shipped this session with commit shas, open threads and the concrete next action, and gotchas.

It is written to be read after a **`/clear`**, not a compaction. A compaction leaves a summary; a clear leaves nothing. So the note assumes a reader who has never seen the conversation and reads only this file plus the pointers it names.

The filename carries the blueprint name so co-located blueprints don't clobber each other's. The SessionStart hook delivers it into the next session as context and **deletes it** — which is what makes "read on boot, then deleted" true, and what stops the SessionEnd hook re-archiving the same stale note forever.

Untracked, transient, and never committed.

## `/note` — a message to another environment

`/note <target>` writes `<target>.NOTE.md` at the **target's** project root — that env's inbox. It reads the note on its next boot and deletes it. Each note **overwrites** the last: one current message, not a queue.

Distinct from `/handoff`: a handoff is a note to yourself for next time, a note is a message to a different agent — you touched something it owns, or hit a problem on its side.

Two failure modes the skill is written to prevent:

- **The target may not share your repo.** The skill resolves the target's own project root and writes there. A SessionStart hook only ever reads its *own* project root, so a cross-repo note left in your repo is a dead letter that looks delivered. (The skill originally assumed one shared repo and reported a target it couldn't find locally as a typo — which misdiagnosed the ordinary multi-repo case twice in one day.)
- **Casing must be the blueprint's canonical form.** The hook matches `<Name>.NOTE.md` exactly; wrong case is silent on a case-sensitive filesystem.

The skill reports the **full path** it wrote to, so a note sent into another repo is visibly that.

## `/twosentences`

Condenses the previous assistant response into exactly two sentences, output with no other text. A pure text task — its `allowed-tools` is deliberately empty.

---

## Writing your own

Nothing stops you adding skills to an env by hand: a `SKILL.md` under `<env>/skills/<name>/` is picked up like any other. Two things to keep in mind:

- **Name it something aello doesn't generate**, or pin it with `.aello-keep`. The four names above are rewritten on every run.
- **Set `disable-model-invocation: true`** if it should only run when you type it, and say so in the body as well — the frontmatter flag alone has already proven insufficient at least once.

A skill you want in *every* env is a different thing: that's a change to `templates.rs`, not a file you copy 39 times. See [development.md](development.md).
