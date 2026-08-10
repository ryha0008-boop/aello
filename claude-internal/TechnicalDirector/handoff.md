> Transient resume note (TechnicalDirector). Read on boot, then delete.

# Handoff — 2026-08-10

One feature shipped, committed and pushed. Working tree clean, `main` level with
origin. The value here is §3 (four decisions waiting on the user, three of them
carried forward for days) and §4.

## 1. Read first

- `$CLAUDE_CONFIG_DIR/CLAUDE.md` — this env's persona.
- `$CLAUDE_CONFIG_DIR/projects/C--Users-H-Desktop-work-aello/memory/MEMORY.md` —
  the memory index. Open **`aello-token-accounting.md`** (new this session) and
  **`working-style.md`**. `aello-contextdb.md` and `aello-overview.md` were both
  amended this session.

Read those before acting on this note.

## 2. Shipped this session

- **`e6b169c`** — `feat: report per-env token usage and estimated cost`.
  Pushed. 19 files: new `src/tokens.rs`, `docs/tokens.md`, plus `main.rs`,
  `tui.rs`, `docs.rs`, README, CHANGELOG, CLAUDE.md, `site/lib/docs.ts`,
  `docs/{concepts,workflows,troubleshooting}.md`, and the
  `claude-internal/TechnicalDirector/` mirror.
