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

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    // Strip a UTF-8 BOM. Every Windows way of creating this file writes one —
    // PowerShell's `Set-Content -Encoding utf8` on 5.1 and Notepad both do — and
    // it is invisible, so without this the FIRST declared name is rejected as
    // "not a usable variable name" and the error blames the user's spelling of a
    // word that is spelled correctly. Measured the first time a real `.aello-env`
    // was created for a real project.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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

/// This machine's configured store script, if it has one.
///
/// Configured-but-missing is an **error**, never a silent `None`. Falling back
/// would send `aello login` down the `config.toml` path and write a fresh
/// plaintext copy of the credential the user moved out of it — the failure would
/// be invisible and its effect the exact opposite of the intent.
pub fn script(cfg: &crate::models::Config) -> Result<Option<PathBuf>> {
    let Some(spec) = cfg.vault.as_deref().filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let path = crate::config::expand_home(spec);
    if !path.is_file() {
        bail!(
            "the configured vault script does not exist: {}\n\
             Point aello at it again (`aello vault <path>`) or unset it (`aello vault --clear`) — \
             until then a login would silently fall back to plaintext in config.toml.",
            path.display()
        );
    }
    Ok(Some(path))
}

/// Hand one of aello's own credentials to the store, under the variable name the
/// launch path reads it back from.
///
/// **Writing is not resolving.** This module's rule is that aello never reads a
/// secret out of the store, and that still holds: there is no verb here that
/// gets one back. `login` already holds the plaintext for a moment — it captured
/// it from `claude setup-token` or from a prompt — so the only question is where
/// it goes next, and a pipe into the store is strictly better than a write into
/// `config.toml`, which keeps it forever in cleartext.
///
/// The value goes over **stdin**, never as an argument: arguments are visible in
/// process listings and in shell history, and `vault.ps1` refuses a `-Value`
/// flag for that reason. One line, which is what its `set -FromStdin` reads.
pub fn store(script: &Path, name: &str, value: &str) -> Result<()> {
    // A newline is what makes the read terminate — `Invoke-Set` uses
    // `StreamReader.ReadLine()`, and a value carrying one of its own would be
    // truncated there rather than here, so refuse instead of storing half a key.
    if value.contains('\n') || value.contains('\r') {
        bail!("refusing to store a multi-line value as {name} — the store reads one line");
    }
    let shell = if cfg!(windows) { "powershell" } else { "pwsh" };
    let mut child = Command::new(shell)
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .args(["set", name, "-FromStdin"])
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not run {} {}", shell, script.display()))?;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(format!("{value}\n").as_bytes())
        .context("could not hand the value to the vault")?;
    let status = child.wait().context("the vault script did not finish")?;
    if !status.success() {
        bail!(
            "the vault refused to store {name} (exit {}). Nothing was saved anywhere — \
             run the login again once it is fixed.",
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into())
        );
    }
    Ok(())
}

/// Set on the child of a re-exec, so the second aello does not do it again.
///
/// A marker on the *process*, not on disk: it describes this one launch and
/// dies with it, so there is nothing to go stale.
pub const REEXEC_GUARD: &str = "AELLO_VAULT_WRAPPED";

/// Which names this launch should ask the store for.
///
/// The project's declared names **always**, even ones already set in the
/// environment — the store is the authority, and an ambient value is exactly how
/// a stale key wins silently. Measured on this machine: a User-scope
/// `OPENROUTER_API_KEY` differed from the stored one and satisfied the
/// declaration check, so the launch "passed" carrying the wrong key.
///
/// aello's own credential only when `config.toml` does not hold it — otherwise a
/// machine that has not moved its token yet would ask the store for something it
/// was never given, and `vault.ps1 run` fails hard on an unknown name.
pub fn wanted(declared: &[String], cfg: &crate::models::Config, cline: bool) -> Vec<String> {
    let mut names = declared.to_vec();
    let mut push = |n: &str| {
        if !names.iter().any(|x| x == n) {
            names.push(n.to_string());
        }
    };
    if cline {
        if cfg.cline.as_ref().and_then(|c| c.api_key.as_ref()).is_none() {
            push(CLINE_KEY_VAR);
        }
    } else if cfg.oauth_token.is_none() {
        push(OAUTH_VAR);
    }
    names
}

