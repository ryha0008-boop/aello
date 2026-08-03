---
name: regenerate-claude-md
description: Assess a grown project and propose a rewritten GLOBAL persona (its env CLAUDE.md), printed for review rather than written. Invoke manually with /regenerate-claude-md <directory>.
disable-model-invocation: true
allowed-tools: Read, Grep, Glob, Bash
---

# /regenerate-claude-md — propose a project-shaped global persona

> **Only the user runs this.** It happens when they type `/regenerate-claude-md`, and at no
> other time. If you are reading this file because *you* decided to — to see what
> it does, or to carry out its steps yourself — then stop and do neither.
> Following these instructions **is** running the skill, whichever route you took
> to them. Say the skill exists and let the user invoke it.

Every env starts on one of two day-one defaults — `coder` for a coding project,
`none` for anything that isn't one. Both are cookie cutters: correct on day one
and steadily less so. The project grows, working agreements accumulate, the same
mistakes stop being made for reasons nobody wrote down. This skill reads what
the project has become and proposes what its agent's **global** persona should
say now.

Its output is a *proposal*. When the wording is finally right — usually after
several rounds, and after the user has consulted the target env itself — they
run **`/regenerate-claude-md-accept`**, which is what actually installs it and
flips the blueprint to `custom`.

You have **no `Write` tool here, deliberately.** Your output is a proposal the
user carries to that project's env and discusses there. Do not write, edit, or
offer to write any file. Print the proposed persona and stop.

## What "global" means here, and the test that keeps you honest

Two layers, and this skill only touches the first:

| | Global persona | Project CLAUDE.md |
|---|---|---|
| Lives in | `<project>/.claude-env-<name>/CLAUDE.md` | `<project>/CLAUDE.md` |
| Answers | *how should this agent work?* | *what is true about this codebase?* |
| Example | "run the suite before claiming done" | "`cargo test` takes 40s; the digest test prints the new value" |
| Maintained by | a human, rarely | `/sync`, continuously |

**The test:** could this sentence survive the project's entire codebase being
rewritten in another language? If yes, it is persona. If it names a file, a
command, a version, a module or a service, it is project — and it belongs in the
other file, which is already maintained. Putting it here duplicates a doc that
updates itself, and the copy here will be the one that goes stale.

The persona *is* allowed to be shaped by the project. "This project ships to
users who cannot roll back, so prefer a smaller change you can verify" is a
working rule, not a fact — it names nothing. That is the altitude you want.

## Steps

### 1. Resolve the target

The argument is a project directory. If none was given, ask and stop.

List `<directory>/.claude-env-*`. If there is exactly one, that is the env. If
there are several, each has its **own** persona for its own role — ask which
blueprint this is for and stop. Never merge two blueprints' personas.

If the directory has no env dir, say so and stop; there is no persona to
regenerate.

### 2. Read the current persona — and check there is meant to be one

`<env>/CLAUDE.md`. **You are revising, not starting from a blank page.** Anything
already there was either chosen deliberately or has survived; treat removing a
line as a change you must justify, the same as adding one.

**An absent persona tells you what *kind* of project this is — not that a persona
is unwanted.** Check the blueprint's `claude_md` in aello's config (on Windows,
`%APPDATA%\aello\config\config.toml` — note the extra `config/` level):

- **`coder`** — a coding project, started on the stock template because that was
  a reasonable day-one default.
- **`none`** — **not a coding project.** Anything outside IT starts blank,
  because a coding persona would be actively wrong there — not because this env
  should never have one. Blank is right on day one and stays right until the
  project grows.
- **`custom`** — a persona has already been generated and accepted for this env.
  `<env>/CLAUDE.md` is authoritative and aello writes nothing over it. Check
  `<env>/persona.gen` for which generation it is on; you are proposing the next.
- **A path** — the user maintains a persona file of their own. Read it and
  propose changes **to that file**, naming its path; never relocate the persona
  into the env dir. Some of these files are shared by several blueprints, so a
  change there lands in more than one env.

So a grown non-coding project earns a custom persona exactly as a grown coding
one does. What changes is where you start: you have no template to revise, and
you must **derive the role from what the project actually does**. Do not import
coding assumptions — tests, builds, commits, "verified" meaning a green suite —
unless this project genuinely has them. A research project's persona is about
how to handle sources and uncertainty; a writing project's is about voice and
revision. Same altitude, different substance.

Treat a file containing *only* the TL;DR section as absent — `place()` writes
that on its own, so it is not evidence anyone chose a persona.

### 2b. Read contextdb — this step is mandatory

**Do this before deciding anything, and report what you found.** contextdb is
the archive of how work has actually gone in this env: every session that ended
with `/clear` or exit leaves a record holding that session's `/handoff` note,
and from mid-2026 a copy of the transcript itself. It is the only place the
*user's own words* survive — everything else you can read is an agent's summary
of them, and summaries are where a persona's evidence quietly turns into
invention.

Find the root in `%APPDATA%\aello\config\config.toml` (`contextdb = …`) — it is
**not** always the default `~/aello/contextdb`. Records live at
`<contextdb>/<project>/<blueprint>/`.

Report three things:

