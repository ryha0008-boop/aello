//! The Cline half of aello: placing a Cline env and launching it.
//!
//! Everything Cline-specific lives here, and nothing Cline-specific lives
//! anywhere else. `project.rs` and `launch.rs` stay Claude-only. That is the
//! split by design — the two CLIs agree on almost nothing, and a shared code
//! path would end up carrying an `if agent == …` at every branch:
//!
//! | | Claude Code | Cline |
//! |---|---|---|
//! | isolation | `CLAUDE_CONFIG_DIR` env var | `--config` / `--data-dir` flags |
//! | persona | `CLAUDE.md` | `<config>/rules/*.md` (`CLAUDE.md` is ignored) |
//! | auth | one shared subscription token | a provider key, per token spend |
//! | hooks | five events, all used | only `TaskStart`, payload has no content |
//!
//! **Measured 2026-08-06, and the last row is why a Cline env is quieter than a
//! Claude one.** Marker hooks for `TaskStart`, `TaskComplete`, `SessionShutdown`,
//! `UserPromptSubmit` and `PostToolUse` were dropped into both `<config>/hooks/`
//! and a `--hooks-dir` path in a run that made a tool call. Exactly one fired:
//! `<config>/hooks/TaskStart.py`. `--hooks-dir` fired nothing at all. And the
//! payload carries identifiers only — `taskId`, `agent_id`, `workspaceInfo` — no
//! prompt, no response, no transcript path. So there is no end-of-response event
//! to speak from and no per-turn event to inject rules into. The response rules
//! go in a rules file instead, which is the one channel that is measured working.

use crate::models::{Agent, Blueprint, ClineAuth};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The four response rules, as a Cline global rule rather than a per-turn hook.
///
/// Claude envs inject these on every prompt because a style instruction given
/// once decays by turn eighty. Cline has no working `UserPromptSubmit`, so that
/// is not available — but it re-sends its rules in the system prompt on every
/// request, which is the same property arrived at a different way. Verified
/// loading from `<config>/rules/` on 2026-08-06 with a canary rule.
///
/// The TL;DR line is kept even though nothing here speaks it. It costs one line,
/// it is the shape the user reads every other env in, and a Cline env that
/// silently dropped it would be the odd one out for no stated reason.
const RESPONSE_RULES: &str = r#"# Response rules

Be concise: no preamble, no filler, no hedging, no restating the question. Lead
with the answer and stop once it's given. Keep the prose to a few sentences — if
something matters it goes in a step, not a paragraph.

Don't open with praise or agreement. Never validate a premise you haven't
checked, and never soften a finding to be agreeable. Say plainly when the user is
wrong and why; say "I don't know" when you don't.

Never present a plan for approval. Don't lay out what you intend to do and wait
for sign-off. Ask a short question or do the work. When the choice is genuinely
the user's, offer concrete options to pick from rather than a plan to read.

Close every response with one block and nothing after it: a single line of
exactly the form `TL;DR: <two to four sentences>` giving the outcome and what it
means, then — when anything is left for the user to do — 3–4 numbered steps, in
order. Keep the TL;DR on one line. The steps must stand alone: assume the user
reads nothing above the block. Their actions, not yours — steps you are about to
take yourself are a plan. Drop the steps when nothing is waiting on them.
"#;

/// Env dir for a Cline blueprint — `project/.cline-env-<name>`.
pub fn env_dir(project: &Path, name: &str) -> PathBuf {
    project.join(format!("{}{name}", Agent::Cline.env_prefix()))
}

/// Cline keeps provider settings under the **data** dir, not the config dir —
/// measured: a run with both flags set wrote `providers.json` into
/// `<data>/settings/` and left `<config>/` untouched.
fn data_dir(env_dir: &Path) -> PathBuf {
    env_dir.join("data")
}

/// Rules, however, are read from the **config** dir. Measured with a canary in
/// three candidate locations: `<config>/rules/` won over `<data>/rules/` and the
/// project's own `AGENTS.md`.
fn config_dir(env_dir: &Path) -> PathBuf {
    env_dir.join("config")
}

