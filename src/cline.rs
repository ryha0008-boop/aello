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

/// Cline has **no memory system**, so aello builds one.
///
/// Confirmed against the binary: `memory-bank`, `memoryBank`, `cline_docs`,
/// `MEMORY.md` and `rememberThis` are all absent, and the single `auto-memory`
/// hit belongs to the Claude Code settings schema Cline embeds as a dependency.
/// Claude Code writes memory by itself; Cline will not unless told to, every
/// request.
///
/// That is the whole design: there is no hook to load a memory file on session
/// start (`TaskStart` fires but cannot inject), so the instruction has to live
/// somewhere re-sent on every request — which is the rules file. The agent reads
/// and writes the files itself with ordinary tool calls.
const MEMORY_RULE: &str = r#"# Memory

Cline has no built-in memory, so this is it. It is a directory of files, and
keeping it current is your job — nothing does it automatically.

- **At the start of a session**, read `MEMORY_DIR/MEMORY.md`. It is a one-line
  index; open the entries that look relevant to what you have been asked to do.
- **When you learn something durable** — a decision and why, a constraint, a
  preference, something that cost you time to find out — write it as its own
  file in `MEMORY_DIR/` and add a one-line pointer to `MEMORY.md`.
- Do not record what the repo already says. Code structure, git history and the
  project's own docs are not memory; the reasoning that is not recoverable from
  them is.
- Update or delete an entry that turns out to be wrong, rather than adding a
  correction beside it.
"#;

/// The standing block plus the command router.
///
/// **Cline has no user-defined slash commands** — measured. `/canary` with a
/// workflow and a skill installed in every candidate location was read as
/// ordinary prose, and the only slash commands the binary advertises are
/// connector built-ins (`/abort`, `/clear`, `/exit`, `/start`, `/whereami`).
/// Cline's own skills and workflows resolve under `<workspace>/.cline/<plugin>/`
/// as plugin artifacts, not as something a user types.
///
/// So the four commands are routed from here instead: the rules are re-sent in
/// the system prompt on every request, so a message that is exactly `/sync`
/// reliably means the same thing on turn one and turn eighty. The skill bodies
/// are ordinary files the agent opens with a tool call.
const AELLO_RULE: &str = r#"# You are running under aello

This is an **aello** environment named `NAME` — an isolated Cline setup whose
config dir is `ENV_DIR`.

- That directory is **gitignored and rewritten on every `aello run`**. Don't
  hand-edit its rules or skills; the next launch replaces them.
- Other blueprints may share this repo. The working tree is shared; config,
  memory and session history are not. Commits you make are attributed to `NAME`
  automatically.

## Commands the user may type

These are **the user's to type, never yours to run**. Working through a skill's
steps *is* running it, whatever route you took to its file. If you think one is
due, say so and let them ask.

When the user's message **starts with** one of these, open the file named beside
it and follow it exactly. Ignore any words after the command — they are there
because Cline refuses a one-word prompt, not because they change the task:

- `/sync` → `SKILLS_DIR/sync/SKILL.md`
- `/handoff` → `SKILLS_DIR/handoff/SKILL.md`
- `/note <blueprint>` → `SKILLS_DIR/note/SKILL.md`
- `/twosentences` → `SKILLS_DIR/twosentences/SKILL.md`
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

