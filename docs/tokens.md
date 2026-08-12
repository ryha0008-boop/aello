# Token accounting

`aello tokens` reports how many tokens each env has spent, split into the four
buckets that price differently, plus an estimated cost. The TUI has the same
data on its own tab (`T`).

Nothing has to be enabled: this reads the transcripts contextdb already
archives. Turning it on retroactively works — the history was being recorded
before the feature existed.

```
aello tokens                 # per-env totals + the current 5-hour window
aello tokens <name>          # one env
aello tokens --sessions      # per-session breakdown
aello tokens --stats         # projects ranked by tokens/session + where the money goes
aello tokens --json          # machine-readable
```

## Where the numbers come from

Every assistant message in a Claude Code transcript carries a `usage` object:

| Field                         | Bucket        | Priced at        |
| ----------------------------- | ------------- | ---------------- |
| `input_tokens`                | input         | model input rate |
| `output_tokens`               | output        | model output rate|
| `cache_creation_input_tokens` | cache write   | 1.25× input (5m TTL) / 2× input (1h TTL) |
| `cache_read_input_tokens`     | cache read    | 0.1× input       |

The 5m/1h split comes from the `cache_creation` sub-object when present; older
transcripts without it are counted as 5m, the API's default TTL.

**That split is load-bearing, not a detail.** Measured here 2026-08-10: every one
of 38.8M cache-write tokens landed in the **1h** bucket, none in 5m. Had the
sub-object been missed and the 5m fallback taken, the fleet total would have been
understated by $145.63 — a 1h write is 2× input, a 5m write only 1.25×.

**Token share is not cost share**, and the gap is wide enough to mislead. On the
same measurement: cache read is 98.4% of tokens but 70% of cost; cache write is
1.2% of tokens but 18% of cost; output is 0.4% of tokens but 13% of cost. Reads
dominate either way, but "almost everything is cache" is a statement about
tokens, not about money.

There is **no ceiling on how much can be cached** — nothing meters cache storage.
What exists is a cap of 4 cache breakpoints per request, a minimum cacheable
prefix below which nothing is stored, and the 5m/1h expiry. Read tokens are
billed per request, so one cached prefix re-read across a long session accrues
read tokens without anything being "stored" repeatedly. That is why the fleet
total reaches billions of read tokens and why no storage limit needs accounting
for.

Two directories are read:

- **contextdb** (`<contextdb>/<project>/<blueprint>/`) — every session that has
  ended, on any project, going back as far as the archive does.
- **The env dirs placed in the current directory** (`<env>/projects/*/`) — the
  live transcripts, so a session in progress is included rather than appearing
  only after it ends.

Sessions running in *other* projects are therefore missing until they end. The
CLI says so in its footer. contextdb records a project's folder name, not its
path, so there is no way to locate those env dirs from here.

## Deduplication is load-bearing

Claude Code writes **one transcript record per content block**, and every record
for a message repeats that message's full `usage`. Summing records instead of
messages roughly doubles the answer — measured on a real transcript here, 266
usage-bearing records for 173 distinct messages, overstating output by 68%
(218,607 vs 129,877).

`aello tokens` deduplicates on `message.id` before summing. The same dedup
absorbs two other overlaps for free: a session archived twice (compaction *and*
session-end), and the overlap between an archive and the still-live transcript
it was copied from. That is what makes reading both sources safe.

The CLI prints how many duplicates were collapsed. If that number is ever zero
on a real archive, the dedup has stopped working — it is not a clean run.

## Cost is an estimate, not a bill

Costs are **list API rates applied to subscription usage**. The figure answers
"what would this have cost on the API", not "what were you charged". An aello
env authenticates on a Claude subscription; no per-token charge exists to
report.

Rates are compiled into the binary and matched to a model id by longest prefix,
so a dated id (`claude-haiku-4-5-20251001`) resolves to its family. Sonnet 5 is
priced at its standard $3/$15 rather than the $2/$10 introductory rate — a price
keyed to an expiry date would go quietly wrong the morning after, and already
installed copies would keep reporting the old number.

**Known limitation of that prefix match:** the `claude-opus-4` entry is $5/$25,
correct for Opus 4.5 through 4.8, but it also swallows `claude-opus-4-0` and
`claude-opus-4-1`, which are **$15/$75** — a 3× under-price. Deliberately not
split: verified 2026-08-10 that no such transcript exists on this machine
(25,682 Opus 5 records, 16 Haiku 4.5, 4 Sonnet 5, and nothing older), and these
models are retired on the first-party API. It would only matter if an archive
from an older era were ever restored.

**A model with no rate entry is never priced at zero.** Its tokens are counted
into a separate `unpriced` total and the model id is named in the output. A cost
that silently excludes part of the traffic is worse than no cost at all. (The
`<synthetic>` model id that Claude Code uses for local markers carries no usage,
so it shows up with zero tokens and no cost — that is correct, not a gap.)

