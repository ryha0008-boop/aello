---
name: working-style
description: "User doesn't read plans — give short decisions to choose from, ask often; conciseness and anti-sycophancy are universal requirements enforced structurally"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 798bceee-11a8-4325-88c2-708fc3b37d5a
  modified: 2026-08-03T05:36:10.020Z
---

The user does not read plans or long write-ups. Don't present "here is my N-step plan" or a wall of prose for sign-off — instead surface concrete decisions to choose from ("which of these?") and let the user pick. Ask short, ask often: many small focused questions beat one big upfront plan.
**Why:** plans go unread; choosing between concrete options is faster and keeps the user steering. **How to apply:** "which of these?" not "here is my N-step plan"; ask short, ask often.

**Conciseness and anti-sycophancy are universal requirements, not per-project style** (2026-08-03). The user asked for both to be pushed across *all* envs and picked the strongest mechanism on offer: a bundled `UserPromptSubmit` hook injecting them on every prompt in all 41 envs, rather than persona text. **Why:** a rule added to the persona reaches *no* existing env (written once, never clobbered), and a rule delivered once per session decays over a long one — the user wants this enforced structurally, not remembered. **How to apply:** when the user asks for a behavioural rule "everywhere", reach for the channel that self-propagates (`place()` rewrites hook scripts on every run) and expect to backfill rather than let envs migrate lazily. See [[aello-voice-capability]] and [[aello-dev-gotchas]] #13 — a hook backfill is the file *and* the settings registration, both halves.
