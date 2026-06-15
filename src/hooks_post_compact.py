"""PostCompact hook — saves the compaction summary to contextdb/<timestamp>_<session>.jsonl"""
import sys
import json
import os
from datetime import datetime, timezone

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

# Unified location if aello passed one (AELLO_CONTEXTDB), else local to the env.
base = os.environ.get("AELLO_CONTEXTDB", "")
if base:
    contextdb_dir = os.path.join(base, agent)
else:
    contextdb_dir = os.path.join(env_dir, "contextdb")
os.makedirs(contextdb_dir, exist_ok=True)

ts = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
session = data.get("session_id", "unknown")[:8]
filepath = os.path.join(contextdb_dir, f"{ts}_{session}.jsonl")

raw = data.get("compact_summary", "")

# Parse <analysis>...</analysis> and <summary>...</summary> sections.
# The </analysis>\n\n<summary> transition is a reliable boundary.
analysis = ""
summary = ""

for sep in ["</analysis>\n\n<summary>", "</analysis>\n<summary>"]:
    idx = raw.find(sep)
    if idx >= 0:
        a_start = raw.find("<analysis>")
        if a_start >= 0:
            analysis = raw[a_start + len("<analysis>"):idx].strip()
        summary_start = idx + len(sep)
        s_end = raw.rfind("</summary>")
        if s_end > summary_start:
            summary = raw[summary_start:s_end].strip()
        break
else:
    analysis = raw.strip()

entry = {
    "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "agent": agent,
    "session": data.get("session_id", "unknown"),
    "trigger": data.get("trigger", "unknown"),
    "analysis": analysis,
    "summary": summary,
}

try:
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
except Exception:
    pass
