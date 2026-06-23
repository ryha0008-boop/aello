//! Built-in CLAUDE.md persona templates, bundled into the binary.
//!
//! A blueprint's `claude_md` is either a built-in name (`coder`, `sysadmin`) or
//! a filesystem path. `resolve` turns it into the actual CLAUDE.md content to
//! place into the env dir as the env's global instructions.

use crate::models::Capabilities;
use anyhow::{Context, Result};

const CODER: &str = include_str!("../templates/coder.md");
const SYSADMIN: &str = include_str!("../templates/sysadmin.md");

/// Names of the built-in templates, for help text and the TUI picker.
#[allow(dead_code)] // used by the TUI persona picker in Increment 3
pub const BUILTINS: &[&str] = &["coder", "sysadmin"];

/// Content of a built-in template by name, or None if not a builtin.
pub fn builtin(name: &str) -> Option<&'static str> {
    match name {
        "coder" => Some(CODER),
        "sysadmin" => Some(SYSADMIN),
        _ => None,
    }
}

/// Resolve a blueprint's `claude_md` value to CLAUDE.md content: a built-in
/// name returns the bundled template; anything else is read as a file path.
pub fn resolve(claude_md: &str) -> Result<String> {
    if let Some(content) = builtin(claude_md) {
        return Ok(content.to_string());
    }
    std::fs::read_to_string(claude_md)
        .with_context(|| format!("claude_md '{claude_md}' is not a built-in template or a readable file"))
}

