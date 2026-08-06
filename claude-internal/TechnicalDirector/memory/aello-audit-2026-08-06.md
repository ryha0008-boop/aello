---
name: aello-audit-2026-08-06
description: "What the 2026-08-06 whole-codebase audit found, which findings were fixed and verified, and which are still open and why"
metadata: 
  node_type: memory
  type: project
  originSessionId: 9e12b565-3a17-4480-a200-4921600893f8
  modified: 2026-08-06T21:58:08.813Z
---

A whole-codebase audit landed as `AUDIT-2026-08-06.md` at the repo root
(gitignored — audits never go to GitHub, see [[aello-dev-gotchas]] #20). 11 high,
24 med, 19 low, over 8,193 Rust lines and 3,713 vendored Python lines.

**Fixed and verified on 2026-08-06** (commits `e6d6ec6`…`a566de9`, 133 tests):

- The `claude-internal/` mirror deleted itself on a fresh clone — `place` now
  restores the env *from* the mirror when there is no env dir, and a missing
  mirror source no longer prunes. Verified end to end on the **installed**
  binary: a memory note and a hand-kept skill survived deleting the env dir.
- `.claude-env-*` is gitignored for **every** role, not just `github`.
- `github-setup` confirms before `git init`, and refuses to stage an env dir or
  a `.credentials.json`.
- `read_to_string(..).unwrap_or_default()` removed from three whole-file
  rewrites.
- `Agent::env_dir` — the five Claude-hardcoded env-dir paths now dispatch on the
  blueprint's agent. Also found doing it: a Cline blueprint on `custom` had its
  `persona.md` deleted on every run.
- Token redaction widened to any line containing `sk-ant-`; both launch paths
  scrub `CLAUDE_CODE_OAUTH_TOKEN`/`ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN`
  from the child; the Cline key is typed hidden.
- The five bundled hooks decode their own stdin. **The stated reason was wrong
  and CodeAuditor caught it**: `sys.stdin.errors` is `surrogateescape` on
  Windows, so `json.load` never raises and the plan-mode block never failed
  open — `tool_name` is ASCII and decodes cleanly either way. The real cost is
  the paths: a mojibaked `transcript_path` does not open (measured,
  FileNotFoundError), so the SessionEnd archive degrades to a pointer. Both
  aello and the audit had asserted a raise without running it.
- `cmd_remove`/`cmd_login_cline` re-read config before saving; `voice.rs`
  fsyncs; `install.sh` verifies `SHA256SUMS`.

**Open, and deliberately so:**

- **Five findings live in vendored revoiced files** (`hooks_speak.py`,
  `hooks_duck.py`, `hooks_focus.py`) and must be fixed upstream, not here, or a
  re-vendor reverts them: the hand-rolled lock is not mutually exclusive (two
  processes can both hold it), `record()` rewrites `history.jsonl` unlocked and
  non-atomically, `lock()` yields `False` on timeout and every caller ignores it,
  `REVOICED_TELEGRAM` is enabled machine-wide from `HKCU\Environment`, and
  `REVOICED_EDGE_TTS` executes any path that exists. See [[aello-voice-capability]].
- **`voice.rs` still writes `state.json` without taking the hook's lock.** The
  choice is: port the lock protocol to Rust, or give aello its own `mute.json`
  the hook reads and never writes. The second removes the class; nobody has
  picked yet.
- **Release signing.** `SHA256SUMS` travels the same channel as the binary, so
  neither `aello update` nor `install.sh` has tamper protection — only
  corruption detection. Minisign over the manifest with the key in the binary is
  the real fix.
- **No repo-root detection anywhere.** `scaffold_project` writes relative to
  `current_dir()`, so `aello run` from a subdirectory seeds `.github/workflows/`
  where CI will never read it.
- **The Python interpreter is never probed.** Hooks are wired as bare `python`;
  on a Windows box without it, `python.exe` is the Store alias and every hook
  fails on every turn while `aello voice status` still prints a version.

**Two of the audit's three "needs a live run" questions are now settled from
data on disk:** `compact_summary` is the right PostCompact field (a real record
holds a 17 KB summary), and `plan-blocked.log` **does not exist in any of the 39
envs** — the plan-mode denial has never fired in the field, so the injected text
is carrying that rule alone so far. Whether SessionEnd carries a `subagent`
field is still unmeasured; 341 archived records over ~3 months is consistent
with main sessions only.