## The 5-hour window

Claude's rate limit runs in 5-hour blocks. A block starts at the containing hour
of its first message and closes after five hours, or after a five-hour gap with
no traffic — an overnight pause and the next morning's work are two blocks, not
one long one.

Blocks are computed **across every env**, because the quota is machine-wide: all
envs share one subscription token. The per-env split inside the current block is
what tells you which env is eating it.

**The percentage is against this machine's own peak block, not a quota.** The
subscription's actual limit does not appear in any transcript — aello cannot
read it and does not guess. The largest 5-hour block ever recorded here is the
only honest denominator available, and the output names it so the number is not
mistaken for "% of your plan".

## The statusline: the same numbers, in the session

Every placed env registers `aello statusline` as its Claude Code `statusLine`,
so the readout sits under the prompt and updates as the conversation does (up to
about three times a second):

```
204k·$9.95 │ 5h·42%·34M·1h57m │ 7d·20%·527M·5d18h
this·6.57M·$4.08 │ sess·20M·$14.14 │ prjt·926M·$638.27
```

Two rows: **ceilings on top, spend beneath.** The model and the effort are not
shown — the session already knows what it is running, and the space is worth
more as numbers.

Row 1 is context tokens (red) and this session's cost, then each plan window as
*percentage · tokens · time to reset*. Row 2 is `last` / `this` turn, `sess`
and `prjt`, each as tokens and cost. A segment with no data is dropped rather
than drawn as zero, so an API-key session (no `rate_limits`) simply shows fewer
of them.

Colour carries the meaning: context red, money green, a plan window green under
80% and **red at or above it**. Past 100% the whole spend row turns red,
because at that point the spend is the thing to look at.

**Row 1's two halves come from different worlds, and neither can answer the
other's question.**

The **percentage** is the *subscription's own* utilisation, arriving in the
`anthropic-ratelimit-unified-*` response headers and passed to the statusline by
Claude Code. **No transcript carries it**, which is exactly why the 5-hour
section above is measured against this machine's peak block: that is what aello
can do without this number, and this row is what it can do with it.

