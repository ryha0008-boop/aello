---
name: aello-contextdb
description: "contextdb: SessionEnd does all the archiving (PostCompact is dormant, not broken), transcripts are copied not referenced, thinking is captured as summaries, and raw reasoning is unextractable"
metadata: 
  node_type: memory
  type: project
  originSessionId: 07544da2-e8b0-49d8-bd4c-33fbb89ffcc6
  modified: 2026-08-03T04:54:00.397Z
---

Audited end-to-end on **2026-08-03** after the user suspected the PostCompact hook was broken. Findings, all measured against the live contextdb at `C:\Users\H\Desktop\work\contextdb`:

- **PostCompact is dormant, not broken — do not "fix" it.** It fires only on compaction (auto or `/compact`). With a 1M context window and a `/sync` → `/handoff` → `/clear` workflow, compaction effectively never happens. contextdb held **265 SessionEnd records and zero PostCompact records for any aello blueprint**; every one of the 65 PostCompact files belongs to a *helo-era* blueprint (`claude-vanilla`, `chinese-claude`, `claude-simple`) and the newest is 2026-06-15. Those old files prove the script works — `trigger: "auto"`, 7 KB analysis + 15 KB summary parsed correctly. It stays seeded for workflows that do compact.

- **SessionEnd does all the real work**, and `/clear` does fire it: 121 `prompt_input_exit`, 107 `clear`, 37 `other`.

- **The archive stored transcripts by *path*, and the path decayed.** Claude Code deletes its own session files after `cleanupPeriodDays` (**default 30**), and the env dir is gitignored and `--purge`-able. 15% of 265 archives already pointed at nothing, with a clean cliff at the 30-day mark (6–14% dead under 30 days, **44% at 30–39**). Nothing errored — the archive quietly stopped being one, which is this codebase's signature failure shape. Fixed two ways: SessionEnd now **copies** the transcript to `<ts>_<session>_transcript.jsonl` (reporting the result in `transcript_archived`), and `place()` sets `cleanupPeriodDays = 365`, self-healed into existing envs **only when absent** so a user's own value stands. Already-dead references cannot be recovered. Expect growth: transcripts are 1.3 MB median, tens of MB at the tail.

- **Windows `MAX_PATH` silently defeats the copy on long paths.** The encoded-cwd directory repeats the whole project path, so a deep project pushes the transcript past 260 chars (long paths are off by default: `LongPathsEnabled = 0`). Measured at **325 chars** — `open()` failed, the guard recorded `transcript_archived: ""` honestly, and the archive degraded back to a pointer with no error. Both paths now take the `\\?\` prefix. Real projects are short enough that this was latent, but it bit immediately in a scratchpad path — and it also breaks `Get-ChildItem`/PowerShell traversal, so debugging there needs the same prefix.

- **Thinking is now captured, as summaries — and raw reasoning is unextractable.** Every env launches with `--thinking-display summarized` (`launch.rs::THINKING_DISPLAY`). The API default is `omitted`: thinking blocks arrive with an **empty** `thinking` field and only an opaque `signature`, which is what transcripts stored (measured: 2,842 blocks across 53 transcripts, every one empty). **The signature is not ciphertext for the reasoning** — 369 bytes, not gzip, not zlib, 45% printable; it is an integrity token the API validates on replay. There is nothing to decrypt, and the raw chain of thought is never returned on any model, so a summary is the ceiling rather than a compromise. `display` controls visibility only — thinking happens and is billed identically — so this costs nothing.

- **`--thinking-display` is a real CLI flag but is undocumented in `claude --help`.** Found by grepping the `claude` binary (a 265 MB native exe — `grep -a` works on it, and it's also how the four `*THINKING*` env vars and the hook-event names were confirmed). Registered alongside `--thinking` and `--max-thinking-tokens`. Don't conclude a Claude Code capability is absent from `--help` alone.

**What a transcript actually contains** (measured on a 6.1 MB, 880-line session): every tool call with full inputs (195), every tool result in full (2.27 MB total, largest single result 626 KB — no truncation cap), all assistant/user text, file-history snapshots and deltas, attachments and images, and the full `uuid`/`parentUuid` causal chain. So the archive is a complete record of what was *done*; reasoning arrives only as the summaries above.

[[aello-overview]] [[aello-dev-gotchas]]
