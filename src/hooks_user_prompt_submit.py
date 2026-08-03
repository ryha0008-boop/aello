"""UserPromptSubmit hook — the four response rules, injected on every prompt.

Style decays. An instruction delivered once at session start is thoroughly
buried by turn eighty, which is exactly when padding and reflexive agreement
creep back in. These four rules are about how each individual response is
written, so they are delivered per turn rather than per session.

The no-plans rule has a second half in `pre-tool-use.py`, which denies the
plan-mode tools outright. Both are needed: the hook stops the tool, and only
this text stops a numbered proposal written as ordinary prose — which is most
of what a plan actually looks like.

It lives here, and not in the persona, for the reason the SessionStart block
does: `place()` rewrites this script on every run, so a change reaches an env
placed months ago, while the persona is written once, never clobbered, and is
the file most likely to be rewritten wholesale. The voice depended on that file
for its one input until this hook existed.

The TL;DR line is not optional here. `speak.py` speaks that line and nothing
else, and exits 2 asking for one when it is missing — so an env that got this
hook without it would be nagged every turn.

Keep the text short. It is prepended to every single prompt, so its cost is
paid continuously; this is the one place where a paragraph of good advice is
worse than a sentence of it.
"""
import sys
import json

try:
    json.load(sys.stdin)
except Exception:
    # Malformed or absent payload: say nothing rather than injecting noise.
    sys.exit(0)

INSTRUCTION = (
    "Be concise: no preamble, no filler, no hedging, no restating the "
    "question. Lead with the answer and stop once it's given.\n\n"
    "Don't open with praise or agreement. Never validate a premise you "
    "haven't checked, and never soften a finding to be agreeable. Say plainly "
    "when the user is wrong and why; say \"I don't know\" when you don't.\n\n"
    "Never present a plan for approval, and never use plan mode. No numbered "
    "proposals, no \"here's my approach — shall I proceed?\". Ask a short "
    "question or do the work. When the choice is genuinely the user's, offer "
    "concrete options to pick from rather than a plan to read.\n\n"
    "End with a final line of exactly the form `TL;DR: <two sentences>` — "
    "what happened and what it means or what's next, the outcome rather than "
    "the steps. No bullets, no bold, nothing after it. That line is the only "
    "part read aloud, so it carries the answer alone."
)

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "UserPromptSubmit",
        "additionalContext": INSTRUCTION,
    }
}))
