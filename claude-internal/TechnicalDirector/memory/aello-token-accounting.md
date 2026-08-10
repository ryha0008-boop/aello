---
name: aello-token-accounting
description: "`aello tokens` + TUI T tab (2026-08-10): Claude Code transcripts repeat full usage on EVERY content block so dedup by message.id is mandatory (53% of records here are duplicates); the fleet is 3.06B tokens ≈ $2140 at list rates and 98.5% of that is cache READ; the subscription quota appears in no transcript so no tool can honestly report '% of plan'"
metadata: 
  node_type: memory
  type: project
  originSessionId: 31c9589c-0e15-403a-8804-45343b56a964
  modified: 2026-08-10T14:44:30.026Z
---

Shipped 2026-08-10 as `src/tokens.rs` + `aello tokens` + a `T` tab in the TUI.
Reads the transcripts contextdb already archives — no new hook, works
retroactively. Architecture is in the repo's CLAUDE.md and `docs/tokens.md`; what
follows is what only measurement could tell you.

**Claude Code writes one transcript record per *content block*, and repeats the
message's entire `usage` object on each one.** This is a fact about the
transcript format, not about aello, and it is the single thing that makes naive
token counting wrong by ~2x. Measured on one real transcript: **266 usage-bearing
records for 173 distinct `message.id`s**, so summing records overstated output by
**68%** (218,607 vs 129,877). Across this machine's whole archive: **17,216 of
32,350 usage records are duplicates — 53%.** Anything that reads these files for
accounting must dedup by `message.id` first. (The same dedup is what makes it
safe to read overlapping sources: 17 of 122 archives are a session archived
twice, and every live transcript overlaps its own later archive.)

**The fleet's token shape is nothing like intuition, measured 2026-08-10 across
16 envs:** 3.06B tokens total, ≈**$2140** at list API rates. Of that, **cache
read is ~98.5%** (3.00B) against 11M output and 144k input. Even at 0.1x input
price, cache reads therefore *dominate the cost* — for SysAdmin, $106.77 of its
$153.11 was cache read, vs $21.12 of output. Two consequences: shortening
assistant prose is cost-irrelevant (it is already ~0.005% of tokens), and cache
writes here are almost entirely the **1h** TTL bucket (priced 2x input) rather
than 5m. Biggest spenders: AlgoMainDev and TechnicalDirector, ~760M each.

**The subscription's rate limit is in no transcript.** Claude Code records usage,
never the quota. So *no* tool reading these files can honestly say "you are at X%
of your limit" — anything claiming to is inventing the denominator. `aello
tokens` reports % against **this machine's own peak 5-hour block** (273M as of
2026-08-10) and labels it as such. If a real quota number is ever wanted it has to
be a user-set value in `config.toml`; the user was offered that and chose to see
the shipped version first.

**5-hour blocks are global, not per env** — one shared subscription token means
the window is machine-wide, so a per-env window would be the wrong unit. A block
starts at the containing hour of its first message and closes after 5h **or after
a 5h gap**; without the gap rule an overnight pause and the next morning merge
into one "block" that never existed.

**Reach is asymmetric and can't be fixed from here:** contextdb records a
project's *folder name*, not its path, so live (unarchived) sessions can only be
counted for envs placed in the **current** directory. Sessions running in other
projects are invisible until they end. Confirmed working: TechnicalDirector's
total grew 754M → 767M between two runs minutes apart, because the session being
run was being read live.

**Scan cost is seconds, not milliseconds:** 322 MB / **6.2 s** here (220 files),
with a substring pre-filter for `"usage"` before any JSON parse. Fine per CLI
call, fatal per TUI frame — hence the cache on `App::tokens` and the
`SCANNING…` frame painted *before* the scan rather than after.

**Small gotchas:** the model id `<synthetic>` appears in transcripts (Claude Code's
marker for local messages) carrying zero usage — it matches no rate table entry
and correctly shows 0 tokens / no cost, which is not a gap. And `usage` splits
cache writes into `cache_creation.ephemeral_5m_input_tokens` /
`ephemeral_1h_input_tokens`; older records have no `cache_creation` object at all
and must default to the 5m bucket (the API default TTL).

**Open, by the user's own scoping:** they said they want "a lot of functionality
here — filtering, stats, usage per turn, but all that's later, for now basics".
So the `T` tab is deliberately a v1 that will grow, not a finished screen.

[[aello-contextdb]] [[aello-overview]] [[aello-dev-gotchas]]