/// Generate a `/sync` SKILL.md tailored to a blueprint's capabilities, so the
/// skill only covers what this blueprint actually maintains — a no-GitHub
/// blueprint gets no git/commit/push talk at all. `name` is the blueprint name,
/// used for the `Env:` commit trailer. Caller seeds it only when at least one
/// capability is enabled (`Capabilities::any`).
pub fn render_sync_skill(caps: &Capabilities, name: &str) -> String {
    let tools = if caps.github {
        "Bash, Read, Edit, Write, Grep, Glob"
    } else {
        "Read, Edit, Write, Grep, Glob"
    };
    let tail = if caps.github { ", then commit and push" } else { "" };

    let mut s = format!(
        "---
name: sync
description: Checkpoint the project — reconcile the docs this blueprint maintains against the current code{tail}. Invoke manually with /sync.
disable-model-invocation: true
allowed-tools: {tools}
---

# /sync — project checkpoint

When invoked, reconcile the docs this project maintains so they match the current code{tail}. Invoking this skill is your authorization to do so.
"
    );

    if caps.github {
        s.push_str(
            "
## Repo health
- Run `git rev-parse --is-inside-work-tree`. If this is not a git repo, tell the user and stop — this blueprint expects one.
- Check for an `origin` remote (`git remote get-url origin`). If there is none, warn and offer to create one with `gh repo create` — do NOT create it without explicit confirmation.
- Report the current branch (warn on detached HEAD), `git fetch` (best-effort), then report ahead/behind vs the upstream.
- Show a short `git status` summary.
",
        );
    }

    let mut roles: Vec<&str> = Vec::new();
    if caps.project_md {
        roles.push("- **CLAUDE.md** (project root) — project-specific instructions and context for this codebase. Keep it accurate as the project evolves. This is the *project* CLAUDE.md, separate from the global persona.");
    }
    if caps.readme {
        roles.push("- **README.md** — user-facing entry point: what the project is, install, usage, and the command/feature reference. Must reflect current behavior.");
    }
    if caps.changelog {
        roles.push("- **CHANGELOG.md** — version history of user-facing changes. Add new entries under `[Unreleased]` (create it if missing). Match the file's existing style.");
    }
    if caps.docs {
        roles.push("- **docs/** — deeper, topic-by-topic reference docs. Keep each page consistent with actual behavior; don't just duplicate the README.");
    }
    // Memory is reconciled before any doc (and before the github mirror below)
    // so the checkpoint captures what this env learned this session.
    s.push_str(
        "
## Reconcile memory, then docs
**Memory first** — before any doc, refresh this env's memory so the checkpoint (and the mirror below, if any) captures what you've learned this session. Review `MEMORY.md` and the per-fact files under `$CLAUDE_CONFIG_DIR/projects/<this-project>/memory/`: add new facts, correct stale ones, prune what's wrong, and keep the one-line `MEMORY.md` index in sync. Report: memory updated / already-fresh.
",
    );
    if !roles.is_empty() {
        s.push_str(
            "
Then, for each doc file below that exists, compare it against the current code and recent commits, then make it accurate. This is a **two-way reconcile, not append-only**: add what's missing, **correct what's now wrong**, and **delete what no longer applies**. Report per file: updated / already-fresh / skipped (absent).

",
        );
        s.push_str(&roles.join("\n"));
        s.push('\n');
    }

    if caps.github {
        s.push_str(&format!(
            "
## Mirror this env's internal config (tracked)
Version-control this env's internal config by mirroring it into the tracked `claude-internal/{name}/` folder at the repo root, so the skills, memory, and persona that live in the gitignored `.claude-env-{name}/` dir are captured in git. The folder is **namespaced per blueprint** (`claude-internal/{name}/`) so multiple blueprints sharing this repo don't clobber each other's mirror. The live env dir stays the **single source of truth** — this is a **one-way copy** from it, refreshing only what changed.
- Self-heal first: `mkdir -p claude-internal/{name}/skills claude-internal/{name}/memory` — an env placed before this step won't have the folder yet, so create it.
- Mirror `.claude-env-{name}/skills/` → `claude-internal/{name}/skills/`.
- Mirror this env's memory dir (`.claude-env-{name}/projects/<this-project>/memory/`) → `claude-internal/{name}/memory/`.
- Snapshot `.claude-env-{name}/CLAUDE.md` → `claude-internal/{name}/persona.CLAUDE.md` — **keep this exact name**, never `CLAUDE.md`, so Claude Code does not auto-load the snapshot as a second persona.
- Stage it by explicit path: `git add claude-internal/{name}`. This folder is tracked on purpose — it is *not* covered by the `.claude-env-*` gitignore line.
"
        ));
        s.push_str(&format!(
            "
## Commit + push
- Stage **only the files you created or modified in this session**, plus any docs you reconciled above — by explicit path (e.g. `git add path/a path/b`). **Never `git add -A` / `git add .`** — a blanket stage sweeps unrelated untracked files (other tooling's scaffolding, another env's in-flight work) into your commit. Run `git status` first; unstage anything you didn't touch. Then commit with a clear message summarizing what changed.
- **End every commit message with a trailer line `Env: {name}`** (after a blank line) so the commit records which aello blueprint made it. Your git author identity is already set to this blueprint; the trailer makes it visible in the message body too.
- **After committing, before pushing, run `git pull --rebase origin <current-branch>`** to integrate any commits the remote gained since you last fetched (e.g. a release CI's `release: vX [skip ci]` auto-bump). This replays your commit on top so the push is a fast-forward — skipping it leaves you a commit behind and the *next* `/sync` push gets rejected.
- Push to `origin` on the current branch. If the push fails for a missing upstream, set it: `git push -u origin <branch>`.
- Report the final state: branch, commit sha, push result, and the remote URL.

Use normal prose for commit messages. Don't skip hooks or force-push unless the user explicitly asks.
"
        ));
    }

    s
}

/// Generate the `/handoff` SKILL.md. Unlike `/sync`, this is **universal** —
/// seeded for every blueprint regardless of capabilities — because a clean
/// session handoff is useful even for a blueprint that maintains no docs. At
/// session end it writes a self-contained `HANDOFF.md` resume note so the next
/// session picks up seamlessly after a full `/clear` (which, unlike a compact,
/// leaves no summary behind). `name` is the blueprint name, for context.
pub fn render_handoff_skill(name: &str) -> String {
    format!(
        "---
name: handoff
description: Write a self-contained HANDOFF.md resume note so the next session continues seamlessly after a /clear. Invoke manually with /handoff.
disable-model-invocation: true
allowed-tools: Write, Read, Bash
---

# /handoff — session resume note

When invoked, write a `HANDOFF.md` at the project root that lets the **next**
session resume this work with **zero prior context**. Invoking this skill is
your authorization to do so.

A handoff is not a compact: after a `/clear` there is no conversation summary to
fall back on, so `HANDOFF.md` must be **fully self-contained**. Assume the reader
boots fresh, has never seen this conversation, and reads only this file plus the
pointers it names.

`HANDOFF.md` is **transient and untracked** — it is read on boot, then deleted.
Begin the file with a one-line banner: `> Transient resume note ({name}). Read on boot, then delete.`

Write these sections, in order:

1. **Read first** — point the next session at its durable context before
   anything else: the env persona (`$CLAUDE_CONFIG_DIR/CLAUDE.md`) and the
   memory index (`$CLAUDE_CONFIG_DIR/projects/<this-project>/memory/MEMORY.md`).
   Tell it to read those before acting on this note.
2. **Shipped this session** — what actually changed, with commit shas (run
   `git log --oneline` for the recent ones) and a one-line summary each. Note
   anything committed-but-not-pushed or staged-but-not-committed.
3. **Open threads / next steps** — what is in flight, what was deferred, and the
   concrete next action. Be specific enough to act on without re-deriving it.
4. **Gotchas** — traps the next session would otherwise hit: failing/flaky
   tests, environment quirks, decisions made and why, paths that matter.

Keep it tight and skimmable. Then tell the user the note is written and remind
them it is deleted on next boot.
"
    )
}

/// Generate the `/twosentences` SKILL.md. Like `/handoff` this is **universal**
/// — seeded for every blueprint regardless of capabilities. It condenses the
/// previous assistant response into exactly two sentences; a pure text task, so
/// it needs no tools.
pub fn render_twosentences_skill() -> String {
    "---
name: twosentences
description: Summarize your previous response in exactly two sentences. Invoke manually with /twosentences.
disable-model-invocation: true
allowed-tools:
---

# /twosentences — two-sentence summary

When invoked, condense your **previous response** (the most recent assistant
message before this invocation) into **exactly two sentences**.

Output only those two sentences — no preamble, no heading, no bullets, no code,
nothing else. Keep the key facts and the outcome; drop detail, caveats, and
step-by-step explanation.
"
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_resolve() {
        assert!(resolve("coder").unwrap().contains("coding agent"));
        assert!(resolve("sysadmin").unwrap().contains("systems administration"));
    }

    #[test]
    fn unknown_name_is_path_error() {
        assert!(resolve("definitely-not-a-file-or-builtin").is_err());
    }

    #[test]
    fn sync_skill_omits_git_when_no_github() {
        let caps = Capabilities { project_md: true, ..Default::default() };
        let s = render_sync_skill(&caps, "coder");
        assert!(s.contains("CLAUDE.md"));
        assert!(!s.contains("git "));
        assert!(!s.contains("commit and push"));
        assert!(!s.contains("Env: coder")); // no commit trailer without github
        assert!(s.contains("Memory first")); // memory reconcile renders for every blueprint
        assert!(!s.contains("claude-internal")); // mirror is github-only
        assert!(s.contains("allowed-tools: Read, Edit, Write, Grep, Glob"));
    }

    #[test]
    fn sync_skill_includes_git_and_only_selected_docs() {
        let caps = Capabilities { github: true, changelog: true, ..Default::default() };
        let s = render_sync_skill(&caps, "coder");
        assert!(s.contains("Repo health"));
        assert!(s.contains("Commit + push"));
        assert!(s.contains("git pull --rebase origin")); // rebase before push, so the next push fast-forwards
        assert!(s.contains("Env: coder")); // per-blueprint commit trailer
        assert!(s.contains("CHANGELOG.md"));
        assert!(!s.contains("README.md"));
        assert!(s.contains("allowed-tools: Bash,"));
    }

    #[test]
    fn sync_skill_reconciles_memory_before_docs() {
        let caps = Capabilities { changelog: true, ..Default::default() };
        let s = render_sync_skill(&caps, "coder");
        let mem = s.find("Memory first").expect("memory step present");
        let doc = s.find("CHANGELOG.md").expect("doc role present");
        assert!(mem < doc, "memory must be reconciled before the docs");
    }

    #[test]
    fn handoff_skill_is_self_contained_and_manual() {
        let s = render_handoff_skill("coder");
        assert!(s.contains("name: handoff"));
        assert!(s.contains("disable-model-invocation: true"));
        assert!(s.contains("allowed-tools: Write, Read, Bash"));
        assert!(s.contains("HANDOFF.md"));
        assert!(s.contains("zero prior context")); // self-contained, no compact summary
        assert!(s.contains("Read on boot, then delete")); // transient
        assert!(s.contains("commit shas"));
        assert!(s.contains("coder")); // blueprint name woven in
    }

    #[test]
    fn twosentences_skill_is_universal_and_manual() {
        let s = render_twosentences_skill();
        assert!(s.contains("name: twosentences"));
        assert!(s.contains("disable-model-invocation: true"));
        assert!(s.contains("exactly two sentences"));
        assert!(s.contains("previous response"));
    }

    #[test]
    fn sync_skill_mirrors_internal_before_commit() {
        let caps = Capabilities { github: true, ..Default::default() };
        let s = render_sync_skill(&caps, "reviewer");
        // Mirror step names the per-blueprint tracked folder, the env source,
        // the renamed persona snapshot, and self-heals the folder.
        assert!(s.contains("claude-internal/reviewer/")); // namespaced per blueprint
        assert!(s.contains("claude-internal/reviewer/persona.CLAUDE.md"));
        assert!(s.contains(".claude-env-reviewer/skills/")); // env dir is source of truth
        assert!(s.contains("mkdir -p claude-internal/reviewer")); // self-heal already-placed envs
        // The mirror is staged before the commit step runs.
        let mirror = s.find("Mirror this env's internal config").expect("mirror step");
        let commit = s.find("## Commit + push").expect("commit step");
        assert!(mirror < commit, "mirror must be staged before commit");
    }
}
