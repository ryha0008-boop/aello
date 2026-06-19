//! Launching `claude` inside an isolated env via CLAUDE_CONFIG_DIR.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Per-blueprint git identity. Multiple blueprints edit the same repo, so
/// attributing commits to the blueprint makes `git blame` / `git log --author`
/// reveal which one made each change. Email is synthetic (`<name>@aello.local`).
pub fn git_identity(name: &str) -> (String, String) {
    (name.to_string(), format!("{name}@aello.local"))
}

/// Spawn `claude` with `CLAUDE_CONFIG_DIR` set to the env dir, inheriting the
/// terminal. Subscription auth — no API keys are set. Returns the exit code.
pub fn launch(
    env_dir: &Path,
    name: &str,
    resume: Option<&Option<String>>,
    prompt: Option<&str>,
    extra: &[String],
    contextdb: &Path,
    oauth_token: Option<&str>,
) -> Result<i32> {
    let mut c = Command::new("claude");
    c.env("CLAUDE_CONFIG_DIR", env_dir);
    // Unified transcript folder for the PostCompact hook.
    c.env("AELLO_CONTEXTDB", contextdb);
    // Per-env git attribution — set author AND committer so both `git blame`
    // and `git log` reveal the blueprint regardless of the machine's git config.
    let (git_name, git_email) = git_identity(name);
    c.env("GIT_AUTHOR_NAME", &git_name);
    c.env("GIT_AUTHOR_EMAIL", &git_email);
    c.env("GIT_COMMITTER_NAME", &git_name);
    c.env("GIT_COMMITTER_EMAIL", &git_email);
    // Long-lived OAuth token — concurrency-safe shared login (no rotation).
    if let Some(t) = oauth_token {
        c.env("CLAUDE_CODE_OAUTH_TOKEN", t);
    }

    match resume {
        Some(Some(id)) => {
            c.args(["--resume", id]);
        }
        Some(None) => {
            c.arg("--continue");
        }
        None => {}
    }
    if let Some(p) = prompt {
        c.args(["-p", p]);
    }
    c.args(extra);

    let status = c
        .status()
        .context("could not launch 'claude' — is Claude Code installed and on PATH?")?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_identity_is_blueprint_scoped() {
        assert_eq!(
            git_identity("coder"),
            ("coder".to_string(), "coder@aello.local".to_string())
        );
    }
}
