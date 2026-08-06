//! `aello github-setup` — create the GitHub repo for the current project and
//! push it, so a github-capable blueprint has a remote to `/sync` against.
//!
//! This is the aello-driven counterpart to the repo creation that `/sync` only
//! *offers* at runtime: precheck `gh` auth, ensure a git repo with an initial
//! commit, then `gh repo create` (which sets `origin` and pushes in one shot).

use crate::models::Agent;
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Arguments for `gh repo create`. `--source=.` + `--remote=origin` + `--push`
/// makes gh set the remote and push the current branch in one shot; gh requires
/// an explicit visibility flag.
fn repo_create_args(name: &str, public: bool) -> Vec<String> {
    vec![
        "repo".into(),
        "create".into(),
        name.into(),
        if public { "--public" } else { "--private" }.into(),
        "--source=.".into(),
        "--remote=origin".into(),
        "--push".into(),
    ]
}

pub fn run(name: Option<String>, public: bool, yes: bool) -> Result<()> {
    let project = std::env::current_dir().context("could not determine current directory")?;

    // 1. gh present and authenticated.
    if !ok(&project, "gh", &["--version"]) {
        bail!("`gh` (GitHub CLI) not found on PATH. Install it: https://cli.github.com");
    }
    if !ok(&project, "gh", &["auth", "status"]) {
        bail!("`gh` is not authenticated. Run `gh auth login` first.");
    }

    // 2. If origin already exists, there's nothing to set up. (Checked before
    //    `git init`, which is deliberate — see the confirm below.)
    if let Some(url) = remote_url(&project, "origin") {
        println!("origin already set to {url} — nothing to do.");
        return Ok(());
    }

    // 3. Resolve the repo name and confirm — BEFORE anything is written. The
    //    confirm used to come after `ensure_git_repo`, so answering "n" left a
    //    `git init` and an `Initial commit` of the whole directory already on
    //    disk with nothing to roll them back: "no" undid nothing.
    let repo = match name {
        Some(n) => n,
        None => project
            .file_name()
            .and_then(|n| n.to_str())
            .context("could not derive a repo name from the directory")?
            .to_string(),
    };
    let visibility = if public { "public" } else { "private" };
    if !yes && !confirm(&format!(
        "Create {visibility} GitHub repo '{repo}', set origin, and push?"
    )) {
        println!("Cancelled.");
        return Ok(());
    }

    // 4. A git repo with at least one commit (gh's --push needs a commit).
    ensure_git_repo(&project)?;

    // 5. Create + push.
    let status = Command::new("gh")
        .args(repo_create_args(&repo, public))
        .current_dir(&project)
        .status()
        .context("failed to run `gh repo create`")?;
    if !status.success() {
        bail!("`gh repo create` failed");
    }
    if let Some(url) = remote_url(&project, "origin") {
        println!("Done — origin = {url}");
    }
    Ok(())
}

/// Ensure the project is a git repo with at least one commit, creating both if
/// needed so `gh repo create --push` has something to push.
fn ensure_git_repo(project: &Path) -> Result<()> {
    if !ok(project, "git", &["rev-parse", "--is-inside-work-tree"]) {
        println!("No git repo here — running `git init`.");
        git(project, &["init"])?;
        // Standardize on `main` regardless of the user's git defaults.
        let _ = git(project, &["checkout", "-B", "main"]);
    }
    if !ok(project, "git", &["rev-parse", "HEAD"]) {
        println!("No commits yet — creating an initial commit.");
        // `git add -A` respects .gitignore, so write the env-dir lines first —
        // this may be a project whose envs were placed by a version that only
        // wrote them for the `github` role, and a Claude env with no shared
        // token holds a `.credentials.json`.
        crate::project::ensure_gitignore_entry(project, Agent::Claude.gitignore_pattern())?;
        crate::project::ensure_gitignore_entry(project, Agent::Cline.gitignore_pattern())?;
        git(project, &["add", "-A"])?;
        // Then check what actually got staged rather than trusting that. An
        // already-tracked env dir ignores .gitignore entirely, and this commit
        // is one `gh repo create --public` away from the internet.
        let staged = staged_paths(project);
        let bad = forbidden_staged(&staged);
        if !bad.is_empty() {
            bail!(
                "refusing to commit — these are agent env paths and may hold credentials:\n  {}\n\
                 Untrack them first (`git rm -r --cached <path>`), then re-run.",
                bad.join("\n  ")
            );
        }
        if !ok(project, "git", &["diff", "--cached", "--quiet"]) {
            let args = initial_commit_args(has_git_identity(project));
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            git(project, &refs)?;
        } else {
            bail!("nothing to commit — add at least one file before setting up the repo");
        }
    }
    Ok(())
}