- Rebased onto **`1ad1903`** (CI's `release: v0.2.27 [skip ci]`) during `/sync`.

State: `main` at `e6b169c`, level with origin, nothing staged or dirty.
`cargo test` **151 passed / 0 failed** (was 142 before this session).
`cargo build --release` green.

**What `aello tokens` does:** per-env input / output / cache-write / cache-read
totals, per-model split, per-session detail (`--sessions`), `--json`, estimated
list-rate cost, and the current 5-hour window. `T` in the TUI is the same data as
a full-screen tab. Reads contextdb (all history, retroactively) plus the live
`<env>/projects/*/` dirs of blueprints placed in the cwd.

## 3. Open threads / next steps

**Decisions the user has been asked for and has not made** — do not decide any of
these alone:

- **The 5-hour denominator.** Shipped as "% of your own peak 5h block" (273M
  here) because the subscription quota is in no transcript. The alternative
  offered was a user-set cap in `config.toml` read as "% of plan". Unanswered.
- **`CHANGELOG.md` `[Unreleased]` is now ~610 lines** covering everything since
  0.2.0, all shipped across 27 tagged releases. **Asked five times now.** Cut a
  `## [0.2.27]` section, or leave it as one running section because CI auto-bumps
  patches and only 0.2.0 was a deliberate line.
- **Cline's last untested leg needs the user's key.** Everything before the
  provider is measured working. `config.toml` has **no `[cline]` section at all**,
  so a valid key returning a real completion is unproven on this machine. The
  user was offered `aello login --agent cline` plus a sub-cent one-shot at
  `minimax/minimax-m3` and did not answer. **Do not add a metered credential on
  their behalf.**
- **Whether a cancelled SessionEnd hook should be made non-silent.** Two options
  were put to the user (a startup check warning when the previous session left no
  contextdb archive, or leave it since the hooks have won the race every time so
  far). No code written either way.

**From this session, actionable:**

- **`aello update` is needed again.** Installed binary is **0.2.27**, built from
  `1ad1903` — it predates `e6b169c`, so the installed copy has **no `aello
  tokens` and no `T` tab**. CI will publish this work as **0.2.28**.
- **Two things I did not verify and said so:** the site build (`npm run build` in
  `site/`) after editing `site/lib/docs.ts`, and the TUI tab in a *live* terminal
  — it was only rendered into an offscreen `TestBackend` buffer. The push touching
  `docs/**` will have triggered the Pages workflow; check it went green.
- **The token tab is a deliberate v1.** The user's own words: "i will want a lot
  of functionality here. filtering, stats, usage per turn, but all that's later,
  for now basics." Do not treat the current screen as finished.

**Carried forward, still open:**

- **Five audit findings belong to vendored revoiced files** and must be fixed
  **upstream in `C:\Users\H\Desktop\structuredwork\revoiced`**, never in aello's
  copy: the hand-rolled lock is not mutually exclusive (`speak.py:241`, companions
  at `:331`/`:257`, same TOCTOU in `duck.py:44-50`); `lock()` yields `False` on
  timeout and `:377`/`:952` ignore it; `record()` rewrites `history.jsonl` unlocked
  and non-atomically (`:963`); `REVOICED_TELEGRAM` is enabled machine-wide from
  `HKCU\Environment` (`:1110`/`:1157`); `REVOICED_EDGE_TTS` executes any path that
  exists (`:788-790`). The note raising these was delivered and consumed.
- **Decision waiting on the user:** `voice.rs` writes the shared `state.json`
  without taking the hook's lock. Port the lock protocol to Rust, or give aello its
  own `mute.json` the hook only reads. The second removes the class rather than
  reimplementing a racy protocol.
- **Open and deliberate:** release signing (minisign over `SHA256SUMS`); no
  repo-root detection anywhere (`scaffold_project` writes relative to
  `current_dir()`); the Python interpreter is never probed (hooks wired as bare
  `python`).
- **CodeAuditor has a note waiting** at
  `C:\Users\H\Desktop\work\code-auditor\CodeAuditor.NOTE.md`. Not a git repo, so
  nothing can commit it; delivered when CodeAuditor next boots.
- **Unmeasured:** whether SessionEnd payloads carry a `subagent` field.
  `plan-blocked.log` exists in **none** of the 39 envs, so the plan-mode denial has
  never fired in the field.
- **Site motion:** the user was twice asked whether they want more than the three
  existing pieces of non-hover motion, and has not answered. Single knob:
  `--brand-animation-duration-default` in `site/app/globals.css`.

## 4. What was measured this session

- **Claude Code transcripts repeat the full `usage` object on every content
  block.** 266 usage-bearing records for 173 distinct `message.id`s on one real
  transcript — counting records overstates output by **68%** (218,607 vs 129,877).
  Across the whole archive here: **17,216 of 32,350 records are duplicates (53%)**.
  Dedup by `message.id` is mandatory for anything reading these files. This also
  makes overlapping sources safe (17 of 122 archives are a session archived twice;
  every live transcript overlaps its own later archive).
- **The fleet's token shape is counter-intuitive.** 3.06B tokens across 16 envs,
  ≈**$2140** at list rates, of which **~98.5% is cache READ** (3.00B) against 11M
  output and 144k input. Cache reads dominate cost even at 0.1x input price.
  Shortening assistant prose is cost-irrelevant. Cache writes here are almost all
  the **1h** TTL bucket (2x input), not 5m.
- **The subscription quota appears in no transcript.** No tool reading these files
  can honestly report "% of your limit" — anything that does invented the
  denominator.
- **Cost arithmetic verified independently to the cent** (SysAdmin `$153.1083`
  recomputed by hand = reported), and the dedup **reproduced a hand PowerShell
  measurement** of the same transcript (173 messages, 129,877 output tokens).
- **Scan cost: 322 MB / 6.2 s**, 220 files. Fine per CLI call, fatal per TUI
  frame — hence the cache on `App::tokens` and the `SCANNING…` frame painted
  *before* the scan.

## 5. Gotchas

**New this session:**

- **Rendering a ratatui screen into `TestBackend` and printing the buffer is how
  you *see* a TUI change without a terminal.** It caught two real layout bugs the
  assertions would not have (env names truncating at a 40-col pane, a `DUR` column
  that actually showed wall-clock span). Worth reaching for on any TUI work.
- **`site/lib/docs.ts` keeps its own copy of `docs.rs::ORDER` plus a blurb map.** A
  new `docs/*.md` appears on the site without it, but sorts last with no blurb —
  a silent half-failure. Update both when adding a doc.
- **`<synthetic>` is a real model id in transcripts** (Claude Code's marker for
  local messages) carrying zero usage. It correctly matches no rate table entry;
  that is not a gap.

**Standing traps that still apply:**

- **`cargo install --path . --force` fails while any `aello.exe` runs** — rename
  it aside first.
- **Test the installed binary, not the checkout.**
- **`git restore Cargo.lock` before staging** — it re-dirties itself within seconds
  of any build and CI never updates it, so the lockfile permanently lags one
  version. Normal, and done again this session.
- **Never add `--locked` to the CI test job.**
- **`git pull --rebase --autostash`** — plain `--rebase` trips over the dirty
  lockfile.
- **Ports 3000–3002 are taken here** and Next slides silently past an occupied
  port; the site launcher pins 4310.
- **`AUDIT-2026-08-06.md`** at the repo root is gitignored working notes; it holds
  the full write-ups behind the revoiced findings in §3.
- **Do not commit this file.** `*.HANDOFF.md` and `*.NOTE.md` are gitignored in
  *this* repo (`dec3fd9`). The tracked crossing is
  `claude-internal/TechnicalDirector/handoff.md` — which currently holds the
  **2026-08-08** note, not this one, because `/sync` ran *before* `/handoff` this
  session. This note reaches a second machine only after the next `/sync`.
