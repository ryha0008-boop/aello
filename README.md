# aello

[![release](https://github.com/ryha0008-boop/aello/actions/workflows/release.yml/badge.svg)](https://github.com/ryha0008-boop/aello/actions/workflows/release.yml)
[![license: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![good first issues](https://img.shields.io/github/issues/ryha0008-boop/aello/good%20first%20issue.svg?color=7057ff&label=good%20first%20issues)](https://github.com/ryha0008-boop/aello/labels/good%20first%20issue)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

Isolated Claude Code environments — like Python venvs, but for AI agents.

`aello` lets you define reusable agent **blueprints** (a name, a model, a persona, and a set of capabilities) and drop them into any project as an isolated Claude Code environment. Each blueprint runs Claude with its own `CLAUDE_CONFIG_DIR`, so multiple agents can work in the same repo without stepping on each other's config — and `git blame` can tell you which one made each change.

- **Isolated** — every blueprint gets its own `.claude-env-<name>/` (settings, persona, hooks, skills), kept out of your repo automatically.
- **Shared login** — one `aello login` token is shared safely across any number of concurrent envs (no credential rotation races).
- **Capability-driven** — pick what a blueprint maintains (`/sync` docs, GitHub, CHANGELOG, docs/, README); aello scaffolds the files and generates a `/sync` skill tailored to exactly that.
- **Spoken** — every env reads each response's `TL;DR:` line aloud, with a different voice per concurrent session and one `aello voice mute` to stop them all.
- **Attributable** — commits made through a blueprint are authored as `<blueprint> <blueprint@aello.local>`, so multi-agent work is traceable.

Cross-platform: Linux (x86_64), macOS (Apple Silicon + Intel), Windows (x86_64).

## Install

### Linux / macOS — one-liner

```sh
curl -fsSL https://raw.githubusercontent.com/ryha0008-boop/aello/main/install.sh | sh
```

Downloads the latest release into `~/.local/bin` (override with `AELLO_BIN_DIR`), makes it executable, clears the macOS quarantine flag, and prints a PATH hint if that dir isn't on your `$PATH`. Platforms without a prebuilt binary (e.g. arm64 Linux) exit with a build-from-source pointer.

Every release also publishes an immutable `vX.Y.Z` tag if you'd rather pin a version — swap `latest` for `v0.1.54` in any download URL below. `aello update` always moves you to the newest release.

### Linux (x86_64) — manual

```sh
mkdir -p ~/.local/bin
curl -L https://github.com/ryha0008-boop/aello/releases/download/latest/aello-x86_64-linux -o ~/.local/bin/aello
chmod +x ~/.local/bin/aello
# ensure ~/.local/bin is on PATH
aello --version
```

Install into a **user-writable** dir (`~/.local/bin`), not root-owned `/usr/local/bin` — `aello update` rewrites the binary in place and needs write access to that directory.

### Windows (x86_64)

Download [`aello-x86_64-windows.exe`](https://github.com/ryha0008-boop/aello/releases/download/latest/aello-x86_64-windows.exe) from the latest release, rename it to `aello.exe`, and put it somewhere on your `PATH` (e.g. `C:\Users\<you>\bin\`).

The `.exe` is **unsigned** (a code-signing certificate is a paid yearly subscription), so SmartScreen may show *"Windows protected your PC"* on first run — **More info → Run anyway**. Verify the download against the release's `SHA256SUMS` if you'd rather check it yourself.

### macOS — manual

```sh
mkdir -p ~/.local/bin
# Apple Silicon; use aello-x86_64-macos on Intel Macs
curl -L https://github.com/ryha0008-boop/aello/releases/download/latest/aello-aarch64-macos -o ~/.local/bin/aello
chmod +x ~/.local/bin/aello
xattr -d com.apple.quarantine ~/.local/bin/aello   # see below
aello --version
```

**Gatekeeper:** the macOS binaries are **unsigned** (code signing needs a paid Apple Developer account), so a browser- or `curl`-downloaded copy is quarantined and macOS refuses to run it — *"aello cannot be opened because the developer cannot be verified."* The `xattr -d com.apple.quarantine` line above clears it; the one-liner installer does it for you. Prefer not to trust an unsigned binary? Build from source — the checksums for every published binary are in the release's `SHA256SUMS`.

### From source (any platform)

Needs a [Rust toolchain](https://rustup.rs); everything else `cargo` pulls in.

```sh
cargo install --git https://github.com/ryha0008-boop/aello   # installs to ~/.cargo/bin/aello

# or, to hack on it:
git clone https://github.com/ryha0008-boop/aello
cd aello
cargo install --path .
```

`~/.cargo/bin` is user-writable, so `aello update`'s in-place binary replacement works from a source install too.

## Prerequisites

- **Claude Code** on your `PATH` (`claude`). aello sets `CLAUDE_CONFIG_DIR` and launches it.
- **Python** (`python3` on Linux/macOS, `python` on Windows) for the PostCompact and SessionEnd transcript hooks.
- **git** / **gh** only if you use the `github` capability.

## Quick start

```sh
aello login                                   # one-time: store a shared Claude token
aello add coder --model opus --claude-md coder --github --changelog --docs --readme
cd ~/my-project
aello run coder                               # places an isolated env + launches Claude
```

Inside that project, `aello run coder` creates `.claude-env-coder/`, scaffolds `CHANGELOG.md` / `README.md` / `docs/` / a project `CLAUDE.md` (only the ones you enabled, only if missing), adds `.claude-env-*` to `.gitignore`, seeds a `/sync` skill tailored to the enabled capabilities, and (on first placement) seeds a starter working-style memory so the env boots with it in `/context`. Type `/sync` inside Claude to reconcile those docs and commit + push.

Run `aello` with no arguments for the full-screen TUI (browse, add via a guided checklist, resume sessions, manage the token, self-update).

## Concepts

- **Blueprint** — a reusable agent identity stored in aello's config: `name`, `model`, an optional global persona, and its capabilities. Reusable across many projects.
- **Env dir** — `<project>/.claude-env-<name>/`. This is the blueprint's `CLAUDE_CONFIG_DIR`: settings, the global persona `CLAUDE.md`, the PostCompact + SessionEnd hooks, and the generated `/sync` skill live here. Gitignored by convention.
- **Global persona vs project CLAUDE.md** — the *global* `CLAUDE.md` (in the env dir) is the agent's persona, set once. The *project* `CLAUDE.md` (in the repo root, enabled by `--project-md`) holds project-specific facts. Memory is separate: a starter working-style memory is seeded on first placement (never clobbered after), then maintained automatically.
- **Capabilities** — what a blueprint maintains. Each one scaffolds its file and adds a section to the generated `/sync` skill. See the table below.
- **`/sync`** — a manually-invoked skill (no auto-commit hooks). Generated per blueprint, so it only covers what that blueprint has — a no-GitHub blueprint gets no git talk at all.
- **`/handoff`** — a manually-invoked skill seeded for *every* blueprint (regardless of capabilities). At session end it writes a self-contained `<blueprint>.HANDOFF.md` resume note at the repo root so the next session continues seamlessly after a full `/clear`. The filename is prefixed with the blueprint name so multiple blueprints in one repo don't clobber each other's handoff. Transient: read on boot, then deleted.
- **`/note`** — a manually-invoked skill seeded for *every* blueprint. Leaves a note for **another** environment in the same repo: `/note <env-name>` writes what you were doing, the problem, and what that env needs to fix to `<env-name>.NOTE.md` at the repo root (its inbox), which the target reads and then deletes. Unlike `/handoff` (a note to yourself), this is a message across environments — the common case when two blueprints split one project and one hits something on the other's side.
- **`/twosentences`** — a manually-invoked skill seeded for *every* blueprint. Condenses your previous response into exactly two sentences.
- **Shared auth** — `aello login` runs `claude setup-token` and stores a long-lived `CLAUDE_CODE_OAUTH_TOKEN`. It doesn't rotate, so any number of concurrent envs share it safely.
- **contextdb** — transcripts are written to a unified tree, `<contextdb>/<project>/<blueprint>/<ts>_<session>.jsonl`. PostCompact saves compaction summaries; SessionEnd captures sessions ended with `/clear` or a plain exit (which never compact), archiving the `/handoff` note. Configurable (TUI → `C`).

See [`docs/concepts.md`](docs/concepts.md) and [`docs/capabilities.md`](docs/capabilities.md) for detail.

## Commands

```
aello                                          # interactive TUI (no args)
aello --version
aello init                                     # first-run: login + first blueprint
aello add <name> --model <m> [--claude-md <coder|sysadmin|path>]
        [--project-md] [--github] [--changelog] [--docs] [--readme]
aello list [--json]
aello remove <name> [--yes] [--purge]         # --purge also deletes the placed env dir + mirror
aello edit <name> [--rename <new>] [--model <m>] [--claude-md <coder|sysadmin|path>]
        [--project-md|--no-project-md] [--github|--no-github]
        [--changelog|--no-changelog] [--docs|--no-docs] [--readme|--no-readme]
aello run [name] [--resume [id]] [-p <prompt>] [-- <extra args for claude>]
aello login                                    # store the shared Claude token
aello github-setup [--name <repo>] [--public] [--yes]   # create + push the repo via gh
aello docs [name]                              # print bundled reference docs (no name lists them)
aello voice <mute|unmute|stop|status> [--project]       # off switch for the voice
aello completions <bash|zsh|fish|powershell|elvish>     # print a shell completion script
aello update                                   # self-update to the latest release
```

- `completions` prints a script to stdout for tab-completing blueprint names and flags. Load it, e.g.:
  ```sh
  aello completions bash | sudo tee /etc/bash_completion.d/aello   # bash (system-wide)
  aello completions zsh  > ~/.zfunc/_aello                         # zsh (ensure ~/.zfunc is on $fpath)
  aello completions fish > ~/.config/fish/completions/aello.fish   # fish
  aello completions powershell >> $PROFILE                         # PowerShell
  ```

- `edit` changes a blueprint in place, including `--rename <new>` (validated, rejected if the name is taken) — which also moves the placed `.claude-env-<name>/` env dir and its `claude-internal/<name>/` mirror in the current project. Capability flags are tri-state: `--github` enables, `--no-github` disables, omitting both leaves it as-is. Changes apply on the next `run`; the global persona in an already-placed env is never re-clobbered.
- `run` with no name uses the sole blueprint (errors if there are several).
- `--resume` with no value continues the most recent session; `--resume <id>` resumes a specific one. The TUI (`S`) browses sessions to resume.
- `-p "<prompt>"` runs headless and exits. Anything after `--` is passed straight to `claude`.

### TUI keys

`↑/↓` move · `↵` run · `F` filter · `S` sessions · `A` add (guided) · `E` edit (guided) · `D` delete · `C` contextdb folder · `M` mute voice · `L` login · `U` update · `?` docs · `Q` quit.

`M` toggles the same machine-wide mute as `aello voice mute` — it silences every env, not just the selected blueprint, and cuts off whatever is speaking. While muted the footer reads `VOICE: MUTED`, so a silent machine doesn't look like a broken hook.

By default the registry shows only blueprints already placed in the current directory (their `.claude-env-<name>/` exists), which keeps a per-project blueprint workflow tidy. `F` toggles between that local subset and all blueprints; when nothing is placed here yet, all are shown.

`E` edits the selected blueprint through the same guided steps as add, pre-filled with its current model, persona, and capabilities (the name isn't editable). Changes apply on the next `run`.

`?` opens a full-screen docs reader over the repo's `docs/` (`↑/↓` scroll, `Tab`/`←→` switch doc, `Esc` close). The same content is available from the CLI via `aello docs`.

## Capabilities

| Flag | TUI label | Scaffolds (if missing) | Adds to `/sync` |
|---|---|---|---|
| `--claude-md <name\|path>` | persona picker | global `CLAUDE.md` in the env (persona) | — |
| `--project-md` | project-md | project-root `CLAUDE.md` | reconcile project CLAUDE.md |
| `--github` | github | `.gitignore` entry `.claude-env-*` | repo health + commit & push + `Env:` trailer |
| `--changelog` | changelog | `CHANGELOG.md` | keep CHANGELOG current |
| `--docs` | docs | `docs/` | reconcile docs/ |
| `--readme` | readme | `README.md` | keep README current |

## Voice — every env speaks

The voice is **not** a capability and there is nothing to turn on. Every env gets a `Stop` hook that reads each response's trailing `TL;DR:` line aloud through a free Edge neural voice, and a `SessionEnd` hook that returns the voice it borrowed. The persona picks up a section instructing it to end every response with that line — without one there is nothing to speak.

Silence is a runtime setting, not a property of a blueprint: `aello voice mute` (or `M` in the TUI) covers every env at once, `mute --project` covers one project. An env is never made quiet by placing it differently.

The hook is copied **into the env** and registered as `$CLAUDE_CONFIG_DIR/hooks/speak.py`, so it never points at a checkout somewhere else on disk: moving or renaming any other directory can't silence it, and a newly placed env speaks with no hand-editing.

Its state — the voice pool, per-session leases, mute flags — lives in one machine-wide folder (`%LOCALAPPDATA%\revoiced`, `~/Library/Application Support/revoiced`, `$XDG_DATA_HOME/revoiced`), shared by every env. So concurrent envs each lease a different voice, playback is serialised machine-wide instead of overlapping, and a single mute covers all of them:

```
aello voice mute              # silence every env, and stop the current sentence
aello voice mute --project    # silence just this project
aello voice unmute            # (--project too)
aello voice stop              # cut off what's speaking now, without muting
aello voice status            # mute state + pool size
```

These work from any directory and need no Python — useful precisely when a machine you didn't expect to talk starts talking.

**Prerequisites.** Python 3 on `PATH`. Without `edge-tts` (`pip install edge-tts`) it falls back to the OS voice — SAPI on Windows, `say` on macOS, `spd-say`/`espeak` on Linux. Linux playback also needs one of `mpv`, `ffplay`, `mpg123`, or `cvlc`; macOS (`afplay`) and Windows (.NET) are covered by the OS. Ducking other applications' audio while it speaks is Windows-only and needs `pycaw`; elsewhere it's a no-op.

With `--github`, commits made through the blueprint are authored as `<name> <name@aello.local>` (both author and committer), and `/sync` appends an `Env: <name>` trailer to each commit — so `git log --author` and `git blame` reveal which blueprint did what.

## Configuration

Blueprints, the shared token, and the contextdb path live in `config.toml` under your OS config dir (via the `directories` crate). The token is plaintext on your personal machine — regenerate it yearly (`aello login`).

## Self-update

```sh
aello update
```

Pulls the matching binary from the rolling `latest` GitHub release and replaces the running executable in place (atomic rename on both platforms). If GitHub is unreachable it prints the releases URL.

## Contributing

Issues and PRs welcome — aello is a small, focused Rust CLI (no extra toolchain), and contributions of all sizes help.

**New here? Start with a [`good first issue`](https://github.com/ryha0008-boop/aello/labels/good%20first%20issue).** Each is scoped, names the file to touch, and lists acceptance criteria — comment to claim it. Most logic (templates, placement, capability scaffolding) is unit-testable without ever launching Claude.

```sh
git clone https://github.com/ryha0008-boop/aello && cd aello
cargo build --release && cargo test     # both green before you start
```

The `site/` directory holds the landing page — a static Next.js app that's independent of the
CLI. You only need Node if you're changing the page itself; `cargo build` and `cargo test`
ignore it entirely.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full dev loop and conventions, and [CLAUDE.md](CLAUDE.md) for the architecture deep-dive (every `src/` module is mapped there).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
