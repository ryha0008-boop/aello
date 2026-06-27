# aello

Isolated Claude Code environments — like Python venvs, but for AI agents.

`aello` lets you define reusable agent **blueprints** (a name, a model, a persona, and a set of capabilities) and drop them into any project as an isolated Claude Code environment. Each blueprint runs Claude with its own `CLAUDE_CONFIG_DIR`, so multiple agents can work in the same repo without stepping on each other's config — and `git blame` can tell you which one made each change.

- **Isolated** — every blueprint gets its own `.claude-env-<name>/` (settings, persona, hooks, skills), kept out of your repo automatically.
- **Shared login** — one `aello login` token is shared safely across any number of concurrent envs (no credential rotation races).
- **Capability-driven** — pick what a blueprint maintains (`/sync` docs, GitHub, CHANGELOG, docs/, README); aello scaffolds the files and generates a `/sync` skill tailored to exactly that.
- **Attributable** — commits made through a blueprint are authored as `<blueprint> <blueprint@aello.local>`, so multi-agent work is traceable.

Cross-platform: Linux and Windows (x86_64). macOS: build from source.

## Install

### Linux (x86_64)

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

### From source (any platform, incl. macOS)

```sh
git clone https://github.com/ryha0008-boop/aello
cd aello
cargo install --path .   # installs to ~/.cargo/bin/aello
```

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
- **`/handoff`** — a manually-invoked skill seeded for *every* blueprint (regardless of capabilities). At session end it writes a self-contained `HANDOFF.md` resume note at the repo root so the next session continues seamlessly after a full `/clear`. Transient: read on boot, then deleted.
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
aello remove <name>
aello edit <name> [--model <m>] [--claude-md <coder|sysadmin|path>]
        [--project-md|--no-project-md] [--github|--no-github]
        [--changelog|--no-changelog] [--docs|--no-docs] [--readme|--no-readme]
aello run [name] [--resume [id]] [-p <prompt>] [-- <extra args for claude>]
aello login                                    # store the shared Claude token
aello github-setup [--name <repo>] [--public] [--yes]   # create + push the repo via gh
aello docs [name]                              # print bundled reference docs (no name lists them)
aello update                                   # self-update to the latest release
```

- `edit` changes a blueprint in place. Capability flags are tri-state: `--github` enables, `--no-github` disables, omitting both leaves it as-is. Changes apply on the next `run`; the global persona in an already-placed env is never re-clobbered.
- `run` with no name uses the sole blueprint (errors if there are several).
- `--resume` with no value continues the most recent session; `--resume <id>` resumes a specific one. The TUI (`S`) browses sessions to resume.
- `-p "<prompt>"` runs headless and exits. Anything after `--` is passed straight to `claude`.

### TUI keys

`↑/↓` move · `↵` run · `F` filter · `S` sessions · `A` add (guided) · `E` edit (guided) · `D` delete · `C` contextdb folder · `L` login · `U` update · `?` docs · `Q` quit.

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

With `--github`, commits made through the blueprint are authored as `<name> <name@aello.local>` (both author and committer), and `/sync` appends an `Env: <name>` trailer to each commit — so `git log --author` and `git blame` reveal which blueprint did what.

## Configuration

Blueprints, the shared token, and the contextdb path live in `config.toml` under your OS config dir (via the `directories` crate). The token is plaintext on your personal machine — regenerate it yearly (`aello login`).

## Self-update

```sh
aello update
```

Pulls the matching binary from the rolling `latest` GitHub release and replaces the running executable in place (atomic rename on both platforms). If GitHub is unreachable it prints the releases URL.

## Contributing

Issues and PRs welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the dev loop and conventions, and [CLAUDE.md](CLAUDE.md) for the architecture deep-dive.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
