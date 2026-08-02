---
name: aello-voice-capability
description: "Cross-project facts behind aello's --voice cap — where revoiced lives, the shared state dir that is the contract boundary, edge-tts is global on this machine, and why existing envs don't adopt it automatically"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4c62a069-53fc-49ff-85fb-83bcc5a0da21
  modified: 2026-08-02T20:38:56.835Z
---

aello's `voice` capability (added 2026-08-01) vendors its TTS hook from **revoiced**, a separate project of the user's. The design rationale lives in aello's `CLAUDE.md` + `docs/capabilities.md` — this note is only the things the repo can't tell you.

1. **revoiced is at `C:\Users\H\Desktop\structuredwork\revoiced`** (github.com/ryha0008-boop/revoiced), developed by its own aello env `RevoicedMainDev`. Source is 4 files under `revoiced/`: `speak.py`, `duck.py`, `win_audio.ps1`, `serve.py`. aello vendors the **first three only** — `serve.py` + `web/index.html` are the voice station, which stays in revoiced.

2. **The voice station is `station.cmd` at the revoiced repo root** (not in the `revoiced/` package dir — easy to miss), which pins port 8778 → <http://127.0.0.1:8778>. Running `serve.py` directly defaults to `--port 0` and picks a random one. There's a "Voice Station" shortcut on the Desktop pointing at it. It's how voice presets are managed; the vendored hook only reads the pool it writes.

3. **`%LOCALAPPDATA%\revoiced\state.json` is the contract boundary** between the two projects, not an implementation detail of either. aello's `voice.rs` writes `global` + `projects` and the `run/stop` token directly; revoiced owns `presets`, `leases`, `duck`. Verified both directions 2026-08-01: a project mute written by `aello voice mute --project` was read back by `speak.py --status` with an identical path key. If either side starts rewriting the file wholesale instead of preserving unknown keys, that breaks silently.

4. **`edge-tts` is installed globally on this machine** (`…\Programs\Python\Python314\Scripts\edge-tts.EXE`), not only in revoiced's `.venv`. That's why vendored copies in arbitrary env dirs still get real Edge neural voices rather than falling back to the SAPI system voice — the fallback path is otherwise easy to mistake for "working".

5. **How to verify voice without an interactive session.** `aello run <bp> -p "..."` places the env *and* fires the `Stop` hook — print mode doesn't load persistent memory ([[aello-dev-gotchas]] #2) but it does load the persona and run hooks, so the TL;DR gets written and spoken. The receipt is `%LOCALAPPDATA%\revoiced\history.jsonl`: each entry records `project`, the `voice` preset that spoke, the text, and the `audio` mp3 path. An entry naming a real preset means edge-tts synthesis worked; `"system fallback voice"` means it silently fell back to SAPI.

6. **Verified working end to end 2026-08-01.** A fresh `voicetest` project spoke its TL;DR aloud using preset `negras`, while the revoiced env minutes earlier had `Andrew calm` — i.e. the per-session leasing really does give concurrent envs different voices. The generated `settings.json` contained `$CLAUDE_CONFIG_DIR/hooks/speak.py` in both `Stop` and the second `SessionEnd` group, with no absolute path anywhere in the file.

7. **Existing envs do not adopt hook changes on their own.** The self-heal (`sync_voice_hooks`) only runs inside `place()`, i.e. on the next `aello run` of that env, and only via a freshly `cargo install`ed binary — the same constraint as universal skills in [[aello-dev-gotchas]] #4. It *replaces* a hand-wired entry rather than adding beside it, so migration doesn't double-speak, but each env still has to be re-run once. See [[aello-architecture-decisions]].

8. **The voice stopped being a capability on 2026-08-02 — every env speaks, unconditionally.** The user's reasoning: they would never want a project mute, and if they did they would mute it at runtime. So `--voice`/`--no-voice`, the TUI row and the wizard question are gone, `Capabilities` is five bools, and `sync_voice_hooks` has no deregister branch. Before this, only a blueprint with `voice = true` ever got a vendored hook (exactly five did), which is why ~42 envs were still hand-wired to the absolute path `…/structuredwork/revoiced/revoiced/speak.py` and quietly stayed that way — that asymmetry confused two sessions in a row, so do not reason from an old note about it. **All 38 aello-placed envs were backfilled by hand the same day**, verified 38/38 wired with zero absolute paths left. Config lives at `%APPDATA%\aello\config\config.toml` — note the extra `config/` level, `find` in the obvious places misses it.

9. **Upstream `speak.py` imports `focus` and `notify`, but guarded, specifically so aello can keep vendoring three files.** Added upstream in `a86023a` (`try` / `ImportError` → `SimpleNamespace` stubs) after aello hit it. Both are station-side and must **never** be vendored — `notify` raises Windows toasts pointing at the station and registers a `revoiced://` handler under `HKCU`, which has no business running from inside 40 env dirs. Same rule as `kie.py`, `launch.py`, `serve.py`. When re-vendoring, take the three files from a **commit sha**, never the working tree: revoiced's tree is often dirty mid-session, and a note describing its shape can be stale by the time it's read (it was, twice — both times claiming "no new sibling import" while there were two).
