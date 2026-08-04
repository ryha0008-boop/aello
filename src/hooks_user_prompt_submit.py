"""UserPromptSubmit hook — the four response rules, injected on every prompt.

Style decays. An instruction delivered once at session start is thoroughly
buried by turn eighty, which is exactly when padding and reflexive agreement
creep back in. These four rules are about how each individual response is
written, so they are delivered per turn rather than per session.

The last of them fixes the whole shape of a response: a few sentences of
prose, then one closing block that is the TL;DR line with the next steps
directly beneath it. Steps and TL;DR were separate rules for about an hour and
that was one section too many — the summary said one thing, the steps repeated
it, and the reader had to reconcile them. Merged, the spoken line introduces
the list it sits on top of.

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

It also has to stay on ONE line, and the steps have to stay below it rather
than above. `extract_tldr` matches `^…TL;DR:\s*(.+?)$` and takes the *last*
match, so a summary wrapped onto a second line is spoken with its tail cut off,
silently. Numbered steps underneath are safe — they match nothing — which is
what makes this ordering possible at all.

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
    "Close every response with one block and nothing after it: a single line "
    "of exactly the form `TL;DR: <two to four sentences>` giving the outcome "
    "and what it means, then — when anything is left for the user to do — "
    "3–4 numbered steps, in order. Keep the TL;DR on one line with no bullets "
    "or bold; it is the only part read aloud, so it carries the answer alone. "
    "The steps must stand alone too: assume the user reads nothing above the "
    "block, so anything they need is in a step and no step says \"as described "
    "above\". Their actions, not yours — steps you are about to take yourself "
    "are a plan. Drop the steps when nothing is waiting on them."
)

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "UserPromptSubmit",
        "additionalContext": INSTRUCTION,
    }
}))
