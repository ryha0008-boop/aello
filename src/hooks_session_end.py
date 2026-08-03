"""SessionEnd hook — saves a session record to contextdb on /clear, logout, or exit.

PostCompact only fires when a session compacts; a session ended with /clear (or a
plain exit) never compacts, so its context would otherwise never reach contextdb.
This hook captures those: it archives the self-contained <agent>.HANDOFF.md (written
by the /handoff skill, deleted on next boot) plus a copy of the full transcript.

In practice this hook does *all* the capturing. PostCompact fires only when a
session compacts, and with a 1M context window ended by /clear that effectively
never happens: on 2026-08-03 contextdb held 265 SessionEnd records and zero
PostCompact records for any aello blueprint, the newest compaction capture being
seven weeks old and from a pre-aello setup.
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
# Every other risky call here fails silently; this one didn't, so an unwritable
# contextdb crashed the hook with a traceback into the session instead.
try:
    os.makedirs(contextdb_dir, exist_ok=True)
except Exception:
    sys.exit(0)

ts = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
session = data.get("session_id", "unknown")[:8]
# `_end` suffix so a SessionEnd never clobbers a PostCompact file from the same
# session + second.
filepath = os.path.join(contextdb_dir, f"{ts}_{session}_end.jsonl")

# Archive the /handoff note if present — it's the crafted, self-contained resume
# summary, and it's deleted on next boot, so this is the only chance to keep it.
# The note is prefixed with the blueprint name so multiple envs in one repo don't
# clobber each other's handoff.
cwd = data.get("cwd", "") or os.getcwd()
handoff = ""
try:
    with open(os.path.join(cwd, f"{agent}.HANDOFF.md"), encoding="utf-8") as f:
        handoff = f.read().strip()
except Exception:
    pass

# Copy the transcript rather than only pointing at it. Claude Code deletes its
# own session files on a retention timer (default 30 days), and the env dir they
# live in is gitignored and removed outright by `aello remove --purge` — so a
# recorded path is a reference that quietly stops resolving. Measured on
# 2026-08-03, 15% of 265 archives already dangled, with a clean cliff at the
# 30-day mark. Copying makes the archive self-contained; the path is still
# recorded next to it, because that is what `--resume` needs and a copy is not.
transcript_path = data.get("transcript_path", "")
archived = ""
if transcript_path:
    try:
        dest = os.path.join(contextdb_dir, f"{ts}_{session}_transcript.jsonl")
        # Windows caps paths at 260 characters unless long paths are enabled
        # (they are off by default). Claude Code's transcript lives under
        # <project>/.claude-env-<name>/projects/<encoded-cwd>/, and the encoded
        # cwd repeats the whole project path — so a deep project blows the limit
        # and `open()` fails. Measured: a 325-char path archived nothing, and the
        # `except` below recorded that honestly rather than crashing, which is
        # exactly how it would go unnoticed. The `\\?\` prefix opts out of the
        # limit; it needs an absolute path with native separators.
        src_path = transcript_path
        if os.name == "nt" and not src_path.startswith("\\\\?\\"):
            src_path = "\\\\?\\" + os.path.abspath(src_path)
            dest = "\\\\?\\" + os.path.abspath(dest)
        # Stream it: transcripts run to tens of MB and this is a session-exit
        # hook, so never hold one in memory. Source is opened BEFORE the
        # destination on purpose — reversed, an unreadable transcript would
        # leave a 0-byte file behind that looks like a successful archive.
        with open(src_path, "rb") as src, open(dest, "wb") as out:
            while True:
                chunk = src.read(1 << 20)
                if not chunk:
                    break
                out.write(chunk)
        archived = os.path.basename(dest)
    except Exception:
        # A missing or unreadable transcript must not cost us the handoff note
        # below, which is the part that cannot be recovered from anywhere else.
        archived = ""

entry = {
    "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "agent": agent,
    "session": data.get("session_id", "unknown"),
    "trigger": data.get("reason", "unknown"),
    "kind": "session_end",
    "handoff": handoff,
    "transcript": transcript_path,
    # Filename beside this record, or "" if the copy failed — so a reader can
    # tell "archived here" from "only ever a pointer" without stat-ing anything.
    "transcript_archived": archived,
}

try:
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
except Exception:
    pass
