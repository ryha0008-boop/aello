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
    if !roles.is_empty() {
        s.push_str(
            "
## Reconcile the docs (only files that exist)
For each file below that exists, compare it against the current code and recent commits, then make it accurate. This is a **two-way reconcile, not append-only**: add what's missing, **correct what's now wrong**, and **delete what no longer applies**. Report per file: updated / already-fresh / skipped (absent).

",
        );
        s.push_str(&roles.join("\n"));
        s.push('\n');
    }

    if caps.github {
        s.push_str(&format!(
            "
## Commit + push
- Stage changes (`git add -A`), commit with a clear message summarizing what changed, and push to `origin` on the current branch.
- **End every commit message with a trailer line `Env: {name}`** (after a blank line) so the commit records which aello blueprint made it. Your git author identity is already set to this blueprint; the trailer makes it visible in the message body too.
- If the push fails for a missing upstream, set it: `git push -u origin <branch>`.
- Report the final state: branch, commit sha, push result, and the remote URL.

Use normal prose for commit messages. Don't skip hooks or force-push unless the user explicitly asks.
"
        ));
    }

    s
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
        assert!(s.contains("allowed-tools: Read, Edit, Write, Grep, Glob"));
    }

    #[test]
    fn sync_skill_includes_git_and_only_selected_docs() {
        let caps = Capabilities { github: true, changelog: true, ..Default::default() };
        let s = render_sync_skill(&caps, "coder");
        assert!(s.contains("Repo health"));
        assert!(s.contains("Commit + push"));
        assert!(s.contains("Env: coder")); // per-blueprint commit trailer
        assert!(s.contains("CHANGELOG.md"));
        assert!(!s.contains("README.md"));
        assert!(s.contains("allowed-tools: Bash,"));
    }
}
