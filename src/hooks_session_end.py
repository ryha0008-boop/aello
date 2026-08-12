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
import hashlib
from datetime import datetime, timezone

try:
    # Read bytes and decode UTF-8 ourselves - `json.load(sys.stdin)` decodes
    # with the console code page on Windows. Measured (cp1252, Python 3.14): it
    # does NOT raise - stdin's error handler is `surrogateescape` - so a CJK
    # character in a path comes back as mojibake plus a lone surrogate and the
    # `except` below never fires. A `transcript_path` decoded that way no
    # longer opens (measured: FileNotFoundError), so the archive silently
    # degrades back to a pointer. Same form speak.py uses.
    data = json.loads(sys.stdin.buffer.read().decode("utf-8-sig", "replace") or "{}")
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
# own session files on a retention timer, and the env dir they live in is
# gitignored and removed outright by `aello remove --purge` — so a recorded path
# is a reference that quietly stops resolving. Measured on 2026-08-03, 15% of 265
# archives already dangled, with a clean cliff at the 30-day mark; re-measured on
# 2026-08-12, 269 of 415 archives held nothing but a path, and the reported fleet
# total was half the real one as a result.
#
# Then, when the copy is PROVEN good and the session left a handoff note, delete
# the original. contextdb becomes the single store, and Claude Code's own copy —
# which is the one sitting in a working tree, in a directory tools scan — is gone
# within the session rather than in N days.
transcript_path = data.get("transcript_path", "")
archived = ""
verified = ""
original_deleted = ""
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
        #
        # Build the prefix from character codes rather than as a literal: written
        # as "\\\\?\\" it survives Python fine, but the same string through a
        # shell heredoc arrived as a 3-character `\?\` and failed 226 copies in a
        # row with Errno 22. Four characters, or it is not the prefix.
        long_prefix = chr(92) * 2 + "?" + chr(92)
        src_path = transcript_path
        if os.name == "nt" and not src_path.startswith(long_prefix):
            src_path = long_prefix + os.path.abspath(src_path)
            dest = long_prefix + os.path.abspath(dest)
        # Stream it: transcripts run to tens of MB and this is a session-exit
        # hook, so never hold one in memory. Source is opened BEFORE the
        # destination on purpose — reversed, an unreadable transcript would
        # leave a 0-byte file behind that looks like a successful archive.
        # Hash while copying, so the digest is of the bytes actually read.
        src_hash = hashlib.sha256()
        written = 0
        with open(src_path, "rb") as src, open(dest, "wb") as out:
            while True:
                chunk = src.read(1 << 20)
                if not chunk:
                    break
                src_hash.update(chunk)
                out.write(chunk)
                written += len(chunk)
        archived = os.path.basename(dest)

        # Verify by re-reading what landed on disk. A size check alone would
        # pass a truncated-then-padded write and, more to the point, the whole
        # reason to verify is that the next step is irreversible.
        dst_hash = hashlib.sha256()
        with open(dest, "rb") as out:
            while True:
                chunk = out.read(1 << 20)
                if not chunk:
                    break
                dst_hash.update(chunk)
        if dst_hash.hexdigest() == src_hash.hexdigest() and written > 0:
            verified = "sha256"
        else:
            verified = "MISMATCH"
    except Exception as e:
        # A missing or unreadable transcript must not cost us the handoff note
        # below, which is the part that cannot be recovered from anywhere else.
        archived = ""
        verified = "error: %s" % type(e).__name__

    # Delete the original only when the copy is proven byte-identical AND the
    # session wrote a handoff note.
    #
    # The note is the gate for a specific reason, not as a proxy for "ended
    # cleanly": deleting the original costs `--resume` for that session and
    # nothing else, because the archive holds every byte. A handoff note *is*
    # the continuity — it is written to be read by the next session — so once it
    # exists the transcript's resume value is already spent. No note means the
    # session may still be worth resuming, so its transcript stays and the
    # retention timer takes it later. Measured over 372 archives: 257 (69%)
    # carried a note.
    if verified == "sha256" and handoff:
        try:
            os.remove(src_path)
            original_deleted = "session-end"
        except Exception as e:
            # Windows refuses to unlink a file another process holds open
            # without FILE_SHARE_DELETE, and Claude Code may still have the
            # transcript open while this hook runs. Recorded, not hidden: the
            # retention timer is the backstop, and an archive that always says
            # `locked` is the signal that this branch never actually works.
            original_deleted = "failed: %s" % type(e).__name__

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
    # "sha256" when the copy was proven byte-identical, "MISMATCH", or the
    # exception type. Never absent, so a silent failure has nowhere to hide.
    "transcript_verified": verified,
    # "session-end" when Claude Code's own copy was removed, "failed: <Error>"
    # when it could not be, "" when the gate said keep it.
    "original_deleted": original_deleted,
}

try:
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
except Exception:
    pass