The **token count** beside it is aello's own, summed from transcripts over the
same window the percentage is measured against (the window start is derived from
the payload's `resets_at`). It is **raw tokens, machine-wide**, and it is *not*
the quantity Anthropic is metering — a percentage and a token count in one
segment look like they should divide into a quota, and they do not. It can also
only under-report: a session running in another project right now is unreadable
until it ends, the same limit the rest of this page documents.

Row 2 is transcripts throughout, deduped by `message.id` like everything else
here. The session figure reproduces `aello tokens --sessions` for the same
session.

**A turn ends at a user prompt, and that is narrower than it sounds.** Claude
Code writes every tool result back as a `user` record, a subagent's whole
conversation is interleaved into the same file, and an interrupt lands as a
`user` record too. Measured across every transcript in this project: 4,539 tool
results, 313 typed prompts, 21 prompts carrying a pasted image, and 22
`[Request interrupted by user]` markers. Only the last two shapes plus plain
text start a turn — treating an interrupt as a boundary inserts an empty turn,
and "last turn" then reads as nothing at the moment you most want it.

The project total is cached for 180 seconds (`<env>/statusline-cache.json`),
because a full scan is ~0.8s and the statusline runs far too often to pay that.
Everything narrower — this turn, the last turn, the session — is re-read from
disk every render. Deleting the cache file costs one slow render.

To turn it off, remove the `statusLine` key from `<env>/settings.json` — but the
next `aello run` puts it back, so pointing it at your own command is the durable
opt-out. A `statusLine` aello did not write is never replaced.

`aello check` **runs** it rather than reading the setting: a statusline that
fails renders nothing, logs to a debug file nobody opens, and looks identical to
one that works.

## Statistics and charts (`S` in the TUI, `--stats` on the CLI)

`S` on the tokens tab opens a second page over the same scan; `aello tokens
--stats` prints the same numbers as text so they can be checked by hand.

**Token-hungry projects** ranks by **tokens ÷ sessions** — how expensive it is
to *engage* with a project, not how much it has been used. A project with one
enormous session outranks one with twenty small ones even when the second has
spent more in total; that inversion is the metric working, not a bug. Sessions
÷ tokens would rank identically and read as `0.0000001`, so it is not offered.
Treat a one-session project as noise: an average of one sample is a sample.

**Where the money goes** puts the token share and the cost share of each bucket
side by side, because they disagree violently — measured here: cache read is
98.3% of tokens and 69.1% of cost, output 0.4% of tokens and 12.4% of cost.
Reading only the token split is how "cache is cheap" becomes a wrong conclusion.

**Daily** is a 30-day sparkline that keeps its empty days: a gap is a fact, and
a chart that closes it invents a month of steady work. **Hour of day** is
bucketed in **UTC** — there is no timezone crate here and guessing an offset
would silently shift every bar.

### Four ways to split the same spend

**Spend by branch** (`gitBranch`) and **spend by reasoning effort** (`effort`)
are the same table twice. Both fields appear only on newer records, so an absent
value gets its own **`(unrecorded)`** row rather than being folded into the
commonest one — measured here, that row is 18.8% of all cost, which is not a
rounding error to hide inside `high`. A split with one row is dropped entirely:
"100% on main" is not a finding. `HEAD` means a detached checkout and is left as
itself.

**Models over time** draws one sparkline per model over the charted window, so a
migration reads as a handover instead of one blended average — the
`claude-opus-4-8` → `claude-opus-5` switch on 2026-07-27 is visible as two
crossing curves. A model with no tokens in the window is listed with its
first/last-seen dates but **not** charted: an all-blank sparkline reads as a
broken chart, and `<synthetic>` records carry an empty usage object.

**Context nobody typed** counts what the *harness* pushed into the conversation
— task reminders, hook output, skill listings, agent and tool listings, nested
memory. Measured here: 8,965 injections, and `hook_additional_context` (which is
where aello's own per-turn rules and SessionStart block land) averages ~454
tokens across 1,335 injections.

That token figure is **characters ÷ 4, an estimate**, and it is labelled as one
on both surfaces. The transcript records what was injected and never what it
tokenised to, so this is the ceiling on what can honestly be said — do not add
it to a total that came from a `usage` field.

### What the sessions *did*

The same scan also reads the half of the transcript that has nothing to do with
money, because the files are open anyway and a second walk over a gigabyte to
count tool calls would be pure waste.

**Turns** come from Claude Code's own `turn_duration` records, so the length of
a turn is measured wall clock and not a difference between two timestamps —
which would quietly include however long you spent reading the last answer. That
is also why the page reports **hours inside turns** rather than elapsed session
time, and prices per hour of *that*. Measured here: 2,786 turns, median 1m44s,
p90 10m09s, longest 1h31m, 193 hours inside turns.

**Tool mix, most-edited files and shell verbs** come from the `tool_use` blocks.
**Skills actually run** comes from `attributionSkill`, which the harness stamps
on assistant messages — it is the only evidence a seeded skill was *run* rather
than merely seeded, and it is counted two ways because they answer different
questions: sessions (how often it was reached for) and messages (how much work
it did). Here: `/handoff` in 213 sessions, `/sync` in 207, `/twosentences` in 19.

**Interrupts** (`[Request interrupted by user]`) and **queued prompts** are the
friction signals — 7.4% of turns interrupted, and 277 of 528 queued prompts
withdrawn before they ran.

Every counter deduplicates on an id its own record carries, and the key matters
more than it looks:

- **Tool calls key on the `toolu_…` id, not `message.id`.** Claude Code writes
  one record per content block and repeats the message id on each, so a
  message-level key keeps the first block — usually `thinking` — and drops every
  tool call the turn made.
- **A queue record has no `uuid` at all.** Keying on one counted **0 of 665**,
  which is indistinguishable from a user who never queues a prompt. It was
  caught by comparing the output against a hand count of the same files, which
  is the only reason it is not still reading zero.

### Two ways these totals are incomplete, both stated on the page

- **Pointer-only archives.** SessionEnd used to record only a *path* to the
  transcript; copying the file came later. Claude Code deletes its own session
  files on a retention timer, so those archives
  point at nothing and contribute **zero tokens**. The count is printed; it is
  not folded into the total as if it were a gap in the work.

  This was not hypothetical. On the machine this was written on, **269 of 415**
  archived sessions were pointer-only and the reported total was **3.94B tokens
  / $2806**. For **226** of them the original transcript was still sitting in
  its env dir, never copied — backfilling those 698 MB moved the true total to
  **7.41B / $5575 across 293 sessions**. Half the history was invisible and read
  as quiet months. If this count is ever non-zero, check whether the originals
  still exist before writing the history off.
- **The live half is cwd-scoped.** contextdb holds ended sessions; unarchived
  ones are only readable in the env dirs of the directory you are standing in.
  Running from `~` instead of a project dropped 430M tokens and 31 days of span
  here, and moved `$/day` from $72 to $315. Run it from the project.

## Cost of the scan

Reading every archive is seconds of work, not milliseconds (322 MB / ~6 s on the
machine this was built on). The CLI pays it per invocation. The TUI tab scans
once when first opened — painting a `SCANNING…` frame first, so it does not look
hung — caches the result for the rest of the session, and rescans on `R`.

## Cline envs

A Cline env produces no Claude Code transcripts, so it is skipped rather than
reported as zero. Zero and not-applicable are different answers, and a silent
zero is the failure this codebase keeps hitting.
