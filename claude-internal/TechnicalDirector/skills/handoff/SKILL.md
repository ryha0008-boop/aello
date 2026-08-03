---
name: handoff
description: Write a self-contained TechnicalDirector.HANDOFF.md resume note so the next session continues seamlessly after a /clear. Invoke manually with /handoff.
disable-model-invocation: true
allowed-tools: Write, Read, Bash
---

# /handoff — session resume note

> **Only the user runs this.** It happens when they type `/handoff`, and at no
> other time. If you are reading this file because *you* decided to — to see what
> it does, or to carry out its steps yourself — then stop and do neither.
> Following these instructions **is** running the skill, whichever route you took
> to them, and a checkpoint the user did not ask for is one they will believe
> happened when it did not. Say the skill exists and let them invoke it.

When invoked, write a `TechnicalDirector.HANDOFF.md` at the project root that lets the
**next** session resume this work with **zero prior context**. Invoking this
skill is your authorization to do so.

The filename is prefixed with this blueprint's name (`TechnicalDirector`) so multiple
blueprints sharing one repo each keep their own handoff without clobbering each
other. Write exactly `TechnicalDirector.HANDOFF.md`, no other name.

A handoff is not a compact: after a `/clear` there is no conversation summary to
fall back on, so `TechnicalDirector.HANDOFF.md` must be **fully self-contained**. Assume the
reader boots fresh, has never seen this conversation, and reads only this file
plus the pointers it names.

`TechnicalDirector.HANDOFF.md` is **transient and untracked** — it is read on boot, then
deleted. Begin the file with a one-line banner: `> Transient resume note (TechnicalDirector). Read on boot, then delete.`

Write these sections, in order:

1. **Read first** — point the next session at its durable context before
   anything else: the env persona (`$CLAUDE_CONFIG_DIR/CLAUDE.md`) and the
   memory index (`$CLAUDE_CONFIG_DIR/projects/<this-project>/memory/MEMORY.md`).
   Tell it to read those before acting on this note.
2. **Shipped this session** — what actually changed, with commit shas (run
   `git log --oneline` for the recent ones) and a one-line summary each. Note
   anything committed-but-not-pushed or staged-but-not-committed.
3. **Open threads / next steps** — what is in flight, what was deferred, and the
   concrete next action. Be specific enough to act on without re-deriving it.
4. **Gotchas** — traps the next session would otherwise hit: failing/flaky
   tests, environment quirks, decisions made and why, paths that matter.

Keep it tight and skimmable. Then tell the user the note is written and remind
them it is deleted on next boot.

`TechnicalDirector.HANDOFF.md` is **never committed.** It is untracked on purpose and gone
by the next boot, so if you run `/sync` after this, leave it out of the staging
list — it is a file you created this session, and that rule does not cover it.

---

*aello regenerates this skill on every run, so edits made here are replaced. To
keep a version you have rewritten for this project, create an empty
`.aello-keep` file beside this one (`skills/handoff/.aello-keep`) — aello then
leaves the skill alone, and will not delete it either. A kept skill no longer
tracks the blueprint's capabilities; remove the marker to return to the
generated version.*
