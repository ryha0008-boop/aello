---
name: aello-token-accounting
description: "`aello tokens` + TUI T tab (2026-08-10): Claude Code transcripts repeat full usage on EVERY content block so dedup by message.id is mandatory (53% of records here are duplicates); the fleet is 3.13B tokens ≈ $2208 at list rates and 98.4% of that is cache READ — but only 70% of the COST, since a 1h cache write is 2x input not cheaper; the subscription quota appears in no transcript so no tool can honestly report '% of plan'; rates re-verified against the published rate card and the arithmetic reproduces to 38 cents"
metadata: 
  node_type: memory
  type: project
  originSessionId: 31c9589c-0e15-403a-8804-45343b56a964
  modified: 2026-08-10T16:17:16.787Z
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
16 envs:** 3.13B tokens total, ≈**$2208** at list API rates. Of that, **cache
read is 98.4%** (3.08B) against 11.3M output and 145k input. Even at 0.1x input
price, cache reads therefore *dominate the cost* — for SysAdmin, $106.77 of its
$153.11 was cache read, vs $21.12 of output. Consequence: shortening assistant
prose is cost-irrelevant (it is already ~0.4% of tokens). Biggest spenders:
TechnicalDirector and AlgoMainDev, ~790M and ~760M.

**Do not read the token share as the cost share — it is the mistake this data
invites.** Same measurement, cost side: cache read $1,539 (70%), cache write
$388 (18%), output $282 (13%), input $0.72 (0.03%). So "98% of it is cache" is
true of tokens and badly wrong as a statement about money — output is 0.4% of
tokens but an eighth of the bill. Related and worth saying out loud: **cache is
not uniformly cheaper.** A read is 0.1x input, but a **1h write is 2x input** —
more expensive than sending the tokens uncached once. Reads win because the same
prefix is re-read many times, not because writes are cheap.

**Every cache write on this machine is 1h, zero 5m** — 38.8M tokens, read from
`usage.cache_creation.ephemeral_1h_input_tokens`. That makes the split-detection
fallback (default to 5m when the sub-object is absent) load-bearing rather than a
tidy-up: if it ever silently took that path, the fleet would be understated by
**$145.63**, since 1h is 2x input and 5m only 1.25x. Check the `cache_write_1h`
field in `--json` is non-zero before trusting a total.

**There is no cache storage ceiling** — checked against the published pricing
page 2026-08-10 because the user reasonably assumed one exists. Nothing meters
stored cache. What actually exists: max 4 `cache_control` breakpoints per
request, a minimum cacheable prefix (512 tokens on Opus 5, up to 4096 on older
models — below it nothing caches, silently), and TTL expiry. The only real upper
bound is the context window. **Read tokens bill per request**, so a 200k cached
prefix re-read across 80 turns bills 16M read tokens — which is why the fleet
total reaches billions without anything being "stored" at that size, and why no
storage cap needs accounting for.

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

**The arithmetic reproduces, independently, twice.** From `aello tokens --json`
against the published rate card: aello says **$2,209.93**, hand-computation says
**$2,210.31**. The 38-cent gap is exactly the Haiku 4.5 / Sonnet 5 traffic priced
at its own cheaper rates instead of Opus 5's — i.e. the per-model split works.
Model census the same day: **25,682 `claude-opus-5`**, 16 `claude-haiku-4-5`, 4
`claude-sonnet-5`, and nothing older. Beware when counting these by grep: a bare
`"model":"sonnet"` in a transcript is usually the **Agent tool's own input
parameter**, not `message.model`, so it never reaches the accounting — parse the
JSON and read `message.model` rather than matching the raw line.

**One known-wrong rate, left in on purpose.** `RATES` has `("claude-opus-4", 5.0,
25.0)` — right for Opus 4.5 through 4.8, but longest-prefix matching also gives
it `claude-opus-4-0` and `claude-opus-4-1`, which are **$15/$75**: a 3x
under-price. Not fixed because the census above found zero such records and the
user said plainly they only run current models. It would only surface if an old
archive were restored. Likewise the **Sonnet 5 intro rate** ($2/$10 through
2026-08-31) was offered again and declined again: the whole Sonnet 5 history here
is **2 messages worth 3 cents**, against a date-keyed rate that would be wrong
from 2026-09-01 in every already-installed binary and cannot self-correct.

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
