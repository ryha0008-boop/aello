"""PreToolUse hook — deny the plan-mode tools, fleet-wide.

The user's rule is "no plans, ever — only a question/answer flow". The
`UserPromptSubmit` text says so on every turn, but an instruction is a rule the
model follows rather than one it cannot break. This is the half it cannot
break: `EnterPlanMode` and `ExitPlanMode` are denied outright, so plan mode is
unavailable rather than discouraged.

Registered with a `matcher`, so Python is spawned only for these two tool names
— an unmatched PreToolUse hook runs on *every* tool call, which on Windows is a
process spawn per Read.

⚠️ **Measured only halfway.** A `PreToolUse` deny provably blocks a tool and
surfaces its reason to the model (verified 2026-08-03 against `Read`: the call
returned the reason string instead of the file). What is NOT verified is that
these two tool names reach a PreToolUse hook at all — `claude -p` never emits
`ExitPlanMode` in three attempts under `--permission-mode plan`, so print mode
cannot answer it and the check needs one interactive session.

That is why a denial appends to `plan-blocked.log` beside this script. It is
evidence, not state: nothing reads it, and deleting it costs nothing. The first
line to appear in any env settles the question this file could not.
"""
import sys
import json
import os

BLOCKED = ("EnterPlanMode", "ExitPlanMode")
REASON = (
    "Plan mode is disabled in every aello environment. Do not present a plan "
    "for approval. Ask a short question, or do the work."
)

try:
    payload = json.load(sys.stdin)
except Exception:
    # Malformed or absent payload: allow, rather than blocking every tool call
    # in the env on a parse error.
    sys.exit(0)

if payload.get("tool_name") not in BLOCKED:
    sys.exit(0)

try:
    log = os.path.join(os.path.dirname(os.path.abspath(__file__)), "plan-blocked.log")
    with open(log, "a", encoding="utf-8") as fh:
        fh.write("%s\t%s\n" % (payload.get("session_id", "?"), payload.get("tool_name")))
except Exception:
    # Evidence is a nice-to-have; never let it stop the denial.
    pass

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": REASON,
    }
}))
