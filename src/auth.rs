//! Long-lived OAuth token capture via `claude setup-token`.
//!
//! The token (1-year, non-rotating) is shared across all envs as
//! CLAUDE_CODE_OAUTH_TOKEN — concurrency-safe, unlike copied `.credentials.json`
//! (whose refresh tokens rotate and break parallel envs).

use anyhow::{Context, Result};
use std::process::{Command, Stdio};

/// Run `claude setup-token`, letting the browser/login flow show on stderr while
/// capturing stdout to grab the printed token. Falls back to pasting if the
/// token can't be parsed. Returns None if the user cancels.
pub fn capture_setup_token() -> Result<Option<String>> {
    println!("Running 'claude setup-token' — complete the login in your browser...");
    let out = Command::new("claude")
        .arg("setup-token")
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::piped())
        .spawn()
        .context("could not run 'claude setup-token' — is Claude Code on PATH?")?
        .wait_with_output()
        .context("'claude setup-token' failed")?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Some(tok) = extract_token(&stdout) {
        return Ok(Some(tok));
    }

    // Couldn't parse it from stdout — let the user paste it.
    print!("Couldn't read the token automatically. Paste it (sk-ant-...), or blank to cancel: ");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let t = line.trim();
    Ok(if t.is_empty() { None } else { Some(t.to_string()) })
}

/// Find a `sk-ant-...` token in arbitrary output.
fn extract_token(s: &str) -> Option<String> {
    s.split_whitespace()
        .find(|w| w.starts_with("sk-ant-") && w.len() > 16)
        .map(|w| w.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_token() {
        let out = "Success!\nYour token:\nsk-ant-oat01-ABCDEF0123456789xyz\nDone.";
        assert_eq!(
            extract_token(out).as_deref(),
            Some("sk-ant-oat01-ABCDEF0123456789xyz")
        );
        assert!(extract_token("no token here").is_none());
    }
}
