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

**Next (not yet done):** draft 5–8 `good first issue`s from CLAUDE.md "Deferred" (e.g. blueprint *rename* support, a `curl|sh` installer, macOS-from-source docs); enable GitHub Discussions (referenced by issue config.yml + CONTRIBUTING); consider HN/r/rust/r/ClaudeAI launch leading with the *pain* (multiple Claude Code personas in one repo clobber each other's config/memory/auth). [[aello-overview]]
