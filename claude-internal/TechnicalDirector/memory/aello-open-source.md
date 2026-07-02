---
name: aello-open-source
description: "Open-sourcing aello: dual MIT/Apache, goal=attract contributors, stay pseudonymous, domain deferred; Tier-1 foundation done"
metadata: 
  node_type: memory
  type: project
  originSessionId: eebc699e-d15d-440f-b8f6-fa8ba17b5d44
---

aello's repo is already public, but as of 2026-06-26 we began adding the open-source *foundation* to drive adoption/contributions. Decisions made:

- **License:** dual **MIT OR Apache-2.0** (Rust ecosystem norm). `LICENSE-MIT` + `LICENSE-APACHE` + `Cargo.toml` `license` field + crate metadata (`repository`/`homepage`/`keywords`/`categories`).
- **Goal:** attract contributors (not just users); maintainer commits to active weekly+ maintenance. So community infra is worth investing in.
- **Attribution / privacy:** user is **deliberately pseudonymous** — copyright holder is the GitHub handle **`ryha0008-boop`**, NOT a real name. Rationale: account ownership is provable if ever needed, and a real name in a license ships permanently in every copy + git history. Apply this default to any future attribution/identity choice. [[working-style]]
- **Domain:** DEFERRED. "aello" is a flagged *premium* keyword (aello.ai $50k, .net $2.6k, .xyz $1.4k — all absurd). `aello.sh` ~$63/yr renewal is the only sane on-brand option; `aello.dev` unverified. Decision: **don't buy until the project has traction** — GitHub repo is the hub; a domain only buys a `curl … | sh` one-liner + landing page, both post-traction nice-to-haves. Don't rename the tool to chase a cheaper domain.

**Tier-1 foundation (DONE):** license files, Cargo metadata, `CONTRIBUTING.md` (points to repo CLAUDE.md for architecture), `.github/ISSUE_TEMPLATE/` (bug + feature YAML forms + config.yml routing questions to Discussions), `PULL_REQUEST_TEMPLATE.md`, README Contributing/License sections.

**Progress (2026-06-27):**
- Ran a pre-launch audit (parallel module audits + verified external-dependent findings) and shipped 9 fixes as 7 scoped commits → pushed, CI green, in the rolling release. Headliner: aello's project-path encoding now maps `.`→`-` to match Claude Code (was silently breaking `--resume` + seeded memory on any dotted cwd; confirmed empirically). Others: atomic config.toml save, update download size-guard, mirror one-way *sync* (prunes orphans), TUI docs scroll clamp, token-parse hardening, init EOF, etc. Audit also cleared 3 false alarms (update endpoint, /twosentences tools, SessionEnd subagent guard) — all correct, don't re-flag.
- Filed **6 GitHub issues (#1–#6)** with good-first-issue/help-wanted labels: completions, ASCII `validate_name`, macOS-from-source docs, `aello remove` confirm/purge, `curl|sh` installer, blueprint *rename*.

**Discoverability/contributor work done (2026-06-27, same day):** set 11 GitHub repo **topics** (claude, claude-code, anthropic, ai-agents, coding-agent, llm, cli, rust, developer-tools, agent, devtools) — repo was previously untagged. Added README **badges** (release CI / license / good-first-issues / PRs-welcome) + a Contributing on-ramp pointing newcomers at the `good first issue` label with an inline build+test loop. Wrote **`scripts/demo-recording.md`** — a ready asciinema→GIF recipe (TUI hero shot + two-blueprints-in-one-repo isolation story); the GIF just needs *recording* (can't be done by the agent — needs a real terminal).

**Found, NOT yet fixed — CI double-bump cruft:** aello dogfooded its own `github` cap onto itself, so its `.github/workflows/` has BOTH `release.yml` (bumps Cargo.toml — the real pipeline) AND `version.yml` (bumps a root `VERSION` file — the stack-agnostic CI meant for *target* projects, pointless here). Every push to main fires two bump workflows → two `[skip ci]` commits racing; `VERSION` + `version.yml` also confuse contributors about which version is authoritative. Recommend deleting `version.yml` + `VERSION` (aello uses CARGO_PKG_VERSION; nothing reads VERSION). Left for explicit approval since it touches the release pipeline. [[aello-ci-release]] [[aello-dev-gotchas]]

**Still TODO (MANUAL — only the human can):** enable GitHub **Discussions** (repo Settings → Features; issue config.yml + CONTRIBUTING link to it and 404 until on); **record** the demo GIF from the kit; publish to **crates.io** (metadata ready, needs their token); then HN/r/rust/r/ClaudeAI **launch** leading with the *pain* (multiple Claude Code personas in one repo clobber each other's config/memory/auth). [[aello-overview]]
