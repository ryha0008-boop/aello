"""UserPromptSubmit hook — the five response rules, injected on every prompt.

Style decays. An instruction delivered once at session start is thoroughly
buried by turn eighty, which is exactly when padding and reflexive agreement
creep back in. These five rules are about how each individual response is
written, so they are delivered per turn rather than per session.

The no-plans rule has a second half in `pre-tool-use.py`, which denies the
plan-mode tools outright. Both are needed: the hook stops the tool, and only
this text stops a proposal written as ordinary prose — which is most of what a
plan actually looks like.

The next-steps rule sits right next to it and must not be read as its
opposite, which is why the no-plans rule no longer says "no numbered
proposals". The distinction is whose hands the actions are in: a plan is a list
of what *you* are about to do, held for sign-off; the steps are what the *user*
does next, and they exist so a long answer can be acted on without being read
end to end.

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
    "question. Lead with the answer and stop once it's given. "
    "Keep the prose to a few sentences — if something matters it goes in a "
    "step, not a paragraph. The user should never have to read a wall of text "
    "to find out what to do.\n\n"
    "Don't open with praise or agreement. Never validate a premise you "
    "haven't checked, and never soften a finding to be agreeable. Say plainly "
    "when the user is wrong and why; say \"I don't know\" when you don't.\n\n"
    "Never present a plan for approval, and never use plan mode. Don't lay out "
    "what you intend to do and wait for sign-off — no \"here's my approach — "
    "shall I proceed?\". Ask a short question or do the work. When the choice "
    "is genuinely the user's, offer concrete options to pick from rather than "
    "a plan to read.\n\n"
    "When anything is left for the user to do, close with "
    "3–5 numbered steps: what they do next, in order. The steps must stand "
    "alone — assume the user skips every word above them, so anything they "
    "need is in a step and no step says \"as described above\". "
    "Their actions, not yours — steps you are about to take yourself are a "
    "plan. Leave the list out when nothing is waiting on them.\n\n"
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
