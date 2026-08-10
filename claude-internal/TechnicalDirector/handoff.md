> Transient resume note (TechnicalDirector). Read on boot, then delete.

# Handoff — 2026-08-08 (evening)

A short session: no code changed. One docs+memory commit, pushed and clean. The
value is in §3 and §4 — most of this session was **measurement**, and several
long-standing claims moved. §3 carries forward threads from the previous note,
which was consumed on boot, so **this is the only copy outside contextdb**.

## 1. Read first

- `$CLAUDE_CONFIG_DIR/CLAUDE.md` — this env's persona.
- `$CLAUDE_CONFIG_DIR/projects/C--Users-H-Desktop-work-aello/memory/MEMORY.md` —
  the memory index. Open **`cline.md`** (entry **#20 is from this session** and
  corrects #13/#15), **`aello-contextdb.md`** (last bullet, new), and
  **`working-style.md`**. `aello-dev-gotchas.md` **#43** is new.

Read those before acting on this note.

## 2. Shipped this session

- **`eae1d4d`** — `docs: scope the Cline isolation claim to what was actually
  measured`. 7 files, docs + memory only, **pushed**. Working tree clean.
- Pulled **`cc85995`** (CI's `release: v0.2.26 [skip ci]`) at the start — the
  branch had been one behind since the previous session.

State: `main` at `eae1d4d`, level with origin, nothing staged, nothing dirty.
**`cargo test` was not run** and did not need to be — no Rust changed. Last known
green: 142 local / 143 Linux CI at `5c1f067`.

## 3. Open threads / next steps

**From this session:**

- **Cline's last untested leg needs the user's key.** Everything before the
  provider is now measured working (see §4). What is unproven on *this machine* is
  a valid key returning a real completion, because `config.toml` has **no
  `[cline]` section at all**. The user was offered `aello login --agent cline`
  plus a sub-cent one-shot at `minimax/minimax-m3` and did not answer. Do not add
  a login on their behalf — it is a metered credential.
- **`aello update` still has not been run.** Installed binary is **0.2.25**;
  `aello restore`, the memory-union fix and the handoff-into-the-mirror snapshot
  are all in **0.2.26**. Three consequences right now: the installed copy still
  *prunes* mirror-only memory notes on every launch, this env's placed skills are
  still the pre-0.2.26 text (the `/handoff` skill still says the note is "never
  committed" full stop, which `5c1f067` scoped to the root file), and **this very
  handoff will not be snapshotted into `claude-internal/TechnicalDirector/`**
  until an `aello run` happens on 0.2.26 or newer.
- **`CHANGELOG.md` `[Unreleased]` is ~575 lines** covering everything since 0.2.0,
  all shipped across 26 tagged releases. **The user has now been asked four times
  and has not answered**: cut a `## [0.2.26]` section, or leave it as one running
  section because CI auto-bumps patches and only 0.2.0 was a deliberate line.
  Do not decide it alone.
- **Offered and unanswered: whether to make a cancelled SessionEnd hook
  non-silent.** Two options were put to the user — a startup check that warns when
  the previous session left no contextdb archive, or leave it, since the hooks
  have won the race every time so far. No code was written either way.

**Carried forward, still open:**

- **Five audit findings belong to vendored revoiced files** and must be fixed
  **upstream in `C:\Users\H\Desktop\structuredwork\revoiced`**, never in aello's
  copy: the hand-rolled lock is not mutually exclusive (`speak.py:241`, companions
  at `:331`/`:257`, same TOCTOU in `duck.py:44-50`); `lock()` yields `False` on
  timeout and `:377`/`:952` ignore it; `record()` rewrites `history.jsonl` unlocked
  and non-atomically (`:963`); `REVOICED_TELEGRAM` is enabled machine-wide from
  `HKCU\Environment` (`:1110`/`:1157`); `REVOICED_EDGE_TTS` executes any path that
  exists (`:788-790`). The note raising these was **delivered and consumed** — the
  file is gone from that repo's root, so RevoicedMainDev has seen them.
- **Decision waiting on the user:** `voice.rs` writes the shared `state.json`
  without taking the hook's lock. Port the lock protocol to Rust, or give aello its
  own `mute.json` the hook only reads. The second removes the class rather than
  reimplementing a racy protocol. (Unchanged by this session's `board` work, which
  was about *key preservation*, not locking.)
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

All four are in memory; this is the index, not the detail.

- **The `aello run` → Cline chain works end to end minus a valid key.** Isolated
  `AELLO_CONFIG_DIR` + scratch project + a deliberately bogus OpenRouter key,
  against the **installed** 0.2.25 binary: `place` → `cline auth` (key lands in
  `<env>/data/settings/providers.json`) → launch → OpenRouter answers
  `User not found.` Sessions, db, logs and cache all stayed inside the env dir;
  `.gitignore` got `.cline-env-*`. The missing-login guard was also measured, from
  the real config, and names its own fix. **`UnderdogerDev` at
  `C:\Users\H\Desktop\ai-tools\underdoger` has rules/skills/memory but no `data/`
  dir — placed, never launched.**
- **Correction: `~/.cline` is NOT untouched by an isolated run.** It creates
  `~/.cline/cli-node-extra-ca-certs.pem` (187 KB node CA bundle) when absent,
  whatever the isolation flags say. No credentials or per-env state cross over.
  This is what `eae1d4d` documents.
- **aello preserves revoiced's `state.json` keys by round-trip, not by allowlist.**
  Answered RevoicedMainDev's question about its new top-level `board` key by
  running the installed binary's `mute`/`mute --project`/`unmute` over a
  `board`-bearing state file in an isolated `LOCALAPPDATA`. It survives intact.
  Nothing to register, now or for the next key they add. Reply delivered as a
  `/note`.
- **"SessionEnd hook … failed: Hook cancelled" is a shutdown race, not a loss.**
  Investigated an AlgoMainDev exit: both hooks reported cancelled and both had
  finished — full 2.0 MB transcript archived, handoff embedded, no leaked lease.
  Timed at 0.29 s and 0.72 s, so not a timeout. **The same message would appear on
  a real loss**, so contextdb is the only answer.

## 5. Gotchas

**New this session:**

- **NTFS *file tunneling* re-uses a deleted file's CreationTime for ~15 s.** A
  file deleted and recreated by the very run you are testing reports the *old*
  creation time, which reads as "the run did not create it". **Compare hashes, not
  timestamps** when the question is "did this operation produce this file".
  (`aello-dev-gotchas.md` #43.)
- **`Set-Content -Encoding utf8` in PowerShell 5.1 writes a BOM**, and both
  `voice.rs` and `speak.py` reject a BOM'd `state.json` as corrupt. If a state-file
  test fails at "not valid JSON", suspect the harness first —
  `[System.IO.File]::WriteAllText` with `UTF8Encoding($false)` is the fix.
- **`AELLO_CONFIG_DIR` + a scratch project is how to exercise a metered agent
  safely.** A deliberately invalid key drives every step aello owns and stops at
  the provider, costing nothing. Scratch dirs from this session are under
  `…\c4ea3199-…\scratchpad\` (`clinetest-config`, `clinetest-proj`, `fakelocal`).

**Standing traps that still apply:**

- **`cargo install --path . --force` fails while any `aello.exe` runs** — rename
  it aside first.
- **Test the installed binary, not the checkout.**
- **`git restore Cargo.lock` before staging** — it re-dirties itself within seconds
  of any build and CI never updates it, so the lockfile permanently lags one
  version. Normal.
- **Never add `--locked` to the CI test job** — the bump commits a `Cargo.toml`
  version without touching `Cargo.lock`.
- **`git pull --rebase --autostash`** — plain `--rebase` trips over the dirty
  lockfile.
- **Ports 3000–3002 are taken here** and Next slides silently past an occupied
  port; the site launcher pins 4310. A dev server may be running on it — the
  user's.
- **`AUDIT-2026-08-06.md`** at the repo root is gitignored working notes; it holds
  the full write-ups behind the revoiced findings in §3.
- **Do not commit this file.** `*.HANDOFF.md` and `*.NOTE.md` are gitignored in
  *this* repo (`dec3fd9`). The tracked crossing is
  `claude-internal/<name>/handoff.md`, written by `place` on 0.2.26+ — which, as
  §3 says, is not installed yet.