/// The four skills, as `(dir, body)`. Same four a Claude env gets, rewritten
/// for what Cline actually has.
///
/// `/sync` is the one that differs in substance: **no git section**. A Claude
/// `/sync` commits and pushes; this one reconciles the project's `AGENTS.md`
/// (Cline's `CLAUDE.md` — measured: Cline ignores `CLAUDE.md` entirely) and the
/// memory directory, and stops there. Flip that by adding a git section here if
/// you want Cline blueprints committing too.
fn skills(name: &str, env_dir: &Path) -> Vec<(&'static str, String)> {
    let mem = slash(&memory_dir(env_dir));
    vec![
        (
            "sync",
            format!(
                r#"# /sync — reconcile the docs and the memory

**Only the user runs this.** If you are reading this file because you decided
to, stop. Following these steps *is* running the skill.

Work through these in order:

1. **Memory first.** Read `{mem}/MEMORY.md` and the entries it points at.
   Anything you learned this session that is durable and not already there gets
   written as its own file, with a one-line pointer added to `MEMORY.md`.
   Anything there that has turned out to be wrong gets corrected or deleted.
2. **The project's `AGENTS.md`.** This is Cline's equivalent of a project
   `CLAUDE.md` — it is read from the project root on every session. Bring it
   back in line with what the code now does: architecture that moved, a
   convention that changed, a decision whose reasoning is not recoverable from
   the diff. Do not restate what the code already says plainly.
3. **Report** what you changed in each, in a few lines. If nothing needed
   changing in one of them, say that rather than inventing an edit.

There is no git step here, by design — committing is yours to do."#
            ),
        ),
        (
            "handoff",
            format!(
                r#"# /handoff — write a note for your next session

**Only the user runs this.** If you are reading this file because you decided
to, stop.

Cline sessions do not carry context forward, so write `{name}.HANDOFF.md` at the
**project root** with everything the next session needs and nothing it can get
from the repo itself:

- **Read first** — the two or three files that matter, by path.
- **What shipped** — what is done, and any commit shas.
- **Open threads** — what is half-finished, and what you would do next.
- **Gotchas** — what looks wrong but is not, and what you ruled out.

Assume the next session starts blank. Nothing here should require having been
in this one. Delete the file yourself once you have read it on the next boot —
nothing does that automatically."#
            ),
        ),
        (
            "note",
            format!(
                r#"# /note — leave a note for another environment

**Only the user runs this.** If you are reading this file because you decided
to, stop.

The argument is another blueprint's name (`/note frontend`). You are `{name}`.

1. Use the target's **canonical casing** — the filename is matched exactly.
2. **Work out the target's project root; it is not always this repo.** Look for
   a `.cline-env-<target>/` or `.claude-env-<target>/` here first. If it is not
   here, that environment lives in another repo and the note belongs at *that*
   repo's root — a note left here would never be read. Ask for the path rather
   than guessing, and do not treat a target you cannot find as a typo.
3. **Overwrite** `<target>.NOTE.md` at that project root. Only the latest note
   matters; do not append.
4. Write: a one-line banner naming you as the sender, then **what I was doing**,
   **the problem**, and **what you need to fix** — naming files and paths.

Tell the user the full path you wrote to."#
            ),
        ),
        (
            "twosentences",
            r#"# /twosentences

**Only the user runs this.** If you are reading this file because you decided
to, stop.

Condense your previous response into exactly two sentences. Output the two
sentences and nothing else — no preamble, no heading, no closing block."#
                .to_string(),
        ),
    ]
}

/// Paths written into rules and skills use forward slashes, on every platform.
///
/// Two reasons, and the first one bit: the templates join with `/` while
/// `Path::display` on Windows yields `\`, so a substituted path came out as
/// `C:\…\skills/sync/SKILL.md` — which works, but means nothing downstream can
/// match it and no test can check it. The second is that a Windows path inside
/// a markdown file is a string full of backslash escapes for whoever reads it.
/// Windows accepts forward slashes in every file API.
fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn memory_dir(env_dir: &Path) -> PathBuf {
    env_dir.join("memory")
}

