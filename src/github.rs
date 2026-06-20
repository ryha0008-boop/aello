//! `aello github-setup` — create the GitHub repo for the current project and
//! push it, so a github-capable blueprint has a remote to `/sync` against.
//!
//! This is the aello-driven counterpart to the repo creation that `/sync` only
//! *offers* at runtime: precheck `gh` auth, ensure a git repo with an initial
//! commit, then `gh repo create` (which sets `origin` and pushes in one shot).

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

    // 2. A git repo with at least one commit (gh's --push needs a commit).
    ensure_git_repo(&project)?;

    // 3. If origin already exists, there's nothing to set up.
    if let Some(url) = remote_url(&project, "origin") {
        println!("origin already set to {url} — nothing to do.");
        return Ok(());
    }

    // 4. Resolve the repo name and confirm.
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
        git(project, &["add", "-A"])?;
        if !ok(project, "git", &["diff", "--cached", "--quiet"]) {
            git(project, &["commit", "-m", "Initial commit"])?;
        } else {
            bail!("nothing to commit — add at least one file before setting up the repo");
        }
    }
    Ok(())
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
}
