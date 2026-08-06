"""SessionStart hook — tells the session it is running under aello, then
surfaces the /handoff resume note and the /note inbox, deleting both.

This is the consumer the /handoff skill has always promised. The skill tells the
agent the note is "read on boot, then deleted", and writes a banner saying so,
but nothing ever read it: with no SessionStart hook the file just sat at the
project root, dirtying git status and being re-archived verbatim by every
subsequent SessionEnd.

Reading it here closes that loop and kills the duplicate archiving as a side
effect — the file is gone before the next SessionEnd can find it again. Deleting
is safe because SessionEnd already archived the note to contextdb, so the content
survives even if this session never uses it.

Also delivers <agent>.NOTE.md, the cross-env inbox written by /note.

The standing block is the other half. A session had no reliable way to learn it
was running under aello at all: the env dir is gitignored, the persona is
user-owned and often says nothing about it, and a project CLAUDE.md only exists
for a maintainer. So agents edited files in `.claude-env-*` that the next launch
overwrote, and ran seeded skills that are the user's alone to invoke. Announcing
it here rather than in the persona is deliberate — the persona is the file most
likely to be rewritten wholesale, while `place()` rewrites this script on every
run, so the block reaches an env placed months ago without touching anything the
user owns. Keep it short: it costs context on every single session.
"""
import sys
import json
import os

try:
    # Read bytes and decode UTF-8 ourselves - `json.load(sys.stdin)` decodes
    # with the console code page on Windows. Measured (cp1252, Python 3.14): it
    # does NOT raise - stdin's error handler is `surrogateescape` - so a CJK
    # character in a path comes back as mojibake plus a lone surrogate and the
    # `except` below never fires. A non-ASCII `cwd` decoded that way names no
    # real directory, so the handoff/note lookup misses. Same form speak.py uses.
    data = json.loads(sys.stdin.buffer.read().decode("utf-8-sig", "replace") or "{}")
except Exception:
    sys.exit(0)

env_dir = os.environ.get("CLAUDE_CONFIG_DIR", "")
if not env_dir:
    sys.exit(0)

agent = os.path.basename(env_dir)
prefix = ".claude-env-"
if agent.startswith(prefix):
    agent = agent[len(prefix):]

cwd = data.get("cwd", "") or os.getcwd()

# Both files are addressed to this blueprint by name, so two envs sharing a repo
# never read each other's. HANDOFF is a note to self from the last session; NOTE
# is an inbox from another env.
parts = []
for filename, heading in (
    (f"{agent}.HANDOFF.md", "Resume note from your last session"),
    (f"{agent}.NOTE.md", "A note left for you by another environment"),
):
    path = os.path.join(cwd, filename)
    try:
        with open(path, encoding="utf-8") as f:
            body = f.read().strip()
    except Exception:
        continue
    if body:
        parts.append(f"## {heading} (`{filename}`)\n\n{body}")
    # Delete either way: an empty file is nothing to resume from, and leaving it
    # would keep it in every future SessionEnd archive.
    try:
        os.remove(path)
    except Exception:
        pass

standing = f"""## You are running under aello

This session is an **aello** environment named `{agent}` — an isolated Claude Code
setup whose config dir is `{env_dir}`.

- That directory is **gitignored and rewritten on every `aello run`**. Don't
  hand-edit its skills, settings or hooks; the next launch replaces them. To keep
  an edited skill, put an empty `.aello-keep` file beside its `SKILL.md`.
- The seeded skills — `/sync`, `/handoff`, `/note`, `/twosentences` — are **the
  user's to type, never yours to run**. Opening one's `SKILL.md` and working
  through its steps *is* running it. If you think a checkpoint is due, say so and
  let the user invoke it.
- Other blueprints may share this repo. The working tree is shared; config,
  memory and session history are not. Commits you make are attributed to
  `{agent}` automatically.
- `aello docs` lists the reference docs, `aello docs <name>` prints one."""

if parts:
    delivered = (
        "The following was left for you before this session started. It has been "
        "read and the file deleted, so this is the only copy in front of you — act "
        "on it, don't go looking for the file.\n\n" + "\n\n---\n\n".join(parts)
    )
    context = standing + "\n\n---\n\n" + delivered
else:
    context = standing

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "SessionStart",
        "additionalContext": context,
    }
}))