fn skills_dir(env_dir: &Path) -> PathBuf {
    env_dir.join("skills")
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
/// `persona` is the resolved persona text (the same bundled templates a Claude
/// env uses) or `None` for `none`/`custom` — written as a rules file, since
/// Cline ignores `CLAUDE.md`.
///
/// The credential is **not** written here — see [`ensure_credential`], which
/// shells out to `cline auth` because that is the only writer whose key
/// survives Cline's next run.
pub fn place(env_dir: &Path, bp: &Blueprint, persona: Option<&str>) -> Result<()> {
    // Unconditional. This env holds an API key in plaintext at
    // `data/settings/providers.json`, so an unignored one is a credential a
    // single `git add -A` away from a public repo, and a blueprint with no git
    // duties leaks it exactly as well as a maintainer would.
    //
    // This comment used to add "and NOT role-gated the way the Claude line is,
    // because a Claude env holds no secret — auth arrives as an environment
    // variable at launch". That reasoning was quoted back at us by an audit
    // along with the line that disproves it: with no shared token configured,
    // Claude Code writes its own `.credentials.json` into the env dir, and
    // `main.rs` probes for exactly that file. Both lines are unconditional now.
    // Left on the record because a stated rationale that has quietly become
    // false is worse than none — everyone after it inherits it as settled.
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
        // A Cline env writes no mirror at all, so there is no destination to carry.
        mirror_root: None,
    })?;
    std::fs::write(env_dir.join(".aello.toml"), inst)
        .context("could not write the Cline instance file")?;

    std::fs::write(rules.join("response-rules.md"), RESPONSE_RULES)
        .context("could not write the Cline response rules")?;

    // The persona, as a rule rather than a CLAUDE.md — Cline ignores that file
    // entirely, so the same bundled templates are simply written somewhere it
    // reads. `custom` and `none` resolve to nothing, exactly as they do for a
    // Claude env, and a persona the user has written into the env is left alone.
    let persona_path = rules.join("persona.md");
    match persona {
        Some(text) => std::fs::write(&persona_path, text)
            .context("could not write the Cline persona")?,
        // Absent means "aello writes none" — but a stale one from a previous
        // persona choice must not linger, since these rules are re-sent on
        // every request and nothing else would ever remove it.
        //
        // `custom` is the exception, and the comment above claimed it while the
        // code did the opposite: `resolve` returns None for both `none` and
        // `custom`, so a Cline blueprint set to `custom` had the persona the
        // user wrote deleted on every single run. `custom` means "the env's copy
        // is authoritative", which is the one case where absence is not a clear.
        None if persona_path.exists()
            && bp.claude_md.as_deref() != Some(crate::templates::CUSTOM) =>
        {
            std::fs::remove_file(&persona_path).context("could not clear the Cline persona")?
        }
        None => {}
    }

    // The standing block and the command router, with the paths resolved: a
    // relative path would be read against the *workspace*, not the env.
    let skills_root = skills_dir(env_dir);
    let aello_rule = AELLO_RULE
        .replace("NAME", &bp.name)
        .replace("ENV_DIR", &slash(env_dir))
        .replace("SKILLS_DIR", &slash(&skills_root));
    std::fs::write(rules.join("aello.md"), aello_rule)
        .context("could not write the Cline standing rules")?;
    std::fs::write(
        rules.join("memory.md"),
        MEMORY_RULE.replace("MEMORY_DIR", &slash(&memory_dir(env_dir))),
    )
    .context("could not write the Cline memory rule")?;

    // Skills are regenerated every run, like a Claude env's, so a wording fix
    // reaches an env placed months ago.
    for (dir, body) in skills(&bp.name, env_dir) {
        let d = skills_root.join(dir);
        std::fs::create_dir_all(&d).context("could not create a Cline skill dir")?;
        std::fs::write(d.join("SKILL.md"), body).context("could not write a Cline skill")?;
    }

    // Memory is seeded once and then belongs to the agent — clobbering it every
    // run is the one thing that would make it worthless.
    let mem = memory_dir(env_dir);
    std::fs::create_dir_all(&mem).context("could not create the Cline memory dir")?;
    let index = mem.join("MEMORY.md");
    if !index.exists() {
        std::fs::write(&index, MEMORY_INDEX).context("could not seed the Cline memory index")?;
    }
    Ok(())
}

