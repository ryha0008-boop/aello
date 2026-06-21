---
name: handoff
description: Write a self-contained HANDOFF.md resume note so the next session continues seamlessly after a /clear. Invoke manually with /handoff.
disable-model-invocation: true
allowed-tools: Write, Read, Bash
---

# /handoff — session resume note

When invoked, write a `HANDOFF.md` at the project root that lets the **next**
session resume this work with **zero prior context**. Invoking this skill is
your authorization to do so.

A handoff is not a compact: after a `/clear` there is no conversation summary to
fall back on, so `HANDOFF.md` must be **fully self-contained**. Assume the reader
boots fresh, has never seen this conversation, and reads only this file plus the
pointers it names.

`HANDOFF.md` is **transient and untracked** — it is read on boot, then deleted.
Begin the file with a one-line banner: `> Transient resume note (TechnicalDirector). Read on boot, then delete.`

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
