//! `.aello-env` — a project's declaration of which vault secrets it needs.
//!
//! aello never resolves a secret. It cannot: the vault (`sysadmin/tools/vault.ps1`)
//! is injection-only by design — there is no verb that prints a value, and
//! reimplementing DPAPI here would put plaintext in aello's address space, which
//! is the thing the vault exists to prevent. The value reaches a session because
//! the vault is the *outer* process:
//!
//! ```text
//! vault.ps1 run -NoCapture … -- aello run <blueprint>
//! ```
//!
//! and everything aello spawns inherits it. That was measured rather than
//! assumed (2026-08-15, fake `claude.cmd` first on PATH, isolated
//! `AELLO_CONFIG_DIR`): a variable set on aello's process reaches the `claude`
//! child unchanged, because `std::process::Command` inherits the parent
//! environment and [`crate::launch::scrub_inherited_credentials`] removes only
//! three specific names. So this module adds no injection. It does three
//! smaller things that injection alone leaves broken.
//!
//! **One: fail fast.** Without a declaration, launching outside the vault
//! wrapper produces an agent that works for forty minutes and then hits a 401 —
//! and the measured response to that is the user pasting the key into the chat,
//! which is how an OpenRouter key ended up in twelve transcript records. A
//! missing declared name stops the launch instead.
//!
//! **Two: bare names, never values.** A line containing `=` is refused with an
//! error rather than parsed. The vault's own `Read-EnvFile` accepts
//! `KEY=literal` and passes it through as plaintext, so a file in that format
//! eventually holds a real key; and it *silently drops* a name-only line
//! (measured: a two-line file returned one key, no error). Bare-names-only makes
//! a plaintext value structurally impossible instead of forbidden by convention,
//! which is what lets the file be committed — that is the point of it, since a
//! second machine then learns what a project needs without anyone carrying a
//! secret across.
//!
//! **Three: don't leak sideways.** Agents run `aello` from inside an aello env
//! routinely, so project A's secrets would ride into project B's session while
//! B's own file declares none. [`apply`] strips any name the parent declared
//! that this project does not.

use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

/// The per-project declaration file. Committed on purpose: with bare names it
/// holds no secret, and it is how another machine learns what to supply.
pub const DECL_FILE: &str = ".aello-env";

/// How a launch tells its children which names it declared, so a nested `aello`
/// can strip the ones its own project does not ask for. Self-describing, so
/// aello never needs to know what the vault holds.
pub const DECLARED_VAR: &str = "AELLO_DECLARED";

/// The Claude subscription token, when it comes from the vault instead of
/// `config.toml`.
///
/// Deliberately **not** `CLAUDE_CODE_OAUTH_TOKEN`: that name is in
/// [`crate::launch::INHERITED_CREDENTIALS`] and is stripped from every child, so
/// a vault-supplied one is deleted before it can be used. Measured — with the
/// token removed from `config.toml` the child saw an empty value and every env
/// fell back to an interactive login. Weakening the scrub is not the fix; it
/// exists because an agent running `aello` inside an env would otherwise
/// authenticate as whoever owns the ambient variable.
pub const OAUTH_VAR: &str = "AELLO_OAUTH_TOKEN";

/// The Cline provider key, when it comes from the vault instead of `config.toml`.
pub const CLINE_KEY_VAR: &str = "AELLO_CLINE_API_KEY";

/// Parse a `.aello-env` body: one bare variable name per line. Blank lines and
/// `#` comments are skipped; anything else is an error, never a skip.
///
/// Refusing loudly is the whole design. The vault's parser drops what it does
/// not understand, which is how a file can look configured while injecting
/// nothing — the failure shape this repo keeps hitting.
pub fn parse(text: &str) -> Result<Vec<String>> {
    let mut names: Vec<String> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains('=') {
            bail!(
                "{DECL_FILE} line {}: '{line}' — this file holds bare variable NAMES, one per \
                 line, never values. A value written here would be plaintext in the project. \
                 Store it in the vault under that name instead.",
                i + 1
            );
        }
        if !is_env_name(line) {
            bail!(
                "{DECL_FILE} line {}: '{line}' is not a usable variable name (letters, digits \
                 and underscore, not starting with a digit).",
                i + 1
            );
        }
        if !names.iter().any(|n| n == line) {
            names.push(line.to_string());
        }
    }
    Ok(names)
}