/// The argument vector that re-runs this exact command inside the store.
///
/// Split out from the spawn so the shape is testable without a store on disk.
/// Two things go wrong here and neither is visible in a launch that works: the
/// names must be **one comma-joined token** (space-separated, every name after
/// the first becomes part of the command), and there must be **no bare `--`**
/// anywhere.
///
/// The missing separator is not an oversight. `vault.ps1` documents `--` as the
/// way to split names from command, and that is right when a human types it —
/// but under `powershell -File` a bare `--` is eaten by PowerShell's *parameter
/// binder* before the script ever runs, and every argument after it fails to
/// bind ("A positional parameter cannot be found that accepts argument 'run'").
/// Measured three ways, including with `--%`, which does not help. Without a
/// separator the script's documented fallback applies: first bare token is the
/// name list, everything after it is the command.
pub fn reexec_args(script: &Path, names: &[String], exe: &Path, rest: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-NoProfile".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        script.to_string_lossy().into_owned(),
        "run".into(),
        names.join(","),
        // The console has to be inherited, or the TUI renders into a pipe and
        // the user sees nothing. It also turns the store's output masking off,
        // which is the trade: fine for a terminal you are looking at, wrong for
        // a run whose output is redirected to a file.
        "-NoCapture".into(),
        exe.to_string_lossy().into_owned(),
    ];
    args.extend(rest.iter().cloned());
    args
}

/// Re-run this launch inside the store, so the values arrive without anyone
/// typing a wrapper. `Ok(None)` means this launch cannot be wrapped and the
/// caller should say so rather than pretend.
///
/// This is what makes "put it in the vault" survive contact with daily use: the
/// alternative is remembering a second command every time, and the measured
/// result of a credential being inconvenient is it getting pasted somewhere.
pub fn reexec(script: &Path, names: &[String]) -> Result<Option<i32>> {
    let rest: Vec<String> = std::env::args().skip(1).collect();
    // A `--` in the user's own arguments collides with the wrapper: PowerShell's
    // binder breaks on it exactly as it does on one of ours. Refuse and name the
    // manual form — silently dropping the extras, or silently not wrapping,
    // would both look like a working launch.
    if rest.iter().any(|a| a == "--") {
        return Ok(None);
    }
    let exe = std::env::current_exe().context("could not find aello's own path")?;
    let shell = if cfg!(windows) { "powershell" } else { "pwsh" };
    let status = Command::new(shell)
        .args(reexec_args(script, names, &exe, &rest))
        .env(REEXEC_GUARD, "1")
        .status()
        .with_context(|| format!("could not run {} {}", shell, script.display()))?;
    Ok(Some(status.code().unwrap_or(1)))
}

/// How to launch so a stored credential actually reaches the session.
///
/// Storing it is only half: the store is the *outer* process, so an env launched
/// plainly sees nothing. Printed after every successful store, because that is
/// the moment the old habit (`aello run <bp>`) stops working.
pub fn launch_hint(names: &[&str]) -> String {
    format!(
        "Launch through the vault so it reaches the env:\n  \
         vault.ps1 run {} -NoCapture -- aello run <blueprint>",
        names.join(",")
    )
}

