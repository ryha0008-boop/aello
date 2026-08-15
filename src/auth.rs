//! Long-lived OAuth token capture via `claude setup-token`.
//!
//! The token (1-year, non-rotating) is shared across all envs as
//! CLAUDE_CODE_OAUTH_TOKEN — concurrency-safe, unlike copied `.credentials.json`
//! (whose refresh tokens rotate and break parallel envs).

use anyhow::{bail, Context, Result};
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
                // Where it lands is decided by the caller (vault or
                // `config.toml`), so don't name one of them here — the line
                // outlived the config-only era once already.
                let _ = writeln!(out, "<token received — hidden from stdout>");
            } else {
                let _ = writeln!(out, "{line}");
            }
            let _ = out.flush();
            captured.push_str(&line);
            captured.push('\n');
        }
    }
    let status = child.wait().context("'claude setup-token' failed")?;

    if let Some(tok) = extract_token(&captured) {
        return Ok(Some(tok));
    }

    // Distinguish "the command failed" from "the command worked but we couldn't
    // parse it". Discarding the exit status meant a hard failure — claude not
    // logged in, a network error — was reported as a parse problem, and the user
    // was invited to paste a token that was never issued.
    if !status.success() {
        bail!(
            "'claude setup-token' exited with {} and printed no usable token —              run it directly to see why",
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "a signal".into())
        );
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
///
/// Deliberately **wider** than [`extract_token`]: a plain substring search for
/// the prefix, with none of the whitespace-word structure extraction relies on.
/// Sharing `clean_token` looked tidy and made redaction exactly as narrow as
/// parsing, so `{"token":"sk-ant-…"}` and `CLAUDE_CODE_OAUTH_TOKEN=sk-ant-…`
/// both printed in the clear — the leading `{"token":"` and the interior `=`
/// are not trimmed, so neither word starts with `sk-ant-`. Worse, they are the
/// same cases where parsing fails, so the user was then asked to paste a token
/// aello had just written to a redirectable stdout. Over-redacting costs a line
/// of output; under-redacting persists a year-long credential in a log.
fn line_has_token(line: &str) -> bool {
    line.contains("sk-ant-")
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

    /// Redaction has to be wider than extraction, not equal to it. Each of these
    /// printed a live year-long token to a redirectable stdout while
    /// `extract_token` — sharing the same predicate — also failed to parse it,
    /// so the user was invited to paste what had just been logged.
    #[test]
    fn redaction_catches_tokens_extraction_cannot_parse() {
        assert!(line_has_token(r#"{"token":"sk-ant-oat01-ABCDEF0123456789xyz"}"#));
        assert!(line_has_token("CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-ABCDEF0123456789xyz"));
        assert!(line_has_token("│ sk-ant-oat01-ABCDEF0123456789xyz│"));
        // Even a fragment: a token wrapped across two lines by a boxed UI still
        // has the prefix on the first of them, and half a token is still half a
        // token in a log.
        assert!(line_has_token("your token is sk-ant-oat01-ABC"));
    }
}
