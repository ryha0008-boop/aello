---
name: aello-overview
description: "aello = isolated Claude Code envs (venvs for agents); Claude-only, subscription-auth, cross-platform; ground-up rebuild of helo"
metadata: 
  node_type: memory
  type: project
  originSessionId: afdce7ab-1496-4a48-a656-d27d377c3496
---

aello (`C:\Users\H\Desktop\work\aello`, repo github.com/ryha0008-boop/aello, public, branch `main`) is a ground-up rebuild of an older tool `helo` (now frozen at `Desktop/helo-win` — don't copy its patterns blindly; providers/API-keys/pi/opencode/auto-hooks were deliberately dropped). Claude-only, subscription auth, cross-platform Linux + Windows x86_64 (macOS source-only). Also runs on a Linux VPS (`devuser@vps-main`, installed `~/.local/bin/aello`, sudo-free updates). A **blueprint** = reusable agent identity (name, model, optional persona, **role**) in config.toml; placing it in a project creates `<project>/.claude-env-<name>/` as Claude's CLAUDE_CONFIG_DIR. Working/verified at **v0.2.0, the first stable line (2026-08-03)**: add/list/remove/edit/run/init/login/github-setup/docs/voice/update + TUI, built-in personas (coder/sysadmin), **three roles** (maintainer/contributor/standalone — the five capability flags are gone, see [[aello-architecture-decisions]]), generated /sync plus the universal /handoff, /note and /twosentences skills, git attribution, a unified contextdb that now **copies** transcripts and captures reasoning summaries ([[aello-contextdb]]), a SessionStart standing block telling every session it is running under aello, self-update, README + **eight** bundled docs, and a static site in `site/` that now also **hosts those docs**, deployed to GitHub Pages at <https://ryha0008-boop.github.io/aello/>. The voice is **not** a capability — every env speaks, and silence is `aello voice mute` at runtime. See the repo's CLAUDE.md for architecture + src module map. [[aello-architecture-decisions]] [[aello-ci-release]]
