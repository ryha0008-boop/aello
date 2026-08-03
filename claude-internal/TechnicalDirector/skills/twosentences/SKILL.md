---
name: twosentences
description: Summarize your previous response in exactly two sentences. Invoke manually with /twosentences.
disable-model-invocation: true
allowed-tools:
---

# /twosentences — two-sentence summary

> **Only the user runs this.** It happens when they type `/twosentences`, and at no
> other time. If you are reading this file because *you* decided to — to see what
> it does, or to carry out its steps yourself — then stop and do neither.
> Following these instructions **is** running the skill, whichever route you took
> to them, and a checkpoint the user did not ask for is one they will believe
> happened when it did not. Say the skill exists and let them invoke it.

When invoked, condense your **previous response** (the most recent assistant
message before this invocation) into **exactly two sentences**.

Output only those two sentences — no preamble, no heading, no bullets, no code,
nothing else. Keep the key facts and the outcome; drop detail, caveats, and
step-by-step explanation.

---

*aello regenerates this skill on every run, so edits made here are replaced. To
keep a version you have rewritten for this project, create an empty
`.aello-keep` file beside this one (`skills/twosentences/.aello-keep`) — aello then
leaves the skill alone, and will not delete it either. A kept skill no longer
tracks the blueprint's role; remove the marker to return to the generated
version.*
