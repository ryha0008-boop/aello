"""SessionEnd hook — saves a session record to contextdb on /clear, logout, or exit.

PostCompact only fires when a session compacts; a session ended with /clear (or a
plain exit) never compacts, so its context would otherwise never reach contextdb.
This hook captures those: it archives the self-contained HANDOFF.md (written by the
/handoff skill, deleted on next boot) plus a pointer to the full transcript.
"""
import sys
import json
import os
from datetime import datetime, timezone

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)

# SessionEnd also fires for every subagent/Task session — skip those, or contextdb
# floods with one entry per spawned agent. Only the main interactive session counts.
if data.get("subagent", False):
    sys.exit(0)

env_dir = os.environ.get("CLAUDE_CONFIG_DIR", "")
if not env_dir:
    sys.exit(0)

agent = os.path.basename(env_dir)
prefix = ".claude-env-"
if agent.startswith(prefix):
    agent = agent[len(prefix):]

# The project is the folder the env dir lives in: <project>/.claude-env-<agent>.
project = os.path.basename(os.path.dirname(os.path.normpath(env_dir))) or "unknown"

# Unified location if aello passed one (AELLO_CONTEXTDB): <base>/<project>/<agent>.
# Otherwise local to the env (already inside the project).
base = os.environ.get("AELLO_CONTEXTDB", "")
if base:
    contextdb_dir = os.path.join(base, project, agent)
else:
    contextdb_dir = os.path.join(env_dir, "contextdb")
os.makedirs(contextdb_dir, exist_ok=True)

ts = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
session = data.get("session_id", "unknown")[:8]
# `_end` suffix so a SessionEnd never clobbers a PostCompact file from the same
# session + second.
filepath = os.path.join(contextdb_dir, f"{ts}_{session}_end.jsonl")

# Archive the /handoff note if present — it's the crafted, self-contained resume
# summary, and it's deleted on next boot, so this is the only chance to keep it.
cwd = data.get("cwd", "") or os.getcwd()
handoff = ""
try:
    with open(os.path.join(cwd, "HANDOFF.md"), encoding="utf-8") as f:
        handoff = f.read().strip()
except Exception:
    pass

entry = {
    "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "agent": agent,
    "session": data.get("session_id", "unknown"),
    "trigger": data.get("reason", "unknown"),
    "kind": "session_end",
    "handoff": handoff,
    "transcript": data.get("transcript_path", ""),
}

try:
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
except Exception:
    pass
