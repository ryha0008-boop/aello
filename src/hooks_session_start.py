"""SessionStart hook — surfaces the /handoff resume note, then deletes it.

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
"""
import sys
import json
import os

try:
    data = json.load(sys.stdin)
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

if not parts:
    sys.exit(0)

context = (
    "The following was left for you before this session started. It has been "
    "read and the file deleted, so this is the only copy in front of you — act "
    "on it, don't go looking for the file.\n\n" + "\n\n---\n\n".join(parts)
)

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "SessionStart",
        "additionalContext": context,
    }
}))
