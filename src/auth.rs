//! Long-lived OAuth token capture via `claude setup-token`.
//!
//! The token (1-year, non-rotating) is shared across all envs as
//! CLAUDE_CODE_OAUTH_TOKEN — concurrency-safe, unlike copied `.credentials.json`
//! (whose refresh tokens rotate and break parallel envs).

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Run `claude setup-token`, capturing the printed token. Its stdout carries the
/// auth URL (critical on a headless VPS) AND the token, so we tee it: each line
/// is echoed to our stdout as it arrives — the URL shows live — while we scan
/// for the token. Falls back to pasting if the token can't be parsed. Returns
/// None if the user cancels.
pub fn capture_setup_token() -> Result<Option<String>> {
    println!("Running 'claude setup-token' — complete the login in your browser...");
    let mut child = Command::new("claude")
        .arg("setup-token")
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::piped())
        .spawn()
        .context("could not run 'claude setup-token' — is Claude Code on PATH?")?;

    let mut captured = String::new();
    if let Some(stdout) = child.stdout.take() {
        let mut out = std::io::stdout();
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap_or_default();
            // Echo live so the auth URL is visible (headless VPS has no browser),
            // but never re-emit the line carrying the token into our own
            // (redirectable) stdout — `aello login | tee`, CI logs, tmux capture
            // would otherwise persist the long-lived token in cleartext.
            if line_has_token(&line) {
                let _ = writeln!(out, "<token received — hidden from stdout, stored in config.toml>");
            } else {
                let _ = writeln!(out, "{line}");
            }
            let _ = out.flush();
            captured.push_str(&line);
            captured.push('\n');
        }
    }
    child.wait().context("'claude setup-token' failed")?;

    if let Some(tok) = extract_token(&captured) {
        return Ok(Some(tok));
    }

    // Couldn't parse it from stdout — let the user paste it.
    print!("Couldn't read the token automatically. Paste it (sk-ant-...), or blank to cancel: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let t = line.trim();
    Ok(if t.is_empty() { None } else { Some(t.to_string()) })
}

/// If a whitespace-delimited word, stripped of surrounding quotes/punctuation,
/// looks like a setup token, return the cleaned token — so a trailing `.` or `"`
/// doesn't truncate or corrupt it. Shared by extraction and stdout redaction.
fn clean_token(word: &str) -> Option<&str> {
    let t = word.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    (t.starts_with("sk-ant-") && t.len() > 20).then_some(t)
}

/// Find a `sk-ant-...` token in arbitrary output. Scans from the end (the token
/// is printed after the auth URL, so the *last* match is the token even if an
/// earlier line embeds the prefix).
fn extract_token(s: &str) -> Option<String> {
    s.split_whitespace().filter_map(clean_token).next_back().map(str::to_string)
}

/// True if a single line contains a token — used to redact it before echoing.
fn line_has_token(line: &str) -> bool {
    line.split_whitespace().any(|w| clean_token(w).is_some())
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
        // A token wrapped in punctuation is cleaned, not truncated.
        assert_eq!(
            extract_token("token: \"sk-ant-oat01-ABCDEF0123456789xyz\".").as_deref(),
            Some("sk-ant-oat01-ABCDEF0123456789xyz")
        );
        // The last match wins (the token is printed after any earlier line).
        let two = "earlier sk-ant-oat01-DECOY00000000000000\nfinal: sk-ant-oat01-REALTOKEN0123456789abc";
        assert_eq!(
            extract_token(two).as_deref(),
            Some("sk-ant-oat01-REALTOKEN0123456789abc")
        );
    }

    #[test]
    fn detects_token_line_for_redaction() {
        assert!(line_has_token("Your token: sk-ant-oat01-ABCDEF0123456789xyz"));
        assert!(line_has_token("\"sk-ant-oat01-ABCDEF0123456789xyz\""));
        // The auth-URL line (and any other non-token line) is not redacted.
        assert!(!line_has_token("Visit https://claude.ai/oauth/authorize?code=xyz"));
        assert!(!line_has_token("Success! Logged in."));
    }
}
