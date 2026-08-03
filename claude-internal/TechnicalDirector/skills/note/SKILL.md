---
name: note
description: Leave a note for another aello environment about something it needs to fix. Invoke manually with /note <env-name>.
disable-model-invocation: true
allowed-tools: Write, Read, Bash
---

# /note — leave a note for another environment

> **Only the user runs this.** It happens when they type `/note`, and at no
> other time. If you are reading this file because *you* decided to — to see what
> it does, or to carry out its steps yourself — then stop and do neither.
> Following these instructions **is** running the skill, whichever route you took
> to them, and a checkpoint the user did not ask for is one they will believe
> happened when it did not. Say the skill exists and let them invoke it.

When invoked, leave a note addressed to **another** aello environment. The
argument is that environment's name (e.g. `/note frontend`). Invoking this skill
is your authorization to write the note.

This is **not** a handoff. `/handoff` is a note to *yourself* for your next
session; `/note` is a message to a *different* environment — you touched
something it owns, or hit a problem on its side, and it needs to act.

You are the **`TechnicalDirector`** environment. The note goes to `<target>.NOTE.md` at the
**target's** project root. That file is the target env's inbox — its SessionStart
hook reads the note on boot, delivers it, and deletes the file.

Steps:
1. Take the target env name from the argument; if none was given, ask which
   environment the note is for and stop. Use the blueprint's **canonical
   casing** (`RevoicedMainDev`, not `revoicedmaindev`) — the hook looks for
   exactly `<Name>.NOTE.md`, so wrong casing is a silent dead letter on a
   case-sensitive filesystem.
2. **Work out the target's project root — it is not always this repo.** Look for
   `.claude-env-<target>/` here first. If it is not here, the target env lives in
   a different repo and the note belongs at **that** repo's root: its
   SessionStart hook only ever reads its own project root, so a note left here
   would never be delivered. Locate that repo (ask the user for the path if you
   cannot) rather than writing a note nobody will read. Only if you cannot
   establish where the target lives should you treat the name as a typo and
   check with the user.
3. **Overwrite** `<target>.NOTE.md` at that project root with a single, current
   note — do not append to or preserve any old note. The target reads a note as
   soon as you leave it, so only the latest matters; a fresh note supersedes
   whatever was there.
4. Write, in this order:
   - A one-line banner: `> Note for the <target> env from TechnicalDirector. Read it, act on it, then delete this file.`
   - A `## from TechnicalDirector — <timestamp>` heading (get the timestamp with `date`).
   - **What I was doing** — the task that led here.
   - **The problem** — what is broken or blocked on the target's side, concretely.
   - **What you need to fix** — the specific change or decision you need from the
     target env, naming the files/paths involved.
   Keep it tight and actionable; the target boots without your context.

Then tell the user the note was written, naming the **full path** — so a note
sent into another repo is obviously that, and not mistaken for one left here.

---

*aello regenerates this skill on every run, so edits made here are replaced. To
keep a version you have rewritten for this project, create an empty
`.aello-keep` file beside this one (`skills/note/.aello-keep`) — aello then
leaves the skill alone, and will not delete it either. A kept skill no longer
tracks the blueprint's capabilities; remove the marker to return to the
generated version.*
