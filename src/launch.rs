//! Launching `claude` inside an isolated env via CLAUDE_CONFIG_DIR.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Spawn `claude` with `CLAUDE_CONFIG_DIR` set to the env dir, inheriting the
/// terminal. Subscription auth — no API keys are set. Returns the exit code.
pub fn launch(
    env_dir: &Path,
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
