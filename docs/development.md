# Developing aello

Workflows for working on aello itself. Contributing guidelines and the on-ramp for a first patch are in [`CONTRIBUTING.md`](../CONTRIBUTING.md); this page is the mechanics.

## Build and test

```sh
cargo build --release           # writes to target/ — safe while aello is running
cargo test                      # unit tests; no Claude launch needed
cargo clippy --all-targets
cargo install --path . --force  # replace ~/.cargo/bin/aello with the local build
```

Nearly everything is unit-testable without launching Claude: template rendering, placement, role expansion, launch environment, config migration. If a change isn't testable that way, that's usually a sign the logic wants pulling out of the I/O.

**`cargo install --path .` fails with "Access is denied" on Windows** while any `aello.exe` is running — including the one in the terminal you're typing in. Rename it out of the way first; the next launch sweeps up the leftovers:

```sh
mv ~/.cargo/bin/aello.exe ~/.cargo/bin/aello.exe.old-1
cargo install --path . --force
```

**`Cargo.lock` goes dirty on its own.** `git restore Cargo.lock` before staging and before any rebase.

## The verification rule that matters here

aello writes files into directories it doesn't own, and the copy that runs is never the copy in the repo. Source sitting next to all its siblings behaves differently from a copy deployed into a bare directory — **when they disagree, the deployed copy is the truth.**

The characteristic bug in this codebase is not a crash. It's a guard that swallows a missing dependency, a registration nothing rejects, an overwrite that looks like a no-op. The useful question is: *what would this look like if it were already broken?* If the answer is "exactly like working", go and measure it — against a placed env, not the checkout.

Both of the worst bugs found here were found that way, by running real data through the deployed copy. Neither was visible in the diff.

## Adding a doc

Drop a `.md` file in `docs/`. That's the whole procedure.

`docs.rs` embeds the directory at compile time (`include_dir!`), so a new file appears in `aello docs`, the TUI reader (`?`) and the docs site automatically — no per-file code. Two optional touches:

- The **title** comes from the first `# H1`, falling back to a prettified slug.
- **Reading order** is the `ORDER` list in `docs.rs`; anything unlisted sorts after it alphabetically.

`docs/` is embedded at compile time, so **a doc edit needs a reinstall before `aello docs` shows it**. New files need no code change, but they do need a rebuild.

Who each page is for: **`docs/` is for developers, `README.md` is for users.** If a section only makes sense to someone who has read the source, it belongs here.

## Re-vendoring the voice hook

The voice hook is vendored from the [`revoiced`](voice.md) project as **five files** copied into `src/`. Re-syncing is four steps, and skipping any of them ships a half-working env:

1. Copy all five files from the upstream checkout into `src/` (`hooks_speak.py`, `hooks_duck.py`, `hooks_focus.py`, `hooks_notify.py`, `hooks_win_audio.ps1`).
2. Bump `project.rs::HOOK_VERSION` to upstream's `HOOK_VERSION`.
3. Update the five-file digest — **the failing test prints the new value.**
4. `cargo install --path . --force`, then re-run every already-placed env so the new files propagate.

Two tests guard this, and they guard different things. The first compares the recorded constant against the vendored `speak.py`. The second digests **all five** files (CRLF-normalised, so a Windows checkout and Linux CI agree) — because `HOOK_VERSION` lives in `speak.py` alone, a re-vendor touching only `duck.py` or `win_audio.ps1` would otherwise slip past. That second test has already caught two bumps.

Ask a *placed copy* what it has:

```sh
python <env>/hooks/speak.py --hook-version
```

That prints before any optional import, so unlike `--status` it answers even from a partial copy.

**Never record an upstream commit sha as the drift marker.** revoiced's CI commits a version bump on every push to main, so local shas get rewritten by the rebase and a recorded sha rots on its own. `HOOK_VERSION` is content, not history.

## Shipping includes propagation

Committing is the start, not the end. A change only counts once every already-installed copy has it — and a stale copy quietly overwrites fresh work, because `place()` rewrites the bundled files on every run. So:

- **Install immediately after hand-backfilling env dirs**, or a stale binary will undo the backfill one env at a time.
- Afterwards, verify by asking each copy what version it has, rather than assuming the rollout worked.

## Release process

Push to `main` and GitHub Actions does the rest:

1. **bump** — resolves the version. If the version in `Cargo.toml` already has a `vX.Y.Z` tag, it increments the patch, commits `release: vX.Y.Z [skip ci]` and pushes via `GITHUB_TOKEN` (which does not re-trigger CI). If it has no tag yet, a human set it deliberately and it is published unchanged — that is how a minor or major bump ships, since a patch bump can only ever produce X.Y.(Z+1).
2. **build** — four targets, at the bump commit's sha so the binary reports the new version: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, and `x86_64-apple-darwin` (cross-compiled on the Apple Silicon runner). All unsigned — `install.sh` clears the macOS quarantine attribute and the README documents Windows SmartScreen.
3. **publish** — generates `SHA256SUMS` and uploads binaries + manifest to **two** releases: the immutable `vX.Y.Z` tag (flagged `--latest`) and the permanent rolling `latest` tag (`--clobber`, explicitly `--latest=false`).

Both releases exist because two consumers disagree: `aello update` hits `/releases/latest` and resolves to the versioned release, while `install.sh` and older binaries hard-code the rolling `latest` tag's download URL. `aello update` verifies the download against `SHA256SUMS` before installing (verify-if-present, so releases predating the manifest still update) — keep publishing it.

Rules that are easy to violate and expensive to debug:

- **Release notes must end with the build sha** — `aello update` reads the commit from the last whitespace token of the body.
- **Never delete and recreate a release.** The intermediate draft state breaks `aello update` with a 404.
- The asset↔platform map is duplicated in `update.rs` and `install.sh`; **change both, plus the workflow.**
- After CI, `git pull --rebase` to pick up the bumped `Cargo.toml` before the next local `cargo install`.
- Minor and major versions are bumped by hand in `Cargo.toml`.
- If a plain push doesn't trigger the workflow, `gh workflow run release.yml --ref main` is the fallback.

## Commits

- **Small and scoped** — one change per commit; say what it does, not what you touched.
- **Docs in the same commit as the code**, never a sweep afterwards. Every user-facing change gets a `CHANGELOG.md` entry.
- **Explain what the diff can't tell you** — why this way, what the alternative cost, what will look wrong later but isn't.

`git commit` commits the whole index, not just what you last `git add`ed — check `git diff --cached --name-only` before committing if you've been staging in pieces.

On PowerShell 5.1, here-strings containing double quotes get mangled when passed to a native exe; use `git commit -F <file>`. `git` also writes to stderr on success, which surfaces as `NativeCommandError` — check the result line, not the exception.

## The site

`site/` is a static Next.js app, entirely separate from the crate: `cargo build` and `cargo test` never touch it and it ships nothing into the binary.

```sh
cd site
npm install
npm run dev      # http://localhost:3000
npm run build    # static HTML in out/
```

Its docs pages are generated at build time from this very directory, so a doc you add here appears on the site with no site-side change. Design tokens live in the `:root` block of `site/app/globals.css` and are transcribed from a captured design system — change values there, never inline in a component.