1. **How many sessions, and over what span.** `*_end.jsonl` count and date range.
2. **What the user actually said.** Pull their turns out of any archived
   `*_transcript.jsonl`, and their instructions out of the `handoff` field.
   Directive language — "don't", "never", "instead", "I want", "why did you" —
   is where the persona's rules come from.
3. **Recurring lessons.** Something reverted and redone, a fix made in three
   places, a correction issued more than once.

**A thin contextdb does not prove a young project.** Archiving depends on the
`SessionEnd` hook being registered in that env's `settings.json`, and that hook
reached most envs only recently — it was in 2 of 39 as late as 2026-08-02. An
empty directory may mean nobody was recording. Before concluding a project is
young, check `<env>/settings.json` for a `SessionEnd` entry running
`session-end.py`; say which of the two you found.

### 2c. Is the project grown enough to deserve one?

The gate, and it applies to both kinds:

- **Barely started** — few commits, little beyond the scaffold aello created,
  and **little or no contextdb history**. Nothing has been learned yet that a
  persona could encode, so there is nothing to write: a custom persona here is
  invention dressed as observation. **Say so and stop**, and leave it as it is —
  `coder` for a coding project, `none` for anything else.
- **Grown** — real history, real conventions, decisions visible in the commits,
  the docs and the archived sessions. Proceed.

Weigh contextdb depth heavily. A handful of archived sessions is not enough
material to derive working rules from, however old the repo is — and the point
of a generated persona is that every line traces to something that happened.

If it is borderline, say which way you lean and why, and let the user decide.
The cost of waiting is nothing; the cost of a confidently wrong persona is that
it is read on every turn.

### 3. Assess the project

Read in this order, stopping when you have enough to characterise how work
actually goes here:

- `README.md` — what this is and who it is for.
- `<project>/CLAUDE.md` — the project layer. Read it to know what you must
  **not** repeat, and to see which rules keep being restated (a rule written
  three times is a rule someone keeps breaking — that is persona material).
- `<env>/projects/<encoded-cwd>/memory/` — the accumulated lessons for this env.
  The richest source **after contextdb**, and unlike contextdb it is already
  distilled — which also means it is one agent's reading of what happened.
  Don't build that path by hand: list `<env>/projects/` and use what is there.
- `git log --oneline -100` and a handful of full messages — how changes arrive,
  how big they are, what the commit voice is, what gets fixed twice.
- `CHANGELOG.md` if present — the shape of what ships.
- The layout: languages, where tests live, whether CI exists, how it is built.

Look specifically for **recurring working lessons**: something reverted and
redone, a fix that had to be made in three places, a convention that appears
everywhere but is written nowhere, a class of mistake the docs keep warning
about. Those are what a grown persona should encode, and they are the whole
reason this skill exists.

### 4. Draft the persona

Keep it the length of a page someone will actually re-read — the persona is
loaded on **every turn**, so every line is paid for continuously. If a line does
not change what the agent does, cut it. Bias toward fewer, sharper rules.

Cover, in whatever order suits the project: the role, how to think before acting,
how to size and scope changes, what "verified" means here, how to communicate
results, and commit discipline. Drop any of those the project genuinely does not
care about rather than padding them out.

Write rules that are **actionable and falsifiable**. "Be careful" is not a rule.
"Reproduce the bug before fixing it and confirm it's gone after" is.

**Two things you must not do:**

- **Do not invent facts about the project.** Every project-shaped claim must be
  something you actually read. If you are unsure whether a convention is real,
  leave it out and say so in your summary rather than asserting it.
- **Leave the TL;DR section exactly as you found it.** If the current persona has
  one, **drop it** and say that you did. Every env now carries the TL;DR
  instruction on a bundled `UserPromptSubmit` hook, injected on every prompt
  alongside the conciseness and anti-sycophancy rules, so a copy in the persona
  is redundant — it survives in older personas only because aello will not edit
  a file the user owns. A generated persona is a fresh file and should not
  reintroduce it.

  Do not restate the other two hook rules either. Be concise / don't be
  sycophantic reach every env per turn already; repeating them here costs
  context on every turn and buys nothing.

  The stakes are small either way: `speak.py` blocks any turn with no `TL;DR:`
  line and asks for one, so the worst case is an extra round trip rather than a
  silent env.

### 5. Output

Print, in this order:

1. **What the record shows** — the contextdb numbers (sessions, span, whether
   the hook was even registered), then three or four sentences on what you found
   and what it implies for how this agent should work. This is what the user
   argues with, so make the reasoning visible.
2. **The proposed persona** — the complete file in one fenced block, nothing
   elided, ready to paste. It must stand alone.
3. **What changed and why** — the material differences from the current persona,
   each with the evidence behind it, quoting the user's own words from contextdb
   where you have them ("you said X twice, so this rule is now specific"). Name
   anything you deliberately left out and why.
4. **What you were unsure about** — the judgement calls, so the user can settle
   them. Be honest here; this is the part that makes the conversation useful.

Then stop. Expect to do this several times: the user will take the proposal to
the target env, argue with it there, and come back. Nothing is installed until
they run `/regenerate-claude-md-accept`, which is a separate skill — do not
offer to write the file yourself, and do not run that skill on their behalf.
