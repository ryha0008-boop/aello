//! Launching `claude` inside an isolated env via CLAUDE_CONFIG_DIR.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Credentials that must not reach a launched agent by inheritance.
///
/// An env's auth is aello's to choose: the shared OAuth token, or nothing and
/// Claude's own login prompt. The launching shell's environment was neither.
/// Agents run `aello` from inside an aello env in this project, so an ambient
/// `CLAUDE_CODE_OAUTH_TOKEN` is routine — and the `no token configured` branch
/// printed "Claude will prompt login" while the env silently authenticated as
/// whoever owns that variable. On the Cline side it is worse than confusing:
/// Cline's `claude-code` provider would pick up aello's shared subscription
/// token in place of the per-env metered key the blueprint was created for.
///
/// Removed on the *child*, never unset in this process — `aello` itself still
/// reads its own environment normally.
const INHERITED_CREDENTIALS: &[&str] =
    &["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

/// Strip [`INHERITED_CREDENTIALS`] from a child command. Call before setting any
/// credential aello does choose, so that one wins.
pub fn scrub_inherited_credentials(c: &mut Command) {
    for k in INHERITED_CREDENTIALS {
        c.env_remove(k);
    }
}

/// Per-blueprint git identity. Multiple blueprints edit the same repo, so
/// attributing commits to the blueprint makes `git blame` / `git log --author`
/// reveal which one made each change. Email is synthetic (`<name>@aello.local`).
pub fn git_identity(name: &str) -> (String, String) {
    (name.to_string(), format!("{name}@aello.local"))
}

/// Spawn `claude` with `CLAUDE_CONFIG_DIR` set to the env dir, inheriting the
/// terminal. Subscription auth — no API keys are set. Returns the exit code.
/// The `claude` executable to spawn.
///
/// On Windows, `Command::new("claude")` resolves only `claude.exe` — an
/// npm-installed Claude Code is a `claude.cmd` shim, which is invisible to it, so
/// a perfectly working install reported "claude is not installed". Probe PATH for
/// the shim forms and fall back to the bare name everywhere else.
fn claude_exe() -> std::ffi::OsString {
    #[cfg(windows)]
    {
        if let Ok(path) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path) {
                for ext in ["exe", "cmd", "bat"] {
                    let cand = dir.join(format!("claude.{ext}"));
                    if cand.is_file() {
                        return cand.into_os_string();
                    }
                }
            }
        }
    }
    std::ffi::OsString::from("claude")
}

/// Ask Claude Code for a readable summary of each turn's reasoning.
///
/// Without it the API's default is `omitted`: the response still carries
/// `thinking` blocks, but their text is an empty string and only an opaque
/// signature survives. That is what lands in the transcript, so contextdb
/// archived a complete record of what was *done* and nothing of what was
/// *thought* — measured 2026-08-03 across 53 transcripts and 2,842 thinking
/// blocks, every one empty.
///
/// `display` controls visibility only — thinking happens and is billed
/// identically either way, so this costs nothing. The raw chain of thought is
/// never returned on any model; a summary is the most that exists.
///
/// Unconditional for the same reason the voice is: it applies to every env and
/// there is no per-blueprint reason to differ. `aello run <name> -- \
/// --thinking-display omitted` turns it off for one run, since user extras are
/// appended after this.
const THINKING_DISPLAY: &[&str] = &["--thinking-display", "summarized"];

pub fn launch(
    env_dir: &Path,
    name: &str,
    resume: Option<&Option<String>>,
    prompt: Option<&str>,
    extra: &[String],
    contextdb: &Path,
    oauth_token: Option<&str>,
) -> Result<i32> {
    let mut c = Command::new(claude_exe());
    scrub_inherited_credentials(&mut c);
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
    c.args(THINKING_DISPLAY);
    // User `--` extras go last so any of them override what aello chose above.
    c.args(extra);

    let status = c
        .status()
        .context("could not launch 'claude' — is Claude Code installed and on PATH?")?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scrub has to reach the child as an explicit *removal*, not merely an
    /// absence — the child inherits this process's environment otherwise, and
    /// this process is very often an aello env with a token in it.
    #[test]
    fn launched_agents_do_not_inherit_the_shells_credentials() {
        let mut c = Command::new("claude");
        scrub_inherited_credentials(&mut c);
        let removed: Vec<String> = c
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for k in INHERITED_CREDENTIALS {
            assert!(removed.contains(&k.to_string()), "{k} still reaches the child");
        }
    }

    /// aello's own token still wins: the scrub runs first, then the `env` call.
    #[test]
    fn the_configured_token_survives_the_scrub() {
        let mut c = Command::new("claude");
        scrub_inherited_credentials(&mut c);
        c.env("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-configured");
        let set: Vec<_> = c
            .get_envs()
            .filter(|(k, _)| k.to_string_lossy() == "CLAUDE_CODE_OAUTH_TOKEN")
            .filter_map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(set, ["sk-ant-oat01-configured"]);
    }

    #[test]
    fn git_identity_is_blueprint_scoped() {
        assert_eq!(
            git_identity("coder"),
            ("coder".to_string(), "coder@aello.local".to_string())
        );
    }

    /// `summarized` is the only value that puts reasoning text in the response —
    /// the API's default (`omitted`) ships thinking blocks whose text is empty,
    /// which is what left contextdb with no record of reasoning at all. A typo
    /// here fails nothing at runtime: Claude Code would reject the value, or
    /// worse accept `omitted` and archive empty blocks exactly as before.
    #[test]
    fn thinking_display_asks_for_summaries() {
        assert_eq!(THINKING_DISPLAY, &["--thinking-display", "summarized"]);
    }
}
