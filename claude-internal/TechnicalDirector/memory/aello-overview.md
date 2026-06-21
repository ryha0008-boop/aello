---
name: aello-overview
description: "aello = isolated Claude Code envs (venvs for agents); Claude-only, subscription-auth, cross-platform; ground-up rebuild of helo"
metadata: 
  node_type: memory
  type: project
  originSessionId: afdce7ab-1496-4a48-a656-d27d377c3496
---

aello (`C:\Users\H\Desktop\work\aello`, repo github.com/ryha0008-boop/aello, public, branch `main`) is a ground-up rebuild of an older tool `helo` (now frozen at `Desktop/helo-win` — don't copy its patterns blindly; providers/API-keys/pi/opencode/auto-hooks were deliberately dropped). Claude-only, subscription auth, cross-platform Linux + Windows x86_64 (macOS source-only). Also runs on a Linux VPS (`devuser@vps-main`, installed `~/.local/bin/aello`, sudo-free updates). A **blueprint** = reusable agent identity (name, model, optional persona, capabilities) in config.toml; placing it in a project creates `<project>/.claude-env-<name>/` as Claude's CLAUDE_CONFIG_DIR. Working/verified at ~v0.1.24: add/list/remove/run/login/update + TUI, built-in personas (coder/sysadmin), per-blueprint capabilities, generated /sync, git attribution, unified contextdb, self-update, README + docs/ + repo CLAUDE.md. See the repo's CLAUDE.md for architecture + src module map. [[aello-architecture-decisions]] [[aello-ci-release]]