/// A conservative environment-variable name. Rejects the near-misses a hand
/// edit produces — a hyphen, a trailing colon, a `$` prefix — rather than
/// passing them through to a lookup that can only ever fail.
fn is_env_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Read a project's declaration. An absent file means "declares nothing", which
/// is every project today — this must stay a no-op for them.
pub fn read(project: &Path) -> Result<Vec<String>> {
    let path = project.join(DECL_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(anyhow::Error::new(e).context(format!("could not read {}", path.display()))),
    }
}

/// Which declared names are not usable in this process's environment.
///
/// `lookup` is a parameter so this is testable without mutating the real
/// environment, which is process-global and races across parallel tests.
pub fn missing_with<F>(declared: &[String], lookup: F) -> Vec<String>
where
    F: Fn(&str) -> Option<String>,
{
    declared
        .iter()
        // Present-but-empty counts as missing. A variable set to nothing reads
        // as configured everywhere and injects nothing, which is worse than
        // absent because it silences the check written to catch it.
        .filter(|n| lookup(n).is_none_or(|v| v.is_empty()))
        .cloned()
        .collect()
}

/// [`missing_with`] against the real environment.
pub fn missing(declared: &[String]) -> Vec<String> {
    missing_with(declared, |n| std::env::var(n).ok())
}