/// Arguments for `cline auth`, which is the only writer whose key survives.
///
/// **aello wrote `providers.json` itself for exactly one afternoon.** The file
/// is small and its shape is obvious from a real one, so hand-writing it looked
/// right — and it half-worked, which is the dangerous part. The env placed, the
/// run launched, the provider was reached and returned an error that read like a
/// bad key. What had actually happened: **Cline rewrote `providers.json` on the
/// next run and dropped the `apiKey` field entirely**, leaving `provider`,
/// `model` and `tokenSource` behind, so the request went out with no credential
/// at all. Measured 2026-08-06 by diffing the file before and after a run. A key
/// installed by `cline auth` survives the same run untouched — so the difference
/// is the writer, not the value.
///
/// The cost is that placement now needs `cline` on PATH. That is not a real
/// cost: nothing can run a Cline env without it.
pub fn auth_args(env_dir: &Path, auth: &ClineAuth) -> Vec<String> {
    let mut args = vec![
        "auth".into(),
        "-p".into(),
        auth.provider.clone(),
        "-m".into(),
        auth.model.clone(),
        "--config".into(),
        config_dir(env_dir).to_string_lossy().into_owned(),
        "--data-dir".into(),
        data_dir(env_dir).to_string_lossy().into_owned(),
    ];
    if let Some(k) = &auth.api_key {
        args.push("-k".into());
        args.push(k.clone());
    }
    if let Some(u) = &auth.base_url {
        args.push("-b".into());
        args.push(u.clone());
    }
    args
}

/// Install the shared credential into this env, every run.
///
/// Not cached behind a marker: the key can change in `config.toml`, and a marker
/// recording "this env is authenticated" would be a cache that goes stale
/// exactly when the truth moves. Re-running it is what makes a rotated key reach
/// every env on its next launch.
pub fn ensure_credential(env_dir: &Path, auth: &ClineAuth) -> Result<()> {
    let status = Command::new(cline_exe())
        .args(auth_args(env_dir, auth))
        .stdout(std::process::Stdio::null())
        .status()
        .context("could not run 'cline auth' — is the Cline CLI installed and on PATH? (npm i -g cline)")?;
    if !status.success() {
        anyhow::bail!("'cline auth' failed for provider '{}' — check the login with `aello login --agent cline`", auth.provider);
    }
    Ok(())
}

/// Write a Cline env into `project/.cline-env-<name>`.
///
/// Rewritten on every run, exactly as `project::place` is, so a credential
/// change or a rules edit reaches an env placed months ago. Deliberately much
/// smaller than the Claude placement: no hooks (only `TaskStart` fires and it
/// carries nothing), no voice, no `/sync` skill, no contextdb capture. What a
/// Cline env gets is isolation and the response rules.
///
/// The credential is **not** written here — see [`ensure_credential`], which
/// shells out to `cline auth` because that is the only writer whose key
/// survives Cline's next run.
pub fn place(env_dir: &Path, bp: &Blueprint) -> Result<()> {
    // Unconditional, and NOT role-gated the way the Claude env's ignore line is.
    // That line is a tidiness measure — a Claude env holds no secret, since auth
    // arrives as an environment variable at launch. This one holds an API key in
    // plaintext at `data/settings/providers.json`, so an unignored Cline env is a
    // credential one `git add -A` away from a public repo. A blueprint with no
    // git duties still gets the line.
    if let Some(project) = env_dir.parent() {
        crate::project::ensure_gitignore_entry(project, Agent::Cline.gitignore_pattern())?;
    }

    std::fs::create_dir_all(data_dir(env_dir).join("settings"))
        .context("could not create the Cline data dir")?;
    let rules = config_dir(env_dir).join("rules");
    std::fs::create_dir_all(&rules).context("could not create the Cline rules dir")?;

    // The instance file, so a placed env says what it is without consulting
    // config.toml — same contract as a Claude env's `.aello.toml`.
    let inst = toml::to_string_pretty(&crate::models::Instance {
        name: bp.name.clone(),
        model: bp.model.clone(),
    })?;
    std::fs::write(env_dir.join(".aello.toml"), inst)
        .context("could not write the Cline instance file")?;

    std::fs::write(rules.join("response-rules.md"), RESPONSE_RULES)
        .context("could not write the Cline response rules")?;
    Ok(())
}

/// Resolve the `cline` executable. Same shim problem as `claude`: an
/// npm-installed Cline is `cline.cmd`, which `Command::new("cline")` cannot see
/// on Windows, so a working install reports as missing.
fn cline_exe() -> std::ffi::OsString {
    #[cfg(windows)]
    {
        if let Ok(path) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path) {
                for ext in ["exe", "cmd", "bat"] {
                    let cand = dir.join(format!("cline.{ext}"));
                    if cand.is_file() {
                        return cand.into_os_string();
                    }
                }
            }
        }
    }
    std::ffi::OsString::from("cline")
}

