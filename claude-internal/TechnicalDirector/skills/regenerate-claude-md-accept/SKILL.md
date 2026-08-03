---
name: regenerate-claude-md-accept
description: Install an agreed global persona into a target env — writes its CLAUDE.md, flips the blueprint to `custom` and bumps the generation. Invoke manually with /regenerate-claude-md-accept <env> followed by the full persona.
disable-model-invocation: true
allowed-tools: Read, Write, Bash
---

# /regenerate-claude-md-accept — install an agreed persona

> **Only the user runs this.** It happens when they type
> `/regenerate-claude-md-accept`, and at no other time. If you are reading this
> file because *you* decided to — to see what it does, or to carry out its steps
> yourself — then stop and do neither. Following these instructions **is**
> running the skill, whichever route you took to them. Say the skill exists and
> let the user invoke it.

The other half of `/regenerate-claude-md`. That skill *proposes* and writes
nothing; this one installs what the user finally agreed to, after however many
rounds it took. The split is the point: generation is cheap and reversible,
installation overwrites a file read on every turn of every future session.

## What it does

Three things, together, via one command:

1. Replaces `<target-env>/CLAUDE.md` with the persona given.
2. Sets that blueprint's `claude_md = "custom"` in aello's config, so `place`
   stops seeding a template over it on the next run.
3. Bumps `<target-env>/persona.gen` — `gen1 2026-08-03`, `gen2 …` and so on.

Together those answer both questions the user asked of the system: *which envs
have a real persona* (config says `custom`) and *which generation each is on*
(the sidecar, sitting beside the file it describes).

## Input

    /regenerate-claude-md-accept <env-dir-or-blueprint-name>

followed by the complete persona — normally pasted in after the command.

- **The persona is the whole file.** Not a diff, not the changed sections. What
  you write replaces `CLAUDE.md` entirely.
- If the user passes a path like `…/.claude-env-AlgoMainDev`, the blueprint name
  is the part after `.claude-env-` and the project is that directory's parent.
- If they pass a bare blueprint name, ask which project — a blueprint can be
  placed in more than one, each with its own persona and its own generation.

## Steps

### 1. Resolve the target, and refuse to guess

Establish the blueprint name and the project directory holding
`.claude-env-<name>/`. Confirm the env dir exists. If it does not, say so and
stop — there is nothing placed there to give a persona to.

If you cannot tell which project is meant, **ask and stop**. Writing the right
persona into the wrong env is silent: both files look plausible afterwards.

### 2. Check what you are about to overwrite

Read the current `<env>/CLAUDE.md` and `<env>/persona.gen` if present. Report,
in two or three lines: the current generation, roughly what is being replaced,
and anything in the outgoing file that is **not** in the incoming one.

That last part matters. The proposal was written against the persona as it was;
if the user has edited the file in the target env since — which is exactly what
"I will consult the actual env" produces — those edits are about to be lost.
Say what they are and let the user decide before writing.

### 3. Write it

Save the persona to a file, then hand it to aello:

    aello persona <name> --from <file> --project <project-dir>

Use the command rather than editing anything by hand. It does all three writes
together, and the config edit in particular has to go through aello: `Config` is
serialized from its struct, so a key written into `config.toml` by anything else
is dropped the next time aello saves.

Write the persona through a **file**, never by echoing it into a shell — it is a
multi-line markdown document with backticks and quotes, and PowerShell 5.1
mangles exactly that.

### 4. Verify, then report

Do not trust the command's exit code alone. Read back:

- `<env>/CLAUDE.md` — first and last lines match what was agreed.
- `<env>/persona.gen` — the generation went up by one.
- `aello list` — that blueprint now shows `custom`.

Then report in three lines: which env, which generation, and what changed from
the previous one. If the env belongs to another blueprint that is currently
running, mention that the new persona reaches it on its next session, not now.

## Notes

- **This never runs unasked.** A persona is the file an agent reads on every
  turn; installing one is the user's decision, made once they are satisfied.
- **`custom` is one-way in practice.** Once set, aello stops writing a persona
  for that env, so the file in the env dir is the only copy. It reaches git
  through `claude-internal/<name>/persona.CLAUDE.md` on the next `/sync` — which
  is the user's to run, not yours.
- **Don't add the TL;DR section back.** Every env carries that instruction on a
  per-turn hook now, along with the conciseness and anti-sycophancy rules. A
  copy in the persona is redundant.
- **A blueprint pointing at a path** (rather than `coder`/`none`/`custom`) has a
  persona file the user maintains, sometimes shared by several envs. Do not
  convert one to `custom` as a side effect of this skill — propose the edit to
  that file instead, and say that it lands in more than one env.
