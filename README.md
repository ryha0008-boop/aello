# aello

[![release](https://github.com/ryha0008-boop/aello/actions/workflows/release.yml/badge.svg)](https://github.com/ryha0008-boop/aello/actions/workflows/release.yml)
[![license: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![good first issues](https://img.shields.io/github/issues/ryha0008-boop/aello/good%20first%20issue.svg?color=7057ff&label=good%20first%20issues)](https://github.com/ryha0008-boop/aello/labels/good%20first%20issue)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![docs](https://img.shields.io/badge/docs-ryha0008--boop.github.io%2Faello-ff8c42.svg)](https://ryha0008-boop.github.io/aello/docs/)

Isolated agent environments — like Python venvs, but for AI agents.

`aello` lets you define reusable agent **blueprints** (a name, a model, a persona, and a role) and drop them into any project as an isolated environment. Each blueprint runs its own CLI with its own config directory, so multiple agents can work in the same repo without stepping on each other's config — and `git blame` can tell you which one made each change. **Claude Code** is the default and what everything here is built around; the **Cline CLI** is also supported, with its own key — see [Two agents](#two-agents).

- **Isolated** — every blueprint gets its own `.claude-env-<name>/` (settings, persona, hooks, skills), kept out of your repo automatically.
- **Shared login** — one `aello login` token is shared safely across any number of concurrent envs (no credential rotation races). Cline keeps a separate login of its own.
- **Role-driven** — one choice per blueprint: `maintainer` owns the repo's docs and git, `contributor` commits its own work and logs it, `standalone` works alone. aello scaffolds the matching files and generates a `/sync` skill tailored to exactly that.
- **Spoken** — every env reads each response's `TL;DR:` line aloud, with a different voice per concurrent session and one `aello voice mute` to stop them all.
- **Same manners everywhere** — four rules ride every prompt in every env: be concise, don't be sycophantic, never hand over a plan for approval (plan mode is blocked outright), and close with one block — the spoken `TL;DR:` line, with 3–4 next steps beneath it, written to stand alone so you can skip the prose entirely.
- **Accounted** — `aello tokens` (and `T` in the TUI) reports what each env has spent — input, output, cache write, cache read — with an estimated list-rate cost, read back out of transcripts aello was already archiving, so it works over history recorded before the feature existed.
- **Attributable** — commits made through a blueprint are authored as `<blueprint> <blueprint@aello.local>`, so multi-agent work is traceable.

Cross-platform: Linux (x86_64), macOS (Apple Silicon + Intel), Windows (x86_64).

## Install

### Linux / macOS — one-liner

```sh
curl -fsSL https://raw.githubusercontent.com/ryha0008-boop/aello/main/install.sh | sh
```

Downloads the latest release into `~/.local/bin` (override with `AELLO_BIN_DIR`), **verifies it against the release's `SHA256SUMS`**, makes it executable, clears the macOS quarantine flag, and prints a PATH hint if that dir isn't on your `$PATH`. It refuses to install on a checksum mismatch, and says so out loud if no `sha256sum`/`shasum` is available to check with. (That catches a corrupted or truncated download — the manifest travels the same channel as the binary, so it is not tamper protection.) Platforms without a prebuilt binary (e.g. arm64 Linux) exit with a build-from-source pointer.

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
- **Cline** (`npm i -g cline`) only if you create a `--agent cline` blueprint. Not needed otherwise.

## Quick start

```sh
aello login                                   # one-time: store a shared Claude token
aello add coder --model opus --claude-md coder --role maintainer
cd ~/my-project
aello run coder                               # places an isolated env + launches Claude
```

Inside that project, `aello run coder` creates `.claude-env-coder/`, scaffolds `CHANGELOG.md` / `README.md` / `docs/` / a project `CLAUDE.md` (only the ones its role owns, only if missing), adds `.claude-env-*` to `.gitignore` (every role does this), seeds a `/sync` skill tailored to its role, and (on first placement) seeds a starter working-style memory so the env boots with it in `/context`. Type `/sync` inside Claude to reconcile those docs and commit + push.

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

Full documentation: **<https://ryha0008-boop.github.io/aello/docs/>** — start with [workflows](https://ryha0008-boop.github.io/aello/docs/workflows/) for task-shaped walkthroughs, then [concepts](https://ryha0008-boop.github.io/aello/docs/concepts/), [roles](https://ryha0008-boop.github.io/aello/docs/roles/), [skills](https://ryha0008-boop.github.io/aello/docs/skills/), [voice](https://ryha0008-boop.github.io/aello/docs/voice/), [cline](https://ryha0008-boop.github.io/aello/docs/cline/) and [troubleshooting](https://ryha0008-boop.github.io/aello/docs/troubleshooting/).

Those pages are generated from [`docs/`](docs/) in this repo, and the same files ship **inside the binary** — `aello docs` lists them, `aello docs workflows` prints one, and `?` in the TUI opens a reader. No internet needed.

## Commands

```
aello                                          # interactive TUI (no args)
aello --version
aello init                                     # first-run: login + first blueprint
aello add <name> --model <m> [--agent claude|cline] [--claude-md <coder|none|custom|path>]
        [--role maintainer|contributor|standalone]
aello list [--json]
aello remove <name> [--yes] [--purge]         # --purge also deletes the placed env dir + mirror
aello edit <name> [--rename <new>] [--model <m>] [--claude-md <coder|none|custom|path>]
        [--role maintainer|contributor|standalone] [--mirror-dir <path|->]
aello persona <name> --from <file> [--project <dir>]     # install a written persona into a placed env
aello restore <name> [--project <dir>]         # adopt the tracked mirror after pulling another machine's work
aello run [name] [--resume [id]] [-p <prompt>] [-- <extra args for the agent>]
aello login [--agent claude|cline]             # store a shared login (asks which, if unsaid)
aello github-setup [--name <repo>] [--public] [--yes]   # create + push the repo via gh
aello docs [name]                              # print bundled reference docs (no name lists them)
aello check [path] [--all] [--root <dir>] [--json]      # verify a repo's integrations (exit 1 on failure)
aello tokens [name] [--sessions] [--json]      # token usage + estimated cost per env
aello statusline                               # the in-session readout (run by Claude Code, not by hand)
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
- `persona` replaces a placed env's `CLAUDE.md` with a persona you have written for that project, sets the blueprint to `custom` so aello stops seeding a template over it, and records the generation in `<env>/persona.gen` (`gen1 2026-08-03`). It is the only command that overwrites a persona — `run` never does. `aello list` then shows `custom` for that blueprint, so you can see at a glance which envs have a real persona.
- `restore` is for working one blueprint from **two machines**. `aello run` seeds an env from the tracked `claude-internal/<name>/` mirror only when there is no env dir at all (a fresh clone); on the machine that already has one, pulling another machine's commits changes nothing the agent can see. `restore` copies that pulled memory and skills into the env dir and puts the resume note back at the project root. It is additive — memory and skills are merged, a differing persona is reported rather than replaced — so it's safe to run whenever you're unsure. The full loop is in [`docs/workflows.md`](docs/workflows.md#one-env-two-machines).
- `run` with no name uses the sole blueprint (errors if there are several).
- `--resume` with no value continues the most recent session; `--resume <id>` resumes a specific one. The TUI (`S`) browses sessions to resume.
- `-p "<prompt>"` runs headless and exits. Anything after `--` is passed straight to `claude`.

### TUI keys

`↑/↓` move · `↵` run · `F` filter · `S` sessions · `A` add (guided) · `E` edit (guided) · `D` delete · `C` contextdb folder · `M` mute voice · `L` login · `U` update · `T` tokens · `?` docs · `Q` quit. Command keys are case-insensitive, and `Ctrl+C` quits from anywhere. In the docs reader (`?`), `Home`/`End` jump to the start and end of a page.

`M` toggles the same machine-wide mute as `aello voice mute` — it silences every env, not just the selected blueprint, and cuts off whatever is speaking. While muted the footer reads `VOICE: MUTED`, so a silent machine doesn't look like a broken hook.

By default the registry shows only blueprints already placed in the current directory (their `.claude-env-<name>/` exists), which keeps a per-project blueprint workflow tidy. `F` toggles between that local subset and all blueprints; when nothing is placed here yet, all are shown.

`E` edits the selected blueprint through the same guided steps as add, pre-filled with its current model, persona, and role (the name isn't editable). Changes apply on the next `run`.

`?` opens a full-screen docs reader over the repo's `docs/` (`↑/↓` scroll, `Tab`/`←→` switch doc, `Esc` close). The same content is available from the CLI via `aello docs`.

`T` opens the token tab: the current 5-hour window across the top, envs down the left, and the selected env's buckets, per-model split and session list on the right (`↑/↓` env, `PgUp`/`PgDn` scroll, `R` rescan, `Esc` close). The scan reads every archived transcript, so it happens once per TUI session and is cached — the first open shows `SCANNING…` for a few seconds rather than freezing.

## Roles

Pick one per blueprint. **One maintainer per repo, any number of contributors** — that's the shape this is for: the maintainer owns the prose, everyone else commits their own work without rewriting it.

| Role | Scaffolds (if missing) | `/sync` |
|---|---|---|
| `maintainer` | project `CLAUDE.md`, `CHANGELOG.md`, `docs/`, `README.md`, `.gitattributes`, `VERSION` + bump CI, `claude-internal/<name>/`, `.githooks/pre-commit`, test+audit CI, `renovate.json` | full — repo health, reconcile memory + all four docs, mirror the env, commit & push with an `Env:` trailer |
| `contributor` | `CHANGELOG.md`, `.gitattributes`, `VERSION` + bump CI, `claude-internal/<name>/`, `.githooks/pre-commit`, test+audit CI, `renovate.json` | git only — repo health, `CHANGELOG.md`, mirror the env, commit & push with an `Env:` trailer |
| `standalone` | nothing | none — no `/sync` skill is seeded |

Every role, `standalone` included, adds the `.claude-env-*` line to `.gitignore` — an env dir can hold Claude Code's own `.credentials.json` when no shared token is configured, so that one is not a git duty.

Three of those exist to stop a class of mistake rather than to help you write code:

- **`.githooks/pre-commit`** refuses a commit carrying key material — armored private keys, real `.env` files, certificate bundles, non-placeholder provider API keys. `/sync` mirrors your agent's *memory* into git, and a memory note is where a session writes down a credential it just used. It says nothing about IP addresses or hostnames on purpose: a check that cries wolf gets bypassed with `--no-verify`, which takes the real check with it. `git config core.hooksPath .githooks` is re-run on every placement, because that setting is per-clone and does not travel with a pull.
- **`.github/workflows/ci.yml`** runs the tests and audits the dependencies on every push. It detects Python, Node or Rust at run time rather than being generated for one, so it drops into any repo; a repo with no manifest it recognises says so rather than passing silently.
- **`.github/renovate.json`** sets the update policy: grouped minor/patch weekly, majors on their own PR, security updates off-schedule, nothing automerged. It does nothing until you install the [Renovate GitHub App](https://github.com/apps/renovate) — that part is yours.

**`--mirror-dir` sends the `claude-internal/` mirror to another repo.** The mirror is an env's memory, persona and handoff, so in a **public** repo staging it is a publish rather than a backup. Point it at a working tree of a private repo and the product stays public while the memory does not; `/sync` then commits there instead, and stops rather than falling back. Without one, `/sync` checks the repo's visibility before staging and stops if it is public.

The persona is separate from the role: `--claude-md <name\|path>` writes the env's global `CLAUDE.md` once, and no role rewrites it.

Upgrading from a pre-0.2 config? The five capability flags are gone and existing blueprints migrate themselves — see [`docs/roles.md`](docs/roles.md).

## Two agents

Every blueprint drives one CLI, chosen at `add` time and fixed:

```sh
aello add Researcher --model opus                                    # Claude Code (default)
aello add Runner --model openai/gpt-5.6-luna-pro --agent cline       # the Cline CLI
```

They share nothing but the project directory — separate env dirs (`.claude-env-<name>` vs `.cline-env-<name>`), separate logins, separate everything. `aello login` asks which account you mean.

⚠️ **A Cline env is metered.** It uses your own provider key and every turn costs money per token, where a Claude env costs nothing beyond the subscription. Its env dir is gitignored unconditionally, because that key sits in plaintext inside it.

A Cline env gets the same persona, the same four response rules, the same `/sync`, `/handoff`, `/note` and `/twosentences`, and a memory — all of it through a rules file Cline re-sends on every request, since it has no per-turn hook to inject into and no memory system of its own.

It is quieter in two ways, and both are limits of Cline rather than choices: **no voice and no transcript capture**, because Cline fires no end-of-response hook. And in headless `-p` mode a command needs a trailing word — `-p "/sync now"` — because Cline refuses any one-word prompt. Full detail, including why the Claude subscription can't drive a Cline env that edits files: [`docs/cline.md`](docs/cline.md).

## Voice — every Claude env speaks

Every Claude env reads the last line of each response aloud, so you can leave an agent working and hear when it lands. There is nothing to switch on. (A Cline env is silent — see above.)

```sh
aello voice mute              # silence every env, and stop the sentence playing now
aello voice mute --project    # silence just this project
aello voice unmute            # (--project too)
aello voice stop              # cut off what's speaking, without muting
aello voice status            # muted or not, and which version the hook is
```

These work from any directory and need no setup — which is exactly where you are when a machine you didn't expect to talk starts talking. `M` in the TUI is the same switch.

Run several agents at once and each gets a **different voice**, taking turns rather than talking over each other. Each spoken line also raises a desktop notification, for when you're in another window.

Away from the machine? Set `REVOICED_TELEGRAM=1`, `TELEGRAM_BOT_TOKEN` and `TELEGRAM_CHAT_ID` and the same line, plus its audio, is sent to a Telegram chat. All three or it stays off; aello sets none of them. On Windows a user-level variable takes effect in sessions that are **already open** — no relaunch — and a blueprint setting `REVOICED_TELEGRAM=0` (or an empty value) still opts out. If messages stop arriving, `aello voice status` names the last failure.

While it speaks it lowers other applications and puts them back afterwards. An application that goes quiet and then closes — or a reboot mid-sentence — is out of reach of that restore, so each turn also checks the volumes Windows has saved and repairs any left down. Set `REVOICED_SWEEP=signature` if some application is deliberately quiet, or `0` to switch the check off.

**You'll need** Python 3, and `pip install edge-tts` for the good voices — without it you get your OS's built-in voice. On Linux you also need one of `mpv`, `ffplay`, `mpg123` or `cvlc` to play audio.

Not hearing anything? Run `aello voice status` first. [`docs/voice.md`](docs/voice.md) covers the rest.

## Token usage

`aello tokens` reports what each env has spent, split into input / output / cache-write / cache-read — the four buckets that price 20x apart — plus an estimated cost. `T` in the TUI shows the same thing.

```sh
aello tokens                 # per-env totals + the current 5-hour window
aello tokens <name>          # one env
aello tokens --sessions      # per-session breakdown
aello tokens --stats         # projects ranked by tokens/session, where the money goes
aello tokens --json          # machine-readable
```

`--stats` (and `S` on the TUI tokens tab, which charts the same numbers) ranks projects by **tokens ÷ sessions** — how expensive it is to *engage* with a project rather than how much it has been used — and puts each bucket's token share next to its cost share, which disagree violently: cache read is 98% of the tokens and 69% of the money. It also names what it cannot count: archived sessions whose transcript was deleted before aello started copying it contribute zero, and unarchived sessions are only visible from the project directory.

Nothing needs enabling: this reads the transcripts contextdb already archives, so it works retroactively over history recorded before the feature existed. Live sessions in the current directory are counted too; sessions running in other projects appear once they end.

**In the session itself**, every env shows a live readout under the prompt — no setup, it is registered with the env:

```
204k·$9.95 │ 5h·42%·34M·1h57m │ 7d·20%·527M·5d18h
this·6.57M·$4.08 │ sess·20M·$14.14 │ prjt·926M·$638.27
```

Ceilings on top — context tokens, session cost, then each plan window as *percentage · tokens · time to reset*. Spend beneath — last turn, this turn, session, project. Colour carries it: context red, money green, a window green under 80% and red over, and the whole spend row red once a window is spent.

The 5-hour and weekly bars are your **actual plan limits** — Claude Code hands the statusline the utilisation the API reports, which is the one number no transcript contains (so the `aello tokens` 5-hour section, which can only measure against this machine's own peak, is the fallback and this is the real thing). Everything on the third line is transcripts. An env placed before this existed picks it up on its next `aello run`; a `statusLine` you wrote yourself is never replaced. See `docs/tokens.md`.

Three things the output is deliberately explicit about, because all are easy to misread:

- **The token split is not the cost split.** Cache read dominates the token count so heavily (98% on the machine this was built on) that it looks like everything else is noise — but it is only ~70% of the cost, against ~18% for cache writes and ~13% for output. Nor is cache uniformly cheap: a read is 0.1x the input rate, while a 1-hour cache write is **2x** it. Reads win on volume, not on unit price.
- **Cost is an estimate at list API rates, not a bill.** An aello env runs on a Claude subscription, where no per-token charge exists. The figure answers "what would this have cost on the API". A model with no rate in the table is never priced at zero — its tokens are quarantined and the model id is named.
- **The 5-hour percentage is against your own peak block, not your plan's quota.** The subscription limit appears in no transcript, so aello cannot read it and doesn't invent one. The largest 5-hour block ever recorded on the machine is the denominator, and the output says so.

[`docs/tokens.md`](docs/tokens.md) has the details, including why deduplicating by message id is load-bearing (Claude Code writes one record per content block, so summing records overstates output by ~68%).

## Git attribution

For a maintainer or contributor, commits made through a blueprint are authored as `<name> <name@aello.local>`, and `/sync` adds an `Env: <name>` line to each commit — so `git log --author` and `git blame` show which agent did what.

## Configuration

Your blueprints, login token, and transcript folder live in a `config.toml` in your OS's usual config location. The token is stored in plain text on your own machine — regenerate it once a year with `aello login`.

**The transcript folder grows.** When a session ends, aello archives it there — the `/handoff` note plus a full copy of the transcript, including every tool call and result and a summary of the agent's reasoning. Transcripts run about 1.3 MB each and occasionally tens of MB, so expect gigabytes over time. Point it somewhere roomy with `C` in the TUI, or prune it yourself; nothing else reads it. Details in [`docs/concepts.md`](docs/concepts.md).

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
