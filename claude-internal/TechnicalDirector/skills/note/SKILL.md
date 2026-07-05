---
name: note
description: Leave a note for another environment in this repo about something it needs to fix. Invoke manually with /note <env-name>.
disable-model-invocation: true
allowed-tools: Write, Read, Bash
---

# /note — leave a note for another environment

When invoked, append a note addressed to **another** aello environment sharing
this repo. The argument is that environment's name (e.g. `/note frontend`).
Invoking this skill is your authorization to write the note.

This is **not** a handoff. `/handoff` is a note to *yourself* for your next
session; `/note` is a message to a *different* environment — you touched
something it owns, or hit a problem on its side, and it needs to act.

You are the **`TechnicalDirector`** environment. Write the note to `<target>.NOTE.md` at the
project root, where `<target>` is the argument you were given. That file is the
target env's inbox — it reads the note, acts on it, then deletes the file.

Steps:
1. Take the target env name from the argument. If none was given, ask which
   environment the note is for and stop. Sanity-check that a `.claude-env-<target>`
   dir exists at the project root; if it does not, warn the user (a typo?) and
   only write the note if they confirm.
2. **Overwrite** `<target>.NOTE.md` with a single, current note — do not append
   to or preserve any old note. The target reads a note as soon as you leave it,
   so only the latest matters; a fresh note supersedes whatever was there.
3. Write, in this order:
   - A one-line banner: `> Note for the <target> env from TechnicalDirector. Read it, act on it, then delete this file.`
   - A `## from TechnicalDirector — <timestamp>` heading (get the timestamp with `date`).
   - **What I was doing** — the task that led here.
   - **The problem** — what is broken or blocked on the target's side, concretely.
   - **What you need to fix** — the specific change or decision you need from the
     target env, naming the files/paths involved.
   Keep it tight and actionable; the target boots without your context.

Then tell the user the note was written to `<target>.NOTE.md`.
