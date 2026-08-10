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

## Cost of the scan

Reading every archive is seconds of work, not milliseconds (322 MB / ~6 s on the
machine this was built on). The CLI pays it per invocation. The TUI tab scans
once when first opened — painting a `SCANNING…` frame first, so it does not look
hung — caches the result for the rest of the session, and rescans on `R`.

## Cline envs

A Cline env produces no Claude Code transcripts, so it is skipped rather than
reported as zero. Zero and not-applicable are different answers, and a silent
zero is the failure this codebase keeps hitting.
