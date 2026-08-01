---
name: aello-voice-capability
description: "Cross-project facts behind aello's --voice cap — where revoiced lives, the shared state dir that is the contract boundary, edge-tts is global on this machine, and why existing envs don't adopt it automatically"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4c62a069-53fc-49ff-85fb-83bcc5a0da21
  modified: 2026-08-01T10:17:48.873Z
---

aello's `voice` capability (added 2026-08-01) vendors its TTS hook from **revoiced**, a separate project of the user's. The design rationale lives in aello's `CLAUDE.md` + `docs/capabilities.md` — this note is only the things the repo can't tell you.

1. **revoiced is at `C:\Users\H\Desktop\structuredwork\revoiced`** (github.com/ryha0008-boop/revoiced), developed by its own aello env `RevoicedMainDev`. Source is 4 files under `revoiced/`: `speak.py`, `duck.py`, `win_audio.ps1`, `serve.py`. aello vendors the **first three only** — `serve.py` + `web/index.html` are the voice station, which stays in revoiced.

2. **The voice station is `station.cmd` at the revoiced repo root** (not in the `revoiced/` package dir — easy to miss), which pins port 8778 → <http://127.0.0.1:8778>. Running `serve.py` directly defaults to `--port 0` and picks a random one. There's a "Voice Station" shortcut on the Desktop pointing at it. It's how voice presets are managed; the vendored hook only reads the pool it writes.

3. **`%LOCALAPPDATA%\revoiced\state.json` is the contract boundary** between the two projects, not an implementation detail of either. aello's `voice.rs` writes `global` + `projects` and the `run/stop` token directly; revoiced owns `presets`, `leases`, `duck`. Verified both directions 2026-08-01: a project mute written by `aello voice mute --project` was read back by `speak.py --status` with an identical path key. If either side starts rewriting the file wholesale instead of preserving unknown keys, that breaks silently.

4. **`edge-tts` is installed globally on this machine** (`…\Programs\Python\Python314\Scripts\edge-tts.EXE`), not only in revoiced's `.venv`. That's why vendored copies in arbitrary env dirs still get real Edge neural voices rather than falling back to the SAPI system voice — the fallback path is otherwise easy to mistake for "working".

5. **Existing envs do not adopt `voice` on their own.** The self-heal (`sync_voice_hooks`) only runs inside `place()`, i.e. on the next `aello run` of that env, and only via a freshly `cargo install`ed binary — the same constraint as universal skills in [[aello-dev-gotchas]] #4. The user had ~39 envs with the hook wired in by hand against an absolute path; enabling the cap *replaces* that entry rather than adding beside it, so migration doesn't double-speak, but each env still has to be re-run once. See [[aello-architecture-decisions]].
