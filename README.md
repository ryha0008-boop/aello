# aello

[![release](https://github.com/ryha0008-boop/aello/actions/workflows/release.yml/badge.svg)](https://github.com/ryha0008-boop/aello/actions/workflows/release.yml)
[![license: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![good first issues](https://img.shields.io/github/issues/ryha0008-boop/aello/good%20first%20issue.svg?color=7057ff&label=good%20first%20issues)](https://github.com/ryha0008-boop/aello/labels/good%20first%20issue)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

Isolated Claude Code environments — like Python venvs, but for AI agents.

`aello` lets you define reusable agent **blueprints** (a name, a model, a persona, and a role) and drop them into any project as an isolated Claude Code environment. Each blueprint runs Claude with its own `CLAUDE_CONFIG_DIR`, so multiple agents can work in the same repo without stepping on each other's config — and `git blame` can tell you which one made each change.

- **Isolated** — every blueprint gets its own `.claude-env-<name>/` (settings, persona, hooks, skills), kept out of your repo automatically.
- **Shared login** — one `aello login` token is shared safely across any number of concurrent envs (no credential rotation races).
- **Role-driven** — one choice per blueprint: `maintainer` owns the repo's docs and git, `contributor` commits its own work and logs it, `standalone` works alone. aello scaffolds the matching files and generates a `/sync` skill tailored to exactly that.
- **Spoken** — every env reads each response's `TL;DR:` line aloud, with a different voice per concurrent session and one `aello voice mute` to stop them all.
- **Attributable** — commits made through a blueprint are authored as `<blueprint> <blueprint@aello.local>`, so multi-agent work is traceable.

Cross-platform: Linux (x86_64), macOS (Apple Silicon + Intel), Windows (x86_64).

## Install

### Linux / macOS — one-liner

```sh
curl -fsSL https://raw.githubusercontent.com/ryha0008-boop/aello/main/install.sh | sh
```

Downloads the latest release into `~/.local/bin` (override with `AELLO_BIN_DIR`), makes it executable, clears the macOS quarantine flag, and prints a PATH hint if that dir isn't on your `$PATH`. Platforms without a prebuilt binary (e.g. arm64 Linux) exit with a build-from-source pointer.

Every release also publishes an immutable `vX.Y.Z` tag if you'd rather pin a version — swap `latest` for the tag you want in any download URL below. `aello update` always moves you to the newest release.

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
- **Python 3** — used for the voice and for archiving session transcripts.
- **git** / **gh** only for the `maintainer` and `contributor` roles.

## Quick start

```sh
aello login                                   # one-time: store a shared Claude token
aello add coder --model opus --claude-md coder --role maintainer
cd ~/my-project
aello run coder                               # places an isolated env + launches Claude
```

Inside that project, `aello run coder` creates `.claude-env-coder/`, scaffolds `CHANGELOG.md` / `README.md` / `docs/` / a project `CLAUDE.md` (only the ones its role owns, only if missing), adds `.claude-env-*` to `.gitignore`, seeds a `/sync` skill tailored to its role, and (on first placement) seeds a starter working-style memory so the env boots with it in `/context`. Type `/sync` inside Claude to reconcile those docs and commit + push.

Run `aello` with no arguments for the full-screen TUI (browse, add via a guided checklist, resume sessions, manage the token, self-update).

## Concepts

- **Blueprint** — a reusable agent: a name, a model, a persona, and what it looks after. Define it once, drop it into any number of projects.
- **Env dir** — `<project>/.claude-env-<name>/`. Everything that makes that agent itself, kept in your project but out of your commits.
- **Role** — what an agent is responsible for, and therefore which steps its `/sync` has. See the table below.
- **Two `CLAUDE.md` files** — the one in the env dir is the agent's *persona* (who it is), set once. The one in your repo root holds *project* facts and is kept current by `/sync`. Agents also build up memory as they work.
- **Shared login** — one `aello login` covers every agent, however many run at once.

Four commands come with every agent. You type them; agents never run them by themselves:

- **`/sync`** — the checkpoint. Brings the docs it maintains back in line with the code, then commits and pushes. Tailored per blueprint, so an agent without GitHub gets no git talk at all.
- **`/handoff`** — writes a resume note before you stop, so the next session picks up where you left off. Read on boot, then deleted.
- **`/note <agent>`** — leaves a message for a *different* agent, for when two split a project and one hits something on the other's side.
- **`/twosentences`** — condenses the last response to two sentences.

Rewritten one of those for a project and want to keep it? Put an empty `.aello-keep` file beside it and aello stops regenerating it.

Transcripts of every session are archived outside the repo so nothing is lost when a session ends.

See [`docs/workflows.md`](docs/workflows.md) for task-shaped walkthroughs, and [`docs/concepts.md`](docs/concepts.md), [`docs/roles.md`](docs/roles.md), [`docs/skills.md`](docs/skills.md), [`docs/voice.md`](docs/voice.md) and [`docs/troubleshooting.md`](docs/troubleshooting.md) for how all of it actually works. The same pages ship inside the binary — `aello docs` lists them, `aello docs workflows` prints one, and `?` in the TUI opens a reader.

## Commands

```
aello                                          # interactive TUI (no args)
aello --version
aello init                                     # first-run: login + first blueprint
aello add <name> --model <m> [--claude-md <coder|sysadmin|path>]
        [--role maintainer|contributor|standalone]
aello list [--json]
aello remove <name> [--yes] [--purge]         # --purge also deletes the placed env dir + mirror
aello edit <name> [--rename <new>] [--model <m>] [--claude-md <coder|sysadmin|path>]
        [--role maintainer|contributor|standalone]
aello run [name] [--resume [id]] [-p <prompt>] [-- <extra args for claude>]
aello login                                    # store the shared Claude token
aello github-setup [--name <repo>] [--public] [--yes]   # create + push the repo via gh
aello docs [name]                              # print bundled reference docs (no name lists them)
aello voice <mute|unmute|stop|status> [--project]       # off switch for the voice
aello completions <bash|zsh|fish|powershell|elvish>     # print a shell completion script
aello update [--force]                         # self-update (--force reinstalls the current version)
```

- `completions` prints a script to stdout for tab-completing blueprint names and flags. Load it, e.g.:
  ```sh
  aello completions bash | sudo tee /etc/bash_completion.d/aello   # bash (system-wide)
  aello completions zsh  > ~/.zfunc/_aello                         # zsh (ensure ~/.zfunc is on $fpath)
  aello completions fish > ~/.config/fish/completions/aello.fish   # fish
  aello completions powershell >> $PROFILE                         # PowerShell
  ```

- `edit` changes a blueprint in place, including `--rename <new>` (validated, rejected if the name is taken) — which also moves the placed `.claude-env-<name>/` env dir and its `claude-internal/<name>/` mirror in the current project. `--role` swaps the role outright; omitting a flag leaves that field as-is. Changes apply on the next `run`; the global persona in an already-placed env is never re-clobbered.
- `run` with no name uses the sole blueprint (errors if there are several).
- `--resume` with no value continues the most recent session; `--resume <id>` resumes a specific one. The TUI (`S`) browses sessions to resume.
- `-p "<prompt>"` runs headless and exits. Anything after `--` is passed straight to `claude`.

### TUI keys

`↑/↓` move · `↵` run · `F` filter · `S` sessions · `A` add (guided) · `E` edit (guided) · `D` delete · `C` contextdb folder · `M` mute voice · `L` login · `U` update · `?` docs · `Q` quit. Command keys are case-insensitive, and `Ctrl+C` quits from anywhere. In the docs reader (`?`), `Home`/`End` jump to the start and end of a page.

`M` toggles the same machine-wide mute as `aello voice mute` — it silences every env, not just the selected blueprint, and cuts off whatever is speaking. While muted the footer reads `VOICE: MUTED`, so a silent machine doesn't look like a broken hook.

By default the registry shows only blueprints already placed in the current directory (their `.claude-env-<name>/` exists), which keeps a per-project blueprint workflow tidy. `F` toggles between that local subset and all blueprints; when nothing is placed here yet, all are shown.

`E` edits the selected blueprint through the same guided steps as add, pre-filled with its current model, persona, and role (the name isn't editable). Changes apply on the next `run`.

`?` opens a full-screen docs reader over the repo's `docs/` (`↑/↓` scroll, `Tab`/`←→` switch doc, `Esc` close). The same content is available from the CLI via `aello docs`.

## Roles

Pick one per blueprint. **One maintainer per repo, any number of contributors** — that's the shape this is for: the maintainer owns the prose, everyone else commits their own work without rewriting it.

| Role | Scaffolds (if missing) | `/sync` |
|---|---|---|
| `maintainer` | project `CLAUDE.md`, `CHANGELOG.md`, `docs/`, `README.md`, `.gitignore` entry, `.gitattributes`, `VERSION` + bump CI, `claude-internal/<name>/` | full — repo health, reconcile memory + all four docs, mirror the env, commit & push with an `Env:` trailer |
| `contributor` | `CHANGELOG.md`, `.gitignore` entry, `.gitattributes`, `VERSION` + bump CI, `claude-internal/<name>/` | git only — repo health, `CHANGELOG.md`, mirror the env, commit & push with an `Env:` trailer |
| `standalone` | nothing | none — no `/sync` skill is seeded |

The persona is separate from the role: `--claude-md <name\|path>` writes the env's global `CLAUDE.md` once, and no role rewrites it.

Upgrading from a pre-0.2 config? The five capability flags are gone and existing blueprints migrate themselves — see [`docs/roles.md`](docs/roles.md).

## Voice — every env speaks

Every env reads the last line of each response aloud, so you can leave an agent working and hear when it lands. There is nothing to switch on.

```sh
aello voice mute              # silence every env, and stop the sentence playing now
aello voice mute --project    # silence just this project
aello voice unmute            # (--project too)
aello voice stop              # cut off what's speaking, without muting
aello voice status            # muted or not, and which version the hook is
```

These work from any directory and need no setup — which is exactly where you are when a machine you didn't expect to talk starts talking. `M` in the TUI is the same switch.

Run several agents at once and each gets a **different voice**, taking turns rather than talking over each other. Each spoken line also raises a desktop notification, for when you're in another window.

**You'll need** Python 3, and `pip install edge-tts` for the good voices — without it you get your OS's built-in voice. On Linux you also need one of `mpv`, `ffplay`, `mpg123` or `cvlc` to play audio.

Not hearing anything? Run `aello voice status` first. [`docs/voice.md`](docs/voice.md) covers the rest.

## Git attribution

For a maintainer or contributor, commits made through a blueprint are authored as `<name> <name@aello.local>`, and `/sync` adds an `Env: <name>` line to each commit — so `git log --author` and `git blame` show which agent did what.

## Configuration

Your blueprints, login token, and transcript folder live in a `config.toml` in your OS's usual config location. The token is stored in plain text on your own machine — regenerate it once a year with `aello login`.

## Self-update

```sh
aello update
```

Pulls the matching binary from the rolling `latest` GitHub release and replaces the running executable in place (atomic rename on both platforms). If GitHub is unreachable it prints the releases URL.

## Contributing

Issues and PRs welcome — aello is a small, focused Rust CLI (no extra toolchain), and contributions of all sizes help.

**New here? Start with a [`good first issue`](https://github.com/ryha0008-boop/aello/labels/good%20first%20issue).** Each is scoped, names the file to touch, and lists acceptance criteria — comment to claim it. Most logic (templates, placement, role scaffolding) is unit-testable without ever launching Claude.

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
