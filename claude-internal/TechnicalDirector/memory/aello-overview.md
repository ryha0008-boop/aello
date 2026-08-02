---
name: aello-overview
description: "aello = isolated Claude Code envs (venvs for agents); Claude-only, subscription-auth, cross-platform; ground-up rebuild of helo"
metadata: 
  node_type: memory
  type: project
  originSessionId: afdce7ab-1496-4a48-a656-d27d377c3496
---

aello (`C:\Users\H\Desktop\work\aello`, repo github.com/ryha0008-boop/aello, public, branch `main`) is a ground-up rebuild of an older tool `helo` (now frozen at `Desktop/helo-win` — don't copy its patterns blindly; providers/API-keys/pi/opencode/auto-hooks were deliberately dropped). Claude-only, subscription auth, cross-platform Linux + Windows x86_64 (macOS source-only). Also runs on a Linux VPS (`devuser@vps-main`, installed `~/.local/bin/aello`, sudo-free updates). A **blueprint** = reusable agent identity (name, model, optional persona, capabilities) in config.toml; placing it in a project creates `<project>/.claude-env-<name>/` as Claude's CLAUDE_CONFIG_DIR. Working/verified at v0.1.57 (2026-08-02): add/list/remove/edit/run/init/login/github-setup/docs/voice/update + TUI, built-in personas (coder/sysadmin), five per-blueprint capabilities, generated /sync plus the universal /handoff, /note and /twosentences skills, git attribution, unified contextdb, self-update, a static landing page in site/, README + docs/ + repo CLAUDE.md. The voice is **not** a capability — every env speaks, and silence is `aello voice mute` at runtime. See the repo's CLAUDE.md for architecture + src module map. [[aello-architecture-decisions]] [[aello-ci-release]]
