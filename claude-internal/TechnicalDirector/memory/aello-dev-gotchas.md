---
name: aello-dev-gotchas
description: "Windows dev gotchas for aello — locked-exe install workaround, and print-mode doesn't load persistent memory"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 72999d30-dfc8-4f50-aac5-cbe588b64645
---

Two non-obvious things when developing/testing aello on Windows:

1. **`cargo install --path . --force` fails with "Access is denied" when any `aello.exe` is running** (live TUI/run sessions hold the binary). Fix: rename the running exe out of the way first — `Rename-Item ~/.cargo/bin/aello.exe aello.exe.old-<tag> -Force` — then `cargo install`. Windows allows *renaming* a running exe (just not overwriting/deleting). aello's startup sweep cleans `aello.exe.old*` later. Alternatively use `aello update` (it does the rename trick internally, pulling the released binary).

2. **Claude Code print mode (`claude -p`, i.e. `aello run <bp> -p "..."`) does NOT inject persistent project memory** (`<env>/projects/<slug>/memory/MEMORY.md`). Only interactive sessions load it. Verified by control test: even an env with memory definitely loaded (TechnicalDirector) reports "NO MEMORY LOADED" under `-p`. So you can't use `-p` to verify a memory-seeding feature — verify by path-identity instead (the seeded `projects/<slug>/memory/` dir must match the slug Claude writes its session `.jsonl` into; `sessions::encode_project_path` produces it). See [[aello-ci-release]].
