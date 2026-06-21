---
name: aello-ci-release
description: "aello release model: push main → CI auto patch-bumps Cargo.toml + builds linux/windows → rolling `latest`; never draft the release"
metadata: 
  node_type: memory
  type: project
  originSessionId: afdce7ab-1496-4a48-a656-d27d377c3496
---

Push to `main` → GitHub Actions: (1) bump job +0.0.1 in Cargo.toml, commits `release: vX [skip ci]`, pushes via GITHUB_TOKEN (does NOT re-trigger CI). (2) build jobs (ref main) → `aello-x86_64-linux` (x86_64-unknown-linux-gnu) + `aello-x86_64-windows.exe` (x86_64-pc-windows-msvc). (3) publish CLOBBERS both onto one permanent rolling `latest` release. NEVER delete+recreate `latest` — it intermittently goes to DRAFT and 404s `aello update`. No version tags (commits≠tags; user found them confusing). After CI, `git pull --rebase` locally to sync the bumped Cargo.toml — your push is usually "1 behind" the CI bump, that's normal. Fallback if a push doesn't trigger CI: `gh workflow run release.yml --ref main`. Self-update gotchas: Linux can't write() over a running exe (ETXTBSY) → temp-file + atomic rename, install to user-writable ~/.local/bin not /usr/local/bin; Windows renames running exe to unique `aello.exe.old-<nanos>` + sweeps on startup. CHANGELOG version numbers are hand-written and lag Cargo.toml — don't agonize. [[aello-overview]]
