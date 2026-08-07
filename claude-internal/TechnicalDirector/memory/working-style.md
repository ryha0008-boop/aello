---
name: working-style
description: "User doesn't read plans — give short decisions to choose from, ask often; conciseness and anti-sycophancy are universal requirements enforced structurally"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 798bceee-11a8-4325-88c2-708fc3b37d5a
  modified: 2026-08-07T21:55:57.309Z
---

The user does not read plans or long write-ups. Don't present "here is my N-step plan" or a wall of prose for sign-off — instead surface concrete decisions to choose from ("which of these?") and let the user pick. Ask short, ask often: many small focused questions beat one big upfront plan.
**Why:** plans go unread; choosing between concrete options is faster and keeps the user steering. **How to apply:** "which of these?" not "here is my N-step plan"; ask short, ask often.

**On visual work the user reviews the rendered thing and wants the knobs, not the reasoning** (2026-08-07, the site repalette). They asked for the design system to be *displayed* before changing it — "I will want to change some things, so let's make sure everything is clear now" — then gave precise, screenshot-attached feedback ("motion is used multiple times but I don't see any motion", "the landing motion is too fast"). Brand decisions they have made: **orange accent on near-black**, explicitly *not* pure black ("that's bad design practice"), and when adding a colour, **piggyback the existing token slots rather than growing the set**. **Why:** they iterate on what they can see, so a page that renders the tokens live beats prose about them, and a token they can point at beats a value buried in a component. **How to apply:** build the visible artefact first, keep every value in one editable file, and when they report something missing, *measure the running page* before agreeing it is missing — twice now the site was correct and the machine or the easing curve was the cause. See [[aello-dev-gotchas]] #36–37.

**Conciseness and anti-sycophancy are universal requirements, not per-project style** (2026-08-03). The user asked for both to be pushed across *all* envs and picked the strongest mechanism on offer: a bundled `UserPromptSubmit` hook injecting them on every prompt in all 41 envs, rather than persona text. **Why:** a rule added to the persona reaches *no* existing env (written once, never clobbered), and a rule delivered once per session decays over a long one — the user wants this enforced structurally, not remembered. **How to apply:** when the user asks for a behavioural rule "everywhere", reach for the channel that self-propagates (`place()` rewrites hook scripts on every run) and expect to backfill rather than let envs migrate lazily. See [[aello-voice-capability]] and [[aello-dev-gotchas]] #13 — a hook backfill is the file *and* the settings registration, both halves.