/// Seeded once, never rewritten. An empty index that says what it is beats an
/// absent file the agent has to guess the shape of.
const MEMORY_INDEX: &str = "# Memory index\n\n\
    One line per entry: `- [title](file.md) — what it tells you`.\n\
    Nothing here yet.\n";

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

/// Cline rejects a **single-word** prompt, whatever it is.
///
/// `-p "hi"` comes back with "Unknown command or unquoted prompt: hi", which
/// reads like a quoting mistake and is not one — the argument arrives perfectly
/// quoted and Cline still refuses it.
///
/// **This includes `/sync` and the other three**, which is the part that matters
/// and the part that nearly shipped wrong. An early test appeared to show
/// `-p "/twosentences"` working; it had in fact been rewritten by Git Bash into
/// `C:/Program Files/Git/twosentences`, which contains a space, so Cline
/// accepted it and the model *guessed* the skill from the path. Measured
/// directly afterwards: `hi`, `/sync` and `/twosentences` are all refused.
///
/// So in one-shot mode a command needs a trailing word (`-p "/sync now"`), which
/// is why the router matches a *prefix* rather than an exact message. The
/// interactive TUI has a real `/` menu and is not affected — unverified here,
/// since it needs a TTY.
pub fn single_word_prompt(prompt: Option<&str>) -> bool {
    prompt.is_some_and(|p| !p.trim().is_empty() && !p.trim().contains(char::is_whitespace))
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
    // The same reasoning one step further: that provider also reads the Claude
    // credentials themselves, and this env is billed against its own key.
    crate::launch::scrub_inherited_credentials(&mut c);

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
            mirror_root: None,
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
        place(&env, &bp(), None).unwrap();

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

    /// All four skills, and the router that is the only way to reach them.
    ///
    /// Cline has **no user-defined slash commands** — a skill file nothing
    /// points at is a file nothing will ever open, and that failure is
    /// completely silent: the user types `/sync`, the agent reads it as prose
    /// and answers something plausible.
    #[test]
    fn every_skill_is_seeded_and_named_by_the_router() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        place(&env, &bp(), None).unwrap();

        let router = std::fs::read_to_string(env.join("config").join("rules").join("aello.md"))
            .expect("no router rule written");
        for skill in ["sync", "handoff", "note", "twosentences"] {
            let path = env.join("skills").join(skill).join("SKILL.md");
            assert!(path.exists(), "skill '{skill}' was not seeded");
            assert!(
                router.contains(&format!("/{skill}")),
                "skill '{skill}' exists but the router never mentions it — nothing can reach it"
            );
            // The router must name a resolvable absolute path: a relative one
            // resolves against the *workspace*, not the env dir.
            assert!(
                router.contains(&slash(path.parent().unwrap())),
                "the router points somewhere other than the seeded '{skill}'"
            );
        }
        // The router must match a PREFIX. Cline refuses one-word prompts, so a
        // command always arrives with a trailing word, and an exact-match rule
        // would never fire in one-shot mode.
        assert!(
            router.contains("starts with"),
            "the router demands an exact message, which no one-shot prompt can be"
        );

        // Every skill says it is the user's to run, like a Claude env's do.
        for skill in ["sync", "handoff", "note", "twosentences"] {
            let body =
                std::fs::read_to_string(env.join("skills").join(skill).join("SKILL.md")).unwrap();
            assert!(body.contains("Only the user runs this"), "'{skill}' lost its banner");
        }
    }

    /// Cline has no memory of its own, so the rule telling the agent to keep one
    /// is the entire mechanism. Seeded once and then left alone — rewriting the
    /// index every run is the one thing that would make it worthless.
    #[test]
    fn memory_is_seeded_once_and_never_clobbered() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        place(&env, &bp(), None).unwrap();

        let index = env.join("memory").join("MEMORY.md");
        assert!(index.exists());
        let rule = std::fs::read_to_string(env.join("config").join("rules").join("memory.md"))
            .expect("no memory rule");
        assert!(
            rule.contains(&slash(&env.join("memory"))),
            "the memory rule does not name the memory dir, so nothing finds it"
        );

        std::fs::write(&index, "- [a thing](a.md) — learned the hard way\n").unwrap();
        place(&env, &bp(), None).unwrap();
        assert_eq!(
            std::fs::read_to_string(&index).unwrap(),
            "- [a thing](a.md) — learned the hard way\n",
            "placement overwrote the agent's memory"
        );
    }

    /// A persona is a rules file here, because Cline ignores `CLAUDE.md`. The
    /// removal branch matters: rules are re-sent every request, so a persona
    /// left behind after switching to `none` would keep applying forever with
    /// nothing to remove it.
    #[test]
    fn the_persona_is_written_as_a_rule_and_cleared_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        let persona = env.join("config").join("rules").join("persona.md");

        place(&env, &bp(), Some("# You are a careful engineer\n")).unwrap();
        assert!(std::fs::read_to_string(&persona).unwrap().contains("careful engineer"));

        place(&env, &bp(), None).unwrap();
        assert!(!persona.exists(), "a dropped persona kept applying on every request");
    }

    /// `custom` resolves to no text for the same reason `none` does — the env's
    /// copy is authoritative — but it means the opposite. Clearing on both left
    /// a Cline blueprint set to `custom` with no persona at all, rewritten away
    /// on every run.
    #[test]
    fn a_custom_persona_is_left_alone_rather_than_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        let persona = env.join("config").join("rules").join("persona.md");

        place(&env, &bp(), Some("# seeded\n")).unwrap();
        std::fs::write(&persona, "# the persona the user accepted\n").unwrap();

        let mut custom = bp();
        custom.claude_md = Some(crate::templates::CUSTOM.to_string());
        place(&env, &custom, None).unwrap();

        assert_eq!(
            std::fs::read_to_string(&persona).unwrap(),
            "# the persona the user accepted\n",
            "a custom persona was cleared as if it were `none`"
        );
    }

    /// Multi-word prompts pass, one-word prompts don't, and an empty/absent one
    /// is not a prompt at all. Every earlier test used a sentence, which is
    /// exactly why this quirk went unnoticed until a real `-p "hi"`.
    #[test]
    fn a_one_word_prompt_is_caught_before_cline_refuses_it() {
        assert!(single_word_prompt(Some("hi")));
        // The four commands are one-word too, and Cline refuses them just the
        // same — measured. An early result suggesting otherwise was Git Bash
        // rewriting the argument into a path containing a space.
        assert!(single_word_prompt(Some("/sync")));
        assert!(single_word_prompt(Some("/twosentences")));
        // Which is why the documented workaround is a trailing word.
        assert!(!single_word_prompt(Some("/sync now")));
        assert!(single_word_prompt(Some("  hi  ")));
        assert!(!single_word_prompt(Some("say hello")));
        assert!(!single_word_prompt(Some("")));
        assert!(!single_word_prompt(None));
    }

    /// The credential sits in plaintext inside the project directory, so the
    /// ignore line is a security property, not tidiness — and it must not depend
    /// on the role, since a standalone blueprint's key leaks just as well.
    #[test]
    fn placing_a_cline_env_always_ignores_it_even_with_no_git_duties() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_dir(dir.path(), "Runner");
        // Role::Standalone — the role that scaffolds nothing at all.
        place(&env, &bp(), None).unwrap();

        let ignored = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(ignored.contains(".cline-env-*"), "the env holding an API key is not ignored");

        // Idempotent: a second placement must not stack duplicate lines.
        place(&env, &bp(), None).unwrap();
        let again = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(again.matches(".cline-env-*").count(), 1, "{again}");
    }

}