/// The names the parent launch declared, from [`DECLARED_VAR`].
pub fn inherited_declarations() -> Vec<String> {
    std::env::var(DECLARED_VAR)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Prepare a child command: strip inherited secrets this project did not
/// declare, and tell the child what this project did declare.
///
/// `inherited` is passed rather than read here so the whole thing is testable.
pub fn apply(c: &mut Command, declared: &[String], inherited: &[String]) {
    for name in inherited {
        if !declared.iter().any(|d| d == name) {
            c.env_remove(name);
        }
    }
    if declared.is_empty() {
        // Nothing declared here: don't hand a child a stale list naming secrets
        // it no longer has, which would make the next level strip the wrong set.
        c.env_remove(DECLARED_VAR);
    } else {
        c.env(DECLARED_VAR, declared.join(","));
    }
}

/// A vault-supplied value for one of aello's own credentials, or `None`.
///
/// Empty is `None` for the same reason it is in [`missing_with`].
///
/// ⚠️ **Never write the result back into [`crate::models::Config`].** `aello
/// login` and `aello edit` serialize the whole struct to `config.toml`, so an
/// overlaid value would be persisted in plaintext on the next save — the key
/// would move into the vault and aello would quietly copy it back out. This is
/// resolved at each point of use instead, which is why it returns a value rather
/// than mutating anything.
pub fn env_secret(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The message a launch stops on when a declared secret is absent.
///
/// It names the file, the missing names and the shape of the fix. It does not
/// print a ready-made vault command line: the bare-names reader is SysAdmin's to
/// add and its flag is not settled, and inventing one here would be a wrong
/// instruction that reads as authoritative.
pub fn missing_message(project: &Path, missing: &[String]) -> String {
    format!(
        "{} declares {} which {} not set.\n\
         Launch through the vault so the value never passes through aello:\n  \
         vault.ps1 run -NoCapture … -- aello run <blueprint>\n\
         (the vault reads the same {} file; aello only checks it, and never resolves a secret)",
        project.join(DECL_FILE).display(),
        missing.join(", "),
        if missing.len() == 1 { "is" } else { "are" },
        DECL_FILE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_names_parse_past_blanks_and_comments() {
        let names = parse("# what this project needs\n\nOPENROUTER_API_KEY\n  DATABASE_URL  \n").unwrap();
        assert_eq!(names, ["OPENROUTER_API_KEY", "DATABASE_URL"]);
    }

    /// The structural guarantee: this file cannot hold a value. The vault's own
    /// parser would accept `KEY=literal` and pass it through as plaintext, which
    /// is exactly why aello does not reuse that format.
    #[test]
    fn a_value_line_is_refused_and_the_error_names_it() {
        let err = parse("OPENROUTER_API_KEY=sk-or-v1-real").unwrap_err().to_string();
        assert!(err.contains("line 1"), "{err}");
        assert!(err.contains("bare variable NAMES"), "{err}");
    }

    /// A near-miss must not be skipped. The vault's `Read-EnvFile` drops a line
    /// it cannot parse and returns success, which is how a file looks configured
    /// while injecting nothing.
    #[test]
    fn an_unusable_name_is_an_error_not_a_skip() {
        assert!(parse("OPEN-ROUTER_KEY").is_err());
        assert!(parse("$OPENROUTER").is_err());
        assert!(parse("2FA_TOKEN").is_err());
    }

    #[test]
    fn duplicates_collapse_and_order_is_kept() {
        assert_eq!(parse("B\nA\nB\n").unwrap(), ["B", "A"]);
    }

    /// Absent is the normal case for every project that has never used the
    /// vault, and it must stay silent rather than erroring.
    #[test]
    fn an_absent_file_declares_nothing() {
        let dir = std::env::temp_dir().join("aello-vault-absent-test");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(DECL_FILE));
        assert!(read(&dir).unwrap().is_empty());
    }

    /// Present-but-empty is the case that silences the check written to catch
    /// it — `revoiced` shipped exactly this bug once, where `"" != "0"` made an
    /// empty value read as opted in.
    #[test]
    fn an_empty_variable_counts_as_missing() {
        let declared = vec!["SET".to_string(), "EMPTY".to_string(), "ABSENT".to_string()];
        let missing = missing_with(&declared, |n| match n {
            "SET" => Some("value".into()),
            "EMPTY" => Some(String::new()),
            _ => None,
        });
        assert_eq!(missing, ["EMPTY", "ABSENT"]);
    }

    #[test]
    fn a_parent_secret_this_project_did_not_declare_is_stripped() {
        let mut c = Command::new("claude");
        apply(&mut c, &["MINE".to_string()], &["MINE".to_string(), "THEIRS".to_string()]);
        let removed: Vec<String> = c
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(removed.contains(&"THEIRS".to_string()), "parent secret reached the child");
        assert!(!removed.contains(&"MINE".to_string()), "a shared declaration was stripped");
    }

    #[test]
    fn the_child_is_told_what_this_project_declared() {
        let mut c = Command::new("claude");
        apply(&mut c, &["A".to_string(), "B".to_string()], &[]);
        let set: Vec<String> = c
            .get_envs()
            .filter(|(k, _)| k.to_string_lossy() == DECLARED_VAR)
            .filter_map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(set, ["A,B"]);
    }

    /// Declaring nothing must clear the marker, not leave the parent's list in
    /// place — otherwise the next level down strips against names that are gone.
    #[test]
    fn declaring_nothing_clears_the_marker() {
        let mut c = Command::new("claude");
        apply(&mut c, &[], &["THEIRS".to_string()]);
        let removed: Vec<String> = c
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(removed.contains(&DECLARED_VAR.to_string()));
    }

    /// The two credential variables must not be the names the launch scrub
    /// removes, or a vault-supplied value is deleted before it can be used.
    #[test]
    fn the_credential_vars_are_not_the_scrubbed_names() {
        for v in [OAUTH_VAR, CLINE_KEY_VAR] {
            assert!(
                !crate::launch::INHERITED_CREDENTIALS.contains(&v),
                "{v} is stripped from every child, so a vault-supplied value would vanish"
            );
        }
    }

    #[test]
    fn the_missing_message_names_the_file_and_the_names() {
        let msg = missing_message(Path::new("/p"), &["OPENROUTER_API_KEY".to_string()]);
        assert!(msg.contains(DECL_FILE), "{msg}");
        assert!(msg.contains("OPENROUTER_API_KEY"), "{msg}");
        assert!(msg.contains("vault.ps1"), "{msg}");
    }
}