/// What to say when a credential aello needs is not available.
///
/// The two causes have opposite fixes and must not be confused: without a store
/// the answer is "log in", with one it is almost always "you did not launch
/// through the vault". Saying "run `aello login`" to the second user makes them
/// write a fresh plaintext copy of the credential they just moved out.
pub fn missing_credential_hint(vault: Option<&Path>, var: &str, login_cmd: &str) -> String {
    match vault {
        Some(_) => format!(
            "no {var} in this environment — the vault holds it, so this launch has to go \
             through it.\n{}",
            launch_hint(&[var])
        ),
        None => format!("no shared credential — run `{login_cmd}`"),
    }
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

    /// Every Windows way of creating this file writes a UTF-8 BOM, and it is
    /// invisible — so without stripping it the first declared name is refused
    /// with an error blaming a word that is spelled correctly. Hit on the very
    /// first real `.aello-env`, created with PowerShell's `Set-Content`.
    #[test]
    fn a_utf8_bom_does_not_poison_the_first_name() {
        assert_eq!(parse("\u{feff}OPENROUTER_API_KEY\n").unwrap(), ["OPENROUTER_API_KEY"]);
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

    /// The `--` and the comma-joined list are the two things that go wrong here
    /// and neither is visible in a launch that works: without `--` the store
    /// reads aello's own arguments as secret names, and a space-separated list
    /// makes every name after the first part of the command.
    #[test]
    fn the_reexec_command_separates_the_names_from_the_command() {
        let args = reexec_args(
            Path::new("v.ps1"),
            &["A".to_string(), "B".to_string()],
            Path::new("aello.exe"),
            &["run".to_string(), "Probe".to_string()],
        );
        let run = args.iter().position(|a| a == "run").expect("no run verb");
        assert_eq!(args[run + 1], "A,B", "names must be one comma-joined token: {args:?}");
        assert_eq!(args[run + 2], "-NoCapture");
        assert_eq!(&args[run + 3..], ["aello.exe", "run", "Probe"]);
        // A bare `--` anywhere is eaten by PowerShell's parameter binder under
        // `-File`, and every argument after it then fails to bind. Measured.
        assert!(!args.iter().any(|a| a == "--"), "a bare -- breaks the binder: {args:?}");
    }

    /// A project that declares nothing, on a machine with a store, must not be
    /// wrapped — otherwise every existing launch grows a PowerShell process and
    /// asks the store for a name it may not hold.
    #[test]
    fn nothing_to_fetch_means_no_wrapper() {
        let mut cfg = crate::models::Config { oauth_token: Some("t".into()), ..Default::default() };
        assert!(wanted(&[], &cfg, false).is_empty());
        // …but a token that has already left `config.toml` is something to fetch.
        cfg.oauth_token = None;
        assert_eq!(wanted(&[], &cfg, false), [OAUTH_VAR]);
    }

    /// Declared names are fetched even when already set. An ambient value is how
    /// a stale key wins silently — measured here, a User-scope
    /// `OPENROUTER_API_KEY` differed from the stored one and still satisfied the
    /// declaration check, so the launch carried the wrong key and reported fine.
    #[test]
    fn a_declared_name_is_fetched_even_when_the_environment_already_has_one() {
        let cfg = crate::models::Config { oauth_token: Some("t".into()), ..Default::default() };
        assert_eq!(wanted(&["OPENROUTER_API_KEY".to_string()], &cfg, false), ["OPENROUTER_API_KEY"]);
    }

    /// A Claude blueprint must not ask the store for the Cline key, and vice
    /// versa: `vault.ps1 run` fails hard on a name it does not hold, so asking
    /// for the wrong one turns a working launch into an error.
    #[test]
    fn each_agent_asks_only_for_its_own_credential() {
        let cfg = crate::models::Config {
            cline: Some(crate::models::ClineAuth {
                provider: "openrouter".into(),
                api_key: None,
                model: "m".into(),
                base_url: None,
            }),
            ..Default::default()
        };
        assert_eq!(wanted(&[], &cfg, true), [CLINE_KEY_VAR]);
        assert_eq!(wanted(&[], &cfg, false), [OAUTH_VAR]);
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

    fn cfg_with_vault(spec: Option<&str>) -> crate::models::Config {
        crate::models::Config {
            vault: spec.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn no_vault_configured_is_a_silent_none() {
        assert!(script(&cfg_with_vault(None)).unwrap().is_none());
        // Whitespace is the shape a hand-edited config leaves behind.
        assert!(script(&cfg_with_vault(Some("   "))).unwrap().is_none());
    }

    /// A configured-but-missing script must NOT degrade to "no vault". That
    /// fallback would send `aello login` down the `config.toml` path and write a
    /// fresh plaintext copy of the credential the setting exists to move out —
    /// silently, and with the opposite of the intended effect.
    #[test]
    fn a_vault_that_is_not_there_is_an_error_not_a_fallback() {
        let err = script(&cfg_with_vault(Some("/nope/vault.ps1"))).unwrap_err().to_string();
        assert!(err.contains("does not exist"), "{err}");
        assert!(err.contains("plaintext"), "{err}");
    }

    #[test]
    fn a_real_path_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("vault.ps1");
        std::fs::write(&p, "").unwrap();
        let found = script(&cfg_with_vault(Some(&p.to_string_lossy()))).unwrap();
        assert_eq!(found.as_deref(), Some(p.as_path()));
    }

    /// The store reads ONE line, so a value carrying a newline would be stored
    /// truncated — a credential that is silently half a credential. Refuse here,
    /// where the error can still name what happened.
    #[test]
    fn a_multi_line_value_is_refused_rather_than_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("vault.ps1");
        std::fs::write(&p, "").unwrap();
        assert!(store(&p, OAUTH_VAR, "line-one\nline-two").is_err());
    }

    /// The two causes of a missing credential have opposite fixes, and naming
    /// the wrong one is actively harmful: told to run `aello login`, a user with
    /// a vault writes a fresh plaintext copy of what they just moved out of it.
    #[test]
    fn the_missing_credential_hint_names_the_fix_that_matches_the_setup() {
        let with = missing_credential_hint(Some(Path::new("v.ps1")), OAUTH_VAR, "aello login");
        assert!(with.contains("vault.ps1 run"), "{with}");
        assert!(!with.contains("aello login"), "{with}");

        let without = missing_credential_hint(None, OAUTH_VAR, "aello login");
        assert!(without.contains("aello login"), "{without}");
        assert!(!without.contains("vault.ps1 run"), "{without}");
    }

    /// Storing is only half the job — the store is the *outer* process, so an
    /// env launched plainly still sees nothing. The hint has to carry the name
    /// under which it was stored, or the wrapper injects the wrong variable.
    #[test]
    fn the_launch_hint_carries_the_variable_name_and_the_wrapper() {
        let h = launch_hint(&[OAUTH_VAR, CLINE_KEY_VAR]);
        assert!(h.contains(OAUTH_VAR), "{h}");
        assert!(h.contains(CLINE_KEY_VAR), "{h}");
        assert!(h.contains("-NoCapture"), "{h}");
        assert!(h.contains("aello run"), "{h}");
    }

    /// The value must reach the store on **stdin**, never as an argument —
    /// arguments are visible in process listings and shell history, which is why
    /// `vault.ps1` has no `-Value` flag to use instead. Measured against a stub
    /// that records what it was given, rather than reasoned from the arg list.
    ///
    /// Windows-only: the store is DPAPI and the interpreter is `powershell`,
    /// which Linux CI does not have.
    #[cfg(windows)]
    #[test]
    fn the_value_goes_over_stdin_and_never_into_the_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("seen.txt");
        let stub = dir.path().join("vault.ps1");
        std::fs::write(
            &stub,
            format!(
                "param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Argv)\n\
                 $line = (New-Object IO.StreamReader([Console]::OpenStandardInput())).ReadLine()\n\
                 Set-Content -LiteralPath '{}' -Value \"args=$($Argv -join ' ')`nstdin=$line\"\n\
                 exit 0\n",
                out.to_string_lossy().replace('\\', "\\")
            ),
        )
        .unwrap();

        store(&stub, OAUTH_VAR, "sk-ant-oat-SECRET").unwrap();
        let seen = std::fs::read_to_string(&out).unwrap();
        assert!(seen.contains("stdin=sk-ant-oat-SECRET"), "{seen}");
        assert!(seen.contains(&format!("args=set {OAUTH_VAR} -FromStdin")), "{seen}");
        let args_line = seen.lines().find(|l| l.starts_with("args=")).unwrap();
        assert!(!args_line.contains("SECRET"), "the value reached the command line: {args_line}");
    }

    /// A store that refuses must fail the login loudly. Reporting success here
    /// would leave the credential nowhere at all — not in the vault, and (by
    /// then) deliberately not in `config.toml` either.
    #[cfg(windows)]
    #[test]
    fn a_refusing_store_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("vault.ps1");
        std::fs::write(&stub, "exit 1\n").unwrap();
        let err = store(&stub, OAUTH_VAR, "value").unwrap_err().to_string();
        assert!(err.contains("Nothing was saved"), "{err}");
    }

    #[test]
    fn the_missing_message_names_the_file_and_the_names() {
        let msg = missing_message(Path::new("/p"), &["OPENROUTER_API_KEY".to_string()]);
        assert!(msg.contains(DECL_FILE), "{msg}");
        assert!(msg.contains("OPENROUTER_API_KEY"), "{msg}");
        assert!(msg.contains("vault.ps1"), "{msg}");
    }
}