/// Build the argument list for a launch. Pure, so the flag contract is testable
/// without spawning anything — the isolation flags are the whole point of a
/// Cline env, and a dropped one silently falls back to the shared `~/.cline`.
pub fn launch_args(
    env_dir: &Path,
    model: &str,
    provider: Option<&str>,
    resume: Option<&Option<String>>,
    prompt: Option<&str>,
    extra: &[String],
) -> Vec<String> {
    let mut args = vec![
        "--config".into(),
        config_dir(env_dir).to_string_lossy().into_owned(),
        "--data-dir".into(),
        data_dir(env_dir).to_string_lossy().into_owned(),
    ];
    if let Some(p) = provider {
        args.push("-P".into());
        args.push(p.to_string());
    }
    args.push("-m".into());
    args.push(model.to_string());
    // Cline resumes by session id only — there is no `--continue`, so "most
    // recent" is not expressible and is reported rather than silently ignored.
    if let Some(Some(id)) = resume {
        args.push("--id".into());
        args.push(id.clone());
    }
    if let Some(p) = prompt {
        args.push(p.to_string());
    }
    // User extras last, so any of them override what aello chose.
    args.extend_from_slice(extra);
    args
}

/// Spawn `cline` against an isolated env, inheriting the terminal.
pub fn launch(
    env_dir: &Path,
    name: &str,
    auth: Option<&ClineAuth>,
    model: &str,
    resume: Option<&Option<String>>,
    prompt: Option<&str>,
    extra: &[String],
) -> Result<i32> {
    let mut c = Command::new(cline_exe());
    c.args(launch_args(env_dir, model, auth.map(|a| a.provider.as_str()), resume, prompt, extra));

    // Per-env git attribution, identical to a Claude env: several blueprints
    // share one working tree and `git blame` has to say which one.
    let (git_name, git_email) = crate::launch::git_identity(name);
    c.env("GIT_AUTHOR_NAME", &git_name);
    c.env("GIT_AUTHOR_EMAIL", &git_email);
    c.env("GIT_COMMITTER_NAME", &git_name);
    c.env("GIT_COMMITTER_EMAIL", &git_email);

    // A Cline env must not inherit the *aello env's* Claude config dir. Cline's
    // `claude-code` provider reads `CLAUDE_CONFIG_DIR`, so an inherited one
    // points it at a Claude env whose credentials it cannot use.
    c.env_remove("CLAUDE_CONFIG_DIR");

    let status = c
        .status()
        .context("could not launch 'cline' — is the Cline CLI installed and on PATH? (npm i -g cline)")?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;

    fn auth() -> ClineAuth {
        ClineAuth {
            provider: "openrouter".into(),
            api_key: Some("sk-or-v1-secret".into()),
            model: "openai/gpt-5.6-luna-pro".into(),
            base_url: None,
        }
    }

    fn bp() -> Blueprint {
        Blueprint {
            name: "Runner".into(),
            model: "openai/gpt-5.6-luna-pro".into(),
            agent: Agent::Cline,
            claude_md: None,
            role: Role::Standalone,
            legacy_caps: None,
        }
    }

    /// `cline auth` is the only writer whose key survives, so these arguments
    /// are the credential path. Both isolation flags must ride along, or the
    /// key is installed into the shared `~/.cline` for every env at once.
    #[test]
    fn auth_args_carry_the_credential_and_stay_inside_the_env() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        let args = auth_args(&env, &auth());

        assert_eq!(args[0], "auth");
        let val = |flag: &str| {
            args.iter().position(|a| a == flag).map(|i| args[i + 1].clone())
        };
        assert_eq!(val("-p").as_deref(), Some("openrouter"));
        assert_eq!(val("-k").as_deref(), Some("sk-or-v1-secret"));
        assert_eq!(val("-m").as_deref(), Some("openai/gpt-5.6-luna-pro"));
        assert!(val("--config").unwrap().contains(".cline-env-Runner"));
        assert!(val("--data-dir").unwrap().contains(".cline-env-Runner"));
        // An unset base URL must not become an empty `-b`, which Cline would
        // take as a real override and fail to reach the provider.
        assert!(!args.iter().any(|a| a == "-b"), "{args:?}");
    }

    /// A provider that needs no key must not get an empty `-k`.
    #[test]
    fn auth_args_omit_the_key_when_there_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        let mut a = auth();
        a.api_key = None;
        let args = auth_args(&env, &a);
        assert!(!args.iter().any(|x| x == "-k"), "{args:?}");
    }

    /// Both isolation flags must be present on every launch. Dropping either one
    /// does not fail — Cline falls back to the shared `~/.cline`, so the env
    /// looks placed while the session, its history and its credential all land
    /// in the machine-wide tree with every other env's.
    #[test]
    fn every_launch_carries_both_isolation_flags() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        let args = launch_args(&env, "some-model", Some("openrouter"), None, None, &[]);

        let cfg = args.iter().position(|a| a == "--config").expect("no --config");
        let data = args.iter().position(|a| a == "--data-dir").expect("no --data-dir");
        assert!(args[cfg + 1].ends_with("config"), "{:?}", args[cfg + 1]);
        assert!(args[data + 1].ends_with("data"), "{:?}", args[data + 1]);
        // And both point inside this env, never at the shared tree.
        assert!(args[cfg + 1].contains(".cline-env-Runner"));
        assert!(args[data + 1].contains(".cline-env-Runner"));
    }

    /// User extras go last so a one-off `-- -m other-model` wins, matching how
    /// `aello run` behaves for a Claude env.
    #[test]
    fn user_extras_come_after_what_aello_chose() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        let extra = vec!["--thinking".to_string(), "high".to_string()];
        let args = launch_args(&env, "m", None, None, Some("do a thing"), &extra);
        let last_two = &args[args.len() - 2..];
        assert_eq!(last_two, &["--thinking".to_string(), "high".to_string()]);
    }

    /// The rules go in the **config** dir and the credential lands in the
    /// **data** dir — two different directories, both measured, and swapping
    /// them fails silently in both directions. Placement owns only the first;
    /// `cline auth` owns the second.
    #[test]
    fn place_writes_the_rules_where_cline_reads_them() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        place(&env, &bp()).unwrap();

        let rules = env.join("config").join("rules").join("response-rules.md");
        assert!(rules.exists(), "response rules not in the config dir");
        // The data/settings dir exists for `cline auth` to write into, but
        // placement must NOT hand-write providers.json there: Cline drops a
        // hand-written apiKey on its next run, which reads as a bad key.
        assert!(env.join("data").join("settings").is_dir());
        assert!(
            !env.join("data").join("settings").join("providers.json").exists(),
            "placement hand-wrote providers.json again — Cline strips the key from it"
        );

        // The rules file is the only channel carrying these — Cline's
        // UserPromptSubmit hook never fires, so a dropped rule is not recovered
        // anywhere else.
        // One anchor per rule, each chosen to identify that rule alone, so a
        // dropped rule fails here and a reworded one does not.
        let text = std::fs::read_to_string(&rules).unwrap();
        for (rule, phrase) in [
            ("concise", "Be concise"),
            ("no sycophancy", "praise or agreement"),
            ("no plans", "plan for approval"),
            ("closing block", "TL;DR:"),
        ] {
            assert!(text.contains(phrase), "the Cline rules lost the '{rule}' rule");
        }

        // Nothing Claude-shaped may appear in a Cline env: no CLAUDE.md, no
        // settings.json, no hooks. That mixing is what the split prevents.
        assert!(!env.join("CLAUDE.md").exists());
        assert!(!env.join("settings.json").exists());
        assert!(!env.join("hooks").exists());
    }

    /// The credential sits in plaintext inside the project directory, so the
    /// ignore line is a security property, not tidiness — and it must not depend
    /// on the role, since a standalone blueprint's key leaks just as well.
    #[test]
    fn placing_a_cline_env_always_ignores_it_even_with_no_git_duties() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        // Role::Standalone — the role that scaffolds nothing at all.
        place(&env, &bp()).unwrap();

        let ignored = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(ignored.contains(".cline-env-*"), "the env holding an API key is not ignored");

        // Idempotent: a second placement must not stack duplicate lines.
        place(&env, &bp()).unwrap();
        let again = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(again.matches(".cline-env-*").count(), 1, "{again}");
    }

}
