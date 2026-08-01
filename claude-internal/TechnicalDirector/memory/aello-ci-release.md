---
name: aello-ci-release
description: "aello release model: push main → CI auto patch-bumps Cargo.toml + builds 4 targets → publishes BOTH an immutable vX.Y.Z tag and the rolling `latest`; never draft a release; SHA256SUMS is part of the contract; CI never updates Cargo.lock"
metadata: 
  node_type: memory
  type: project
  originSessionId: afdce7ab-1496-4a48-a656-d27d377c3496
  modified: 2026-08-01T14:36:39.596Z
---

Push to `main` → GitHub Actions: (1) **bump** job +0.0.1 in Cargo.toml, commits `release: vX.Y.Z [skip ci]`, pushes via GITHUB_TOKEN (does NOT re-trigger CI). (2) **build** jobs, ref'd at the *bump commit sha* so the binary reports the new version, producing **four** targets: `aello-x86_64-linux`, `aello-x86_64-windows.exe`, `aello-aarch64-macos`, `aello-x86_64-macos` (the Intel Mac cross-compiles on the Apple Silicon runner). All unsigned. (3) **publish** generates `SHA256SUMS` and uploads binaries + manifest to **two** releases: the immutable **`vX.Y.Z`** tag (flagged `--latest`) and the permanent rolling **`latest`** tag (`--clobber`, `--latest=false`). `aello update` hits `/releases/latest`; `install.sh` and older binaries hard-code the `latest` URL — which is why both are published. `update` verifies the download against SHA256SUMS (verify-if-present), so don't drop it from release.yml. **NEVER delete+recreate a release** — it intermittently lands in DRAFT and 404s `aello update`. Release notes must **end with the build sha**; `aello update` reads the commit from the last whitespace token of the body.

**CI bumps `Cargo.toml` but never `Cargo.lock`.** So the lock permanently lags one version behind, and *every* local `cargo build`/`cargo test` rewrites its `aello` version line and dirties the tree. Confirmed twice on 2026-08-01 (0.1.52→53→54). Committing the lock is churn — the next release re-stales it within the minute — so the habit is `git restore Cargo.lock` before staging. The durable fix nobody has done yet is a `cargo update -p aello` (or `cargo check`) step in the bump job so the lock moves with the manifest. Don't mistake the dirty lock for something you broke.

After CI, `git pull --rebase` to sync the bumped Cargo.toml — being "1 behind" right after your push is normal. Fallback if a push doesn't trigger CI: `gh workflow run release.yml --ref main`. Minor/major versions are bumped by hand. CHANGELOG version headings are hand-written and lag Cargo.toml — don't agonize.

Self-update gotchas: Linux can't `write()` over a running exe (ETXTBSY) → temp-file + atomic rename, and install to user-writable `~/.local/bin`, not `/usr/local/bin`; Windows renames the running exe to a unique `aello.exe.old-<nanos>` and sweeps on startup. See [[aello-overview]] and [[aello-dev-gotchas]] #5 (a *branch* build can never be delivered by `aello update`).