/// Paths currently staged, one per line, empty on any failure.
fn staged_paths(project: &Path) -> Vec<String> {
    Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(project)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The staged paths that must never reach a commit aello creates: anything
/// inside either agent's env dir, and any `.credentials.json` wherever it sits.
/// Matched on path *components* so a nested `sub/.claude-env-x/…` is caught too.
fn forbidden_staged(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| {
            p.split(['/', '\\']).any(|c| {
                c.starts_with(Agent::Claude.env_prefix())
                    || c.starts_with(Agent::Cline.env_prefix())
                    || c == ".credentials.json"
            })
        })
        .cloned()
        .collect()
}

/// True when git has a usable author identity (`user.name` AND `user.email`
/// set, in any scope). Decides whether the bootstrap commit needs a fallback.
fn has_git_identity(project: &Path) -> bool {
    nonempty_config(project, "user.name") && nonempty_config(project, "user.email")
}

/// True when `git config --get <key>` resolves to a non-empty value.
fn nonempty_config(project: &Path, key: &str) -> bool {
    Command::new("git")
        .args(["config", "--get", key])
        .current_dir(project)
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// Args for the bootstrap `git commit`. With no machine identity, inject a
/// synthetic `aello` author/committer (mirroring aello's per-env `@aello.local`
/// attribution) via per-invocation `-c` flags so the commit always lands,
/// without writing anything to the user's git config. When an identity already
/// exists, it's used unchanged.
fn initial_commit_args(has_identity: bool) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if !has_identity {
        args.extend([
            "-c".into(),
            "user.name=aello".into(),
            "-c".into(),
            "user.email=aello@aello.local".into(),
        ]);
    }
    args.extend(["commit".into(), "-m".into(), "Initial commit".into()]);
    args
}

/// Run a command in `project`, returning true only on a successful exit, with
/// stdout/stderr suppressed (used for precheck-style probes).
fn ok(project: &Path, cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .current_dir(project)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run a git command in `project`, inheriting stdio, erroring on failure.
fn git(project: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(project)
        .status()
        .with_context(|| format!("failed to run `git {}`", args.join(" ")))?;
    if !status.success() {
        bail!("`git {}` failed", args.join(" "));
    }
    Ok(())
}

/// The URL of a git remote, or None if it isn't set.
fn remote_url(project: &Path, remote: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["remote", "get-url", remote])
        .current_dir(project)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!url.is_empty()).then_some(url)
}

/// Yes/No prompt on stdin; defaults to No.
fn confirm(question: &str) -> bool {
    print!("{question} [y/N] ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_args_private_by_default() {
        let a = repo_create_args("my-proj", false);
        assert_eq!(a[..3], ["repo", "create", "my-proj"]);
        assert!(a.contains(&"--private".to_string()));
        assert!(!a.contains(&"--public".to_string()));
        assert!(a.contains(&"--source=.".to_string()));
        assert!(a.contains(&"--remote=origin".to_string()));
        assert!(a.contains(&"--push".to_string()));
    }

    #[test]
    fn create_args_public_when_requested() {
        let a = repo_create_args("my-proj", true);
        assert!(a.contains(&"--public".to_string()));
        assert!(!a.contains(&"--private".to_string()));
    }

    #[test]
    fn initial_commit_uses_config_identity_when_present() {
        let a = initial_commit_args(true);
        assert_eq!(a, ["commit", "-m", "Initial commit"]);
        assert!(!a.contains(&"-c".to_string()));
    }

    #[test]
    fn initial_commit_injects_fallback_identity_when_absent() {
        let a = initial_commit_args(false);
        assert_eq!(a[a.len() - 3..], ["commit", "-m", "Initial commit"]);
        assert!(a.contains(&"user.name=aello".to_string()));
        assert!(a.contains(&"user.email=aello@aello.local".to_string()));
        // `-c` precedes each override so git applies them for this invocation only.
        assert_eq!(a.iter().filter(|s| *s == "-c").count(), 2);
    }

    /// The bootstrap commit is a blanket `git add -A` of a directory nobody has
    /// curated, and `gh repo create` can publish it. `.gitignore` alone is not
    /// enough — an env dir tracked before the ignore line existed stays tracked.
    #[test]
    fn the_bootstrap_commit_refuses_to_stage_an_env_dir() {
        let paths: Vec<String> = [
            "src/main.rs",
            ".claude-env-coder/.credentials.json",
            "docs/readme.md",
            "sub/.cline-env-bot/data/settings/providers.json",
            "nested/.credentials.json",
            "claude-internal/coder/persona.CLAUDE.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let bad = forbidden_staged(&paths);
        assert_eq!(
            bad,
            [
                ".claude-env-coder/.credentials.json",
                "sub/.cline-env-bot/data/settings/providers.json",
                "nested/.credentials.json",
            ]
        );
        // The tracked mirror is deliberately NOT forbidden — committing it is
        // the whole point of `claude-internal/`.
        assert!(forbidden_staged(&["claude-internal/coder/memory/MEMORY.md".into()]).is_empty());
    }
}
