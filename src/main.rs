use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

mod auth;
mod cline;
mod config;
mod docs;
mod github;
mod launch;
mod models;
mod project;
mod sessions;
mod templates;
mod tui;
mod update;
mod voice;

use models::{Agent, Blueprint, Instance, Role};

/// Isolated Claude Code environments — like venvs, but for AI agents.
#[derive(Parser)]
#[command(name = "aello", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a blueprint (a named AI identity).
    Add {
        name: String,
        /// Model. For claude: sonnet, opus, haiku. For cline: whatever the
        /// provider calls it, e.g. openai/gpt-5.6-luna-pro.
        #[arg(long)]
        model: String,
        /// Which CLI this blueprint drives: claude (default) or cline. Fixed at
        /// add time — the two share nothing on disk.
        #[arg(long, value_enum, default_value = "claude")]
        agent: Agent,
        /// Global persona: coder (a coding project), none (anything else) or a
        /// path to a CLAUDE.md file. Becomes `custom` once a persona has been
        /// generated for the env. Placed into the env dir on first run.
        #[arg(long)]
        claude_md: Option<String>,
        /// What this blueprint is responsible for: maintainer (owns CLAUDE.md,
        /// CHANGELOG, docs/, README + git), contributor (commits, pushes and
        /// logs its own change), or standalone (no /sync).
        #[arg(long, value_enum, default_value = "standalone")]
        role: Role,
    },
    /// List all blueprints.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove a blueprint by name.
    Remove {
        name: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
        /// Also delete the placed env dir and its `claude-internal/<name>/`
        /// mirror in the current project.
        #[arg(long)]
        purge: bool,
    },
    /// Edit an existing blueprint's model, persona, or role.
    Edit(EditArgs),
    /// Place a blueprint in the current directory and launch it.
    Run {
        /// Blueprint name (optional if you have exactly one).
        name: Option<String>,
        /// Resume the most recent session, or a specific session id.
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        resume: Option<Option<String>>,
        /// Run a single prompt headless and exit.
        #[arg(short = 'p', long)]
        prompt: Option<String>,
        /// Extra args passed straight to claude (after `--`).
        #[arg(last = true)]
        extra: Vec<String>,
    },
    /// First-run setup: log in (if needed) and create your first blueprint.
    Init,
    /// Store a shared login. Asks which agent unless `--agent` says.
    Login {
        /// claude (runs `claude setup-token`) or cline (a provider key).
        #[arg(long, value_enum)]
        agent: Option<Agent>,
    },
    /// Create a GitHub repo for the current project and push (needs `gh`).
    GithubSetup {
        /// Repo name (default: current directory name).
        #[arg(long)]
        name: Option<String>,
        /// Create a public repo (default: private).
        #[arg(long)]
        public: bool,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Update aello to the latest build from GitHub.
    Update {
        /// Reinstall even when already on the published version.
        #[arg(long)]
        force: bool,
    },
    /// Print a shell completion script (bash, zsh, fish, powershell, elvish).
    Completions {
        /// Shell to generate completions for.
        shell: clap_complete::Shell,
    },
    /// Show bundled reference docs (no name lists them).
    Docs {
        /// Doc to print (slug, e.g. `concepts`). Omit to list available docs.
        name: Option<String>,
    },
    /// Accept a generated global persona into a placed env.
    ///
    /// Replaces that env's CLAUDE.md, flips the blueprint's persona to
    /// `custom` so aello stops writing one, and bumps the generation recorded
    /// in `<env>/persona.gen`. This is the only command that overwrites a
    /// persona — `run` never does.
    Persona {
        /// Blueprint whose env receives the persona.
        name: String,
        /// File holding the new CLAUDE.md.
        #[arg(long)]
        from: PathBuf,
        /// Project holding the env dir (default: current directory).
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Mute or unmute the voice (applies to every env).
    Voice {
        #[command(subcommand)]
        action: VoiceAction,
    },
    // More subcommands land here in later phases (sessions, ...).
}

/// Flags for `aello edit`. Every flag is optional; omitting one leaves that
/// field unchanged. Changes take effect on the next `aello run` (the global
/// persona is never re-clobbered).
#[derive(clap::Args)]
struct EditArgs {
    /// Blueprint to edit.
    name: String,
    /// Rename the blueprint (also moves the placed env dir + mirror here).
    #[arg(long)]
    rename: Option<String>,
    /// New model (alias like opus/sonnet/haiku or a full claude-* id).
    #[arg(long)]
    model: Option<String>,
    /// New global persona (built-in name or path to a CLAUDE.md file).
    #[arg(long)]
    claude_md: Option<String>,
    /// New role: maintainer, contributor, or standalone.
    #[arg(long, value_enum)]
    role: Option<Role>,
}

/// Off switch for the voice. State is machine-wide, so these apply
/// to every env at once and work from any directory.
#[derive(Subcommand)]
enum VoiceAction {
    /// Silence the voice hook.
    Mute {
        /// Only this project, instead of everywhere.
        #[arg(long)]
        project: bool,
    },
    /// Let it speak again.
    Unmute {
        /// Only this project, instead of everywhere.
        #[arg(long)]
        project: bool,
    },
    /// Stop the sentence playing right now, without changing any mute.
    Stop,
    /// Show mute state and the voice pool.
    Status,
}

fn main() {
    // Windows leaves the previous binary as aello.exe.old-<n> after a
    // self-update; sweep up any such leftovers on launch (locked ones, from a
    // still-running old instance, are skipped silently).
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        if let (Some(dir), Some(name)) =
            (exe.parent(), exe.file_name().and_then(|n| n.to_str()))
        {
            let prefix = format!("{name}.old");
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    if e.file_name().to_str().is_some_and(|f| f.starts_with(&prefix)) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
    }

    let cli = Cli::parse();
    let result = match cli.command {
        None => tui::run(),
        Some(Commands::Add { name, model, agent, claude_md, role }) => {
            cmd_add(name, model, agent, claude_md, role)
        }
        Some(Commands::List { json }) => cmd_list(json),
        Some(Commands::Remove { name, yes, purge }) => cmd_remove(name, yes, purge),
        Some(Commands::Edit(args)) => cmd_edit(args),
        Some(Commands::Run { name, resume, prompt, extra }) => cmd_run(name, resume, prompt, extra),
        Some(Commands::Init) => cmd_init(),
        Some(Commands::Login { agent }) => cmd_login(agent),
        Some(Commands::GithubSetup { name, public, yes }) => github::run(name, public, yes),
        Some(Commands::Update { force }) => update::run(force),
        Some(Commands::Completions { shell }) => cmd_completions(shell),
        Some(Commands::Docs { name }) => cmd_docs(name),
        Some(Commands::Persona { name, from, project }) => cmd_persona(name, from, project),
        Some(Commands::Voice { action }) => match action {
            VoiceAction::Mute { project } => voice::mute(project),
            VoiceAction::Unmute { project } => voice::unmute(project),
            VoiceAction::Stop => voice::stop(),
            VoiceAction::Status => voice::status(),
        },
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

/// Blueprint names map to env-dir names, so keep them filesystem-safe.
pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("name cannot be empty");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        bail!("name '{name}' must contain only ASCII letters, digits, '-' or '_'");
    }
    // Bounded so an over-long name fails here with a clear message rather than
    // deep inside create_dir_all with a raw OS error. `.claude-env-<name>` and
    // `claude-internal/<name>/` both nest under the project path, so the real
    // ceiling is well below any filesystem limit anyway.
    const MAX_NAME: usize = 64;
    if name.len() > MAX_NAME {
        bail!("name is {} characters — keep it to {MAX_NAME} or fewer", name.len());
    }
    // The github cap creates a BARE `claude-internal/<name>/` component, so a
    // Windows reserved device name (CON, NUL, COM1…) would make create_dir_all
    // fail with an opaque OS error at the mirror step. Reject them up front,
    // case-insensitively (Windows matches these regardless of case).
    if is_reserved_device_name(name) {
        bail!("name '{name}' is a reserved device name on Windows — pick another");
    }
    Ok(())
}

/// Windows reserved device names (case-insensitive): CON, PRN, AUX, NUL,
/// COM1–COM9, LPT1–LPT9. Bare filesystem components with these names are
/// refused by Windows even on other platforms' shared repos.
fn is_reserved_device_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes().get(3), Some(b'1'..=b'9'))
            && upper.len() == 4)
}

/// Short aliases Claude Code accepts in settings.json "model".
const MODEL_ALIASES: &[&str] = &["opus", "sonnet", "haiku", "default"];

/// Reject typo'd models before they reach settings.json. Accept a known alias
/// (case-insensitive) or any full `claude-*` model id (forward-compatible with
/// new releases without an exact-version allowlist).
///
/// Returns the **normalised** value — trimmed and lowercased — because that is
/// what was actually validated. Callers used to validate a normalised copy and
/// then store the raw string, so `--model " opus "` passed the check and
/// reached settings.json verbatim.
pub(crate) fn validate_model(model: &str) -> Result<String> {
    let m = model.trim().to_lowercase();
    if m.is_empty() {
        bail!("model cannot be empty");
    }
    if MODEL_ALIASES.contains(&m.as_str()) || m.strip_prefix("claude-").is_some_and(|r| !r.is_empty()) {
        return Ok(m);
    }
    bail!(
        "unknown model '{model}'. Use an alias ({}) or a full model id like claude-opus-4-8",
        MODEL_ALIASES.join(", ")
    );
}

fn cmd_add(
    name: String,
    model: String,
    agent: Agent,
    claude_md: Option<String>,
    role: Role,
) -> Result<()> {
    validate_name(&name)?;
    // Only Claude's model names are a known set. Cline's are provider-scoped
    // (`openai/gpt-5.6-luna-pro`, `qwen/qwen3.8-max`), there is no list to check
    // against, and rejecting an unfamiliar one would block every provider aello
    // has not heard of.
    let model = match agent {
        Agent::Claude => validate_model(&model)?,
        Agent::Cline => model,
    };
    // Catch a typo'd built-in / missing template path at add time, not first run.
    if let Some(cm) = &claude_md {
        // Applies to both agents. Cline ignores `CLAUDE.md` as a *file*, but the
        // bundled persona templates are just text — a Cline env gets the same
        // one written into its rules dir, where Cline does read it.
        templates::resolve(cm)?;
    }
    let mut cfg = config::load()?;
    if let Some(existing) = cfg.find_name_conflict(&name) {
        if existing == name {
            bail!("blueprint '{name}' already exists");
        }
        bail!(
            "blueprint name '{name}' collides with existing '{existing}' — names are \
             case-insensitive on Windows/macOS filesystems and would share one env dir"
        );
    }
    let needs_login = agent == Agent::Cline && cfg.cline.is_none();
    cfg.blueprints.push(Blueprint {
        name: name.clone(),
        model,
        agent,
        claude_md,
        role,
        legacy_caps: None,
    });
    config::save(&cfg)?;
    println!("Added blueprint '{name}' ({}, {}).", agent.as_str(), role.as_str());
    if needs_login {
        println!("No Cline login yet — run `aello login --agent cline` before running it.");
    }
    Ok(())
}

fn cmd_remove(name: String, yes: bool, purge: bool) -> Result<()> {
    let mut cfg = config::load()?;
    if cfg.find(&name).is_none() {
        bail!("no blueprint named '{name}'");
    }
    // `--purge` deletes `.claude-env-<name>` and `claude-internal/<name>` derived
    // from the name; re-gate a hand-edited config name so a traversal like
    // `../../x` can't direct the delete outside the project.
    if purge {
        validate_name(&name)
            .with_context(|| format!("blueprint name in config.toml is invalid: '{name}'"))?;
    }

    // On-disk artifacts for the CURRENT project (the only ones we can locate).
    let project = std::env::current_dir().context("could not determine current directory")?;
    let env = project::env_dir(&project, &name);
    let mirror = project.join("claude-internal").join(&name);

    if !yes {
        let action = if purge {
            format!("Remove blueprint '{name}' and delete its env dir + mirror in this project?")
        } else {
            format!("Remove blueprint '{name}'?")
        };
        if !confirm(&action) {
            println!("Cancelled.");
            return Ok(());
        }
    }

    cfg.blueprints.retain(|b| b.name != name);
    config::save(&cfg)?;
    println!("Removed blueprint '{name}'.");

    if purge {
        for dir in [&env, &mirror] {
            if dir.exists() {
                std::fs::remove_dir_all(dir)
                    .with_context(|| format!("could not delete {}", dir.display()))?;
                println!("Deleted {}", dir.display());
            }
        }
    } else if env.exists() {
        println!(
            "Note: {} remains on disk (pass --purge to delete it).",
            env.display()
        );
    }
    Ok(())
}

/// Yes/No prompt on stdin, defaulting to No — returns false on a closed or
/// unreadable stdin so a non-interactive run without `--yes` safely aborts.
fn confirm(question: &str) -> bool {
    use std::io::Write;
    print!("{question} [y/N] ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

fn cmd_edit(args: EditArgs) -> Result<()> {
    // Re-gate the name on the read path, the way cmd_remove and run_blueprint
    // already do. The name reaches the filesystem as a bare `.claude-env-<name>`
    // component, and a hand-edited config.toml is the one way an unvalidated one
    // gets in — defence in depth, not a live exploit.
    validate_name(&args.name)?;
    let mut cfg = config::load()?;
    let Some(idx) = cfg.blueprints.iter().position(|b| b.name == args.name) else {
        bail!("no blueprint named '{}'", args.name);
    };

    // Validate a rename against the whole registry before the mutable borrow.
    if let Some(new) = &args.rename {
        validate_name(new)?;
        if *new == args.name {
            bail!("--rename '{new}' is already the blueprint's name");
        }
        // Reject a case-insensitive collision with a *different* blueprint (they
        // would share one env dir on Windows/macOS). A pure case-flip of this
        // blueprint's own name is left to rename_placed, which handles the
        // on-disk case rename.
        if let Some(existing) = cfg.find_name_conflict(new) {
            if !existing.eq_ignore_ascii_case(&args.name) {
                bail!(
                    "blueprint name '{new}' collides with existing '{existing}' — names are \
                     case-insensitive on Windows/macOS filesystems"
                );
            }
        }
    }

    let bp = &mut cfg.blueprints[idx];
    let mut changed = false;

    if let Some(model) = args.model {
        bp.model = validate_model(&model)?;
        changed = true;
    }
    if let Some(cm) = args.claude_md {
        templates::resolve(&cm)?; // reject a typo'd built-in / missing path now
        bp.claude_md = Some(cm);
        changed = true;
    }

    if let Some(role) = args.role {
        changed |= bp.role != role;
        bp.role = role;
    }

    // Rename last: move on-disk artifacts for this project, then the config name.
    if let Some(new) = args.rename {
        let project = std::env::current_dir().context("could not determine current directory")?;
        let moved = project::rename_placed(&project, &args.name, &new)?;
        bp.name = new.clone();
        changed = true;
        if moved {
            println!("Renamed the placed env dir + mirror in this project to '{new}'.");
        }
        // A rename only touches THIS project. There is no placement registry —
        // Config holds blueprints, contextdb and the token, nothing about where a
        // blueprint has been placed — so aello cannot find the others. Say so,
        // because `run <new>` in one of them silently scaffolds a fresh env
        // beside the old one rather than failing.
        println!(
            "Note: only this project was updated. If '{}' is placed elsewhere, run 
             `aello edit {new} --rename {new}` in each of those directories to move 
             its env dir too — until then they keep the old name on disk.",
            args.name
        );
    }

    if !changed {
        bail!("nothing to change — pass --rename, --model, --claude-md, or --role");
    }

    let name = bp.name.clone();
    config::save(&cfg)?;
    println!("Updated blueprint '{name}'. Changes apply on the next `aello run {name}`.");
    Ok(())
}

fn cmd_run(
    name: Option<String>,
    resume: Option<Option<String>>,
    prompt: Option<String>,
    extra: Vec<String>,
) -> Result<()> {
    // Only consulted to resolve a bare `aello run`; run_blueprint re-loads and is
    // the one that validates, so an existence check here would be a second read
    // of the same file for no gain.
    let cfg = config::load()?;
    let bp_name = match name {
        Some(n) => n,
        None => match cfg.blueprints.as_slice() {
            [one] => one.name.clone(),
            [] => bail!("no blueprints — add one with: aello add <name> --model <model>"),
            _ => bail!("multiple blueprints — specify one: aello run <name>"),
        },
    };
    let code = run_blueprint(&bp_name, resume, prompt.as_deref(), &extra)?;
    std::process::exit(code);
}

/// Place a named blueprint into the current dir and launch Claude. Returns the
/// child exit code. Shared by the CLI `run` command and the TUI's Enter action.
pub(crate) fn run_blueprint(
    name: &str,
    resume: Option<Option<String>>,
    prompt: Option<&str>,
    extra: &[String],
) -> Result<i32> {
    let cfg = config::load()?;
    let bp = cfg.find(name).with_context(|| format!("no blueprint named '{name}'"))?;
    // config.toml is just a file: a hand-edited name like `../../evil` never
    // passed validate_name on write, but every read path interpolates the raw
    // name into `.claude-env-<name>` / `claude-internal/<name>` fs paths. Re-gate
    // it here (the shared placement sink) so a poisoned name can't escape the
    // project dir or inject into generated SKILL.md frontmatter.
    validate_name(&bp.name)
        .with_context(|| format!("blueprint name in config.toml is invalid: '{}'", bp.name))?;

    let project = std::env::current_dir().context("could not determine current directory")?;

    // The two agents diverge completely from here — different env dir, different
    // placement, different launch mechanism. Everything Cline is in `cline.rs`;
    // the rest of this function is Claude's and stays that way.
    if bp.agent == Agent::Cline {
        return run_cline(&cfg, bp, &project, resume, prompt, extra);
    }

    let env = project::env_dir(&project, &bp.name);
    let inst = Instance { name: bp.name.clone(), model: bp.model.clone() };

    // Resolve the global persona: a built-in template name or a file path.
    // Fail rather than warn. `add` and `edit` both reject an unresolvable
    // persona outright, and a warning here went to stderr moments before Claude's
    // alternate screen wiped it — so the one case that matters, a persona file
    // moved or deleted after the blueprint was created, launched a silently
    // persona-less agent that looked fine.
    let claude_md = match &bp.claude_md {
        Some(spec) => templates::resolve(spec).with_context(|| {
            format!("blueprint '{name}' has an unusable persona — fix it with: aello edit {name} --claude-md <coder|none|custom|path>")
        })?,
        None => None,
    };

    project::place(&env, &inst, claude_md.as_deref(), &bp.caps())?;

    // The env now has notify.py; make sure the machine can actually show what it
    // sends. Claims the toast identity only — the protocol stays with a copy that
    // has an action.py to serve it.
    voice::ensure_notify_registered(&env);

    // Concurrency-safe shared login: pass the long-lived OAuth token to the env.
    // No token configured → Claude prompts its own login in this env.
    if cfg.oauth_token.is_some() {
        // Token handles auth; skip Claude's interactive first-run wizard.
        let _ = project::mark_onboarded(&env);
    } else if !env.join(".credentials.json").exists() {
        println!("Launching '{}' — no shared token (run `aello login`); Claude will prompt login.", bp.name);
    }

    // `--resume` with no value means "continue most recent".
    let resume = match resume {
        Some(Some(s)) if s.is_empty() => Some(None),
        other => other,
    };
    let contextdb = config::contextdb_dir(&cfg);
    launch::launch(&env, &bp.name, resume.as_ref(), prompt, extra, &contextdb, cfg.oauth_token.as_deref())
}

/// Place and launch a Cline blueprint.
///
/// Much shorter than the Claude path because a Cline env is much smaller: no
/// persona to resolve (Cline ignores `CLAUDE.md`), no hooks worth registering,
/// no voice, no contextdb. Isolation, a credential, and the response rules.
fn run_cline(
    cfg: &models::Config,
    bp: &Blueprint,
    project: &std::path::Path,
    resume: Option<Option<String>>,
    prompt: Option<&str>,
    extra: &[String],
) -> Result<i32> {
    let env = cline::env_dir(project, &bp.name);
    // Same persona resolution as a Claude env — the bundled templates are just
    // written somewhere Cline reads. Fail rather than warn, for the same reason:
    // a persona file moved after the blueprint was made would otherwise launch a
    // silently persona-less agent.
    let persona = match &bp.claude_md {
        Some(spec) => templates::resolve(spec).with_context(|| {
            format!("blueprint '{}' has an unusable persona — fix it with: aello edit {} --claude-md <coder|none|custom|path>", bp.name, bp.name)
        })?,
        None => None,
    };
    cline::place(&env, bp, persona.as_deref())?;

    // A Cline env is billed per token, so a missing credential is a hard stop
    // rather than a warning: Cline would otherwise fall through to its own
    // interactive login and the user would be authenticating a tool they thought
    // aello had already configured.
    let Some(auth) = cfg.cline.as_ref() else {
        bail!(
            "blueprint '{}' is a Cline blueprint but there is no Cline login — \
             run `aello login --agent cline` first",
            bp.name
        );
    };

    if cline::single_word_prompt(prompt) {
        bail!(
            "Cline rejects a one-word prompt as a possible subcommand — use more than one word              (this is Cline's rule, not a quoting problem on your side)"
        );
    }

    // Install the credential through `cline auth`. Every run, because the key
    // can change in config.toml and a marker saying "already authenticated"
    // would go stale exactly then.
    cline::ensure_credential(&env, auth)?;

    // Cline resumes by session id only; there is no `--continue`. Say so rather
    // than dropping the flag, which would silently start a fresh session.
    if matches!(resume, Some(Some(ref s)) if s.is_empty()) || matches!(resume, Some(None)) {
        bail!("Cline has no 'continue most recent' — pass a session id: aello run {} --resume <id>", bp.name);
    }

    cline::launch(&env, &bp.name, Some(auth), &bp.model, resume.as_ref(), prompt, extra)
}

/// `aello login` covers two different accounts, so it asks which one rather
/// than assuming. The user's rule: the two must stay separate — a Claude
/// subscription and a Cline provider key are different logins with different
/// billing, and setting one must never look like setting the other.
fn cmd_login(agent: Option<Agent>) -> Result<()> {
    let agent = match agent {
        Some(a) => a,
        None => {
            println!("Which agent are you logging in?");
            for (i, a) in Agent::ALL.iter().enumerate() {
                println!("  {}. {} — {}", i + 1, a.as_str(), a.describe());
            }
            match prompt("Choice", "1")?.trim() {
                "2" | "cline" => Agent::Cline,
                _ => Agent::Claude,
            }
        }
    };
    match agent {
        Agent::Claude => cmd_login_claude(),
        Agent::Cline => cmd_login_cline(),
    }
}

fn cmd_login_claude() -> Result<()> {
    match auth::capture_setup_token()? {
        Some(token) => {
            let mut cfg = config::load()?;
            cfg.oauth_token = Some(token);
            config::save(&cfg)?;
            println!("Saved shared login token. All envs will use it (CLAUDE_CODE_OAUTH_TOKEN).");
        }
        None => println!("Cancelled — no token saved."),
    }
    Ok(())
}

/// Store a Cline provider credential, shared by every Cline env the way the
/// OAuth token is shared by every Claude env.
///
/// Prompted rather than captured from a subprocess: `cline auth` takes the same
/// three values as flags (`-p`, `-k`, `-m`), so there is no browser flow to tee
/// the way `claude setup-token` needs. aello writes `providers.json` itself at
/// placement, which is also what makes an env placeable with no `cline` on PATH.
fn cmd_login_cline() -> Result<()> {
    let mut cfg = config::load()?;
    let current = cfg.cline.as_ref();
    println!("Cline is billed per token by your provider — this is not the Claude subscription.");
    let provider = prompt(
        "Provider id (openrouter, cline, anthropic, …)",
        current.map_or("openrouter", |c| c.provider.as_str()),
    )?;
    let model = prompt(
        "Model id for that provider",
        current.map_or("openai/gpt-5.6-luna-pro", |c| c.model.as_str()),
    )?;
    let key = prompt_optional("API key (blank to keep any existing / use no key)")?;
    let base_url = prompt_optional("Base URL override (blank for the provider default)")?;

    let api_key = key.or_else(|| current.and_then(|c| c.api_key.clone()));
    cfg.cline = Some(models::ClineAuth { provider, api_key, model, base_url });
    config::save(&cfg)?;
    println!("Saved the Cline login. Every Cline env will use it.");
    Ok(())
}

/// First-run wizard: ensure a shared login token exists, then walk the user
/// through creating their first blueprint. Idempotent — re-running it with a
/// token and blueprints already present just reports and exits.
fn cmd_init() -> Result<()> {
    let mut cfg = config::load()?;

    if cfg.oauth_token.is_none() {
        println!("No shared login token yet — let's create one.");
        cmd_login(Some(Agent::Claude))?;
        cfg = config::load()?; // reload to pick up the saved token
        if cfg.oauth_token.is_none() {
            println!("\nSkipped login — re-run `aello init` or `aello login` when ready.");
            return Ok(());
        }
    } else {
        println!("Shared login token already set.");
    }

    if !cfg.blueprints.is_empty() {
        println!(
            "\nYou already have {} blueprint(s). Launch one with `aello run <name>`.",
            cfg.blueprints.len()
        );
        return Ok(());
    }

    println!("\nNow let's create your first blueprint.");
    let name = prompt("Blueprint name", "coder")?;
    validate_name(&name)?;
    let model = prompt("Model (opus/sonnet/haiku or a claude-* id)", "sonnet")?;
    let model = validate_model(&model)?;
    let persona = prompt_optional("Persona (coder for a coding project, blank for none)")?;
    if let Some(p) = &persona {
        templates::resolve(p)?; // fail now on a bad name/path, not on first run
    }

    println!("\nRole — what this blueprint is responsible for:");
    for r in Role::ALL {
        println!("  {:<12} {}", r.as_str(), r.describe());
    }
    let role = prompt_role("Role", Role::Maintainer)?;

    // Re-read immediately before mutating. The `cfg` above was loaded before a
    // run of interactive prompts with no time bound on it, and saving that stale
    // snapshot would discard anything written meanwhile — most plausibly an
    // `aello login` in another terminal, whose token is the one thing here that
    // is expensive to lose. Every other command reloads right before it writes.
    let mut cfg = config::load()?;
    if cfg.find_name_conflict(&name).is_some() {
        bail!("blueprint '{name}' was created while you were answering — nothing written");
    }
    cfg.blueprints.push(Blueprint {
        name: name.clone(),
        model,
        // The first-run wizard makes Claude blueprints only. A Cline env needs a
        // metered provider key, which is not a thing to ask for before someone
        // has run aello once.
        agent: Agent::Claude,
        claude_md: persona,
        role,
        legacy_caps: None,
    });
    config::save(&cfg)?;
    println!(
        "\nCreated blueprint '{name}'. Launch it in a project with:\n    aello run {name}"
    );
    Ok(())
}

/// Read a line from stdin, returning `default` if the user just hits Enter.
fn prompt(label: &str, default: &str) -> Result<String> {
    use std::io::Write;
    print!("{label} [{default}]: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).context("could not read input")? == 0 {
        bail!("unexpected end of input — run `aello init` in an interactive terminal");
    }
    let v = line.trim();
    Ok(if v.is_empty() { default.to_string() } else { v.to_string() })
}

/// Role prompt on stdin; blank → `default`, an unrecognised word re-asks rather
/// than silently falling back — picking the wrong role changes what `/sync` is
/// allowed to rewrite, so a typo must not be interpreted as consent.
fn prompt_role(label: &str, default: Role) -> Result<Role> {
    use clap::ValueEnum;
    loop {
        let raw = prompt(label, default.as_str())?;
        match Role::from_str(raw.trim(), true) {
            Ok(r) => return Ok(r),
            Err(_) => println!(
                "  '{}' isn't a role — pick one of: maintainer, contributor, standalone",
                raw.trim()
            ),
        }
    }
}

/// Read an optional line from stdin; blank → None.
fn prompt_optional(label: &str) -> Result<Option<String>> {
    use std::io::Write;
    print!("{label}: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).context("could not read input")? == 0 {
        bail!("unexpected end of input — run `aello init` in an interactive terminal");
    }
    let v = line.trim();
    Ok((!v.is_empty()).then(|| v.to_string()))
}

/// Accept a generated persona into a placed env.
///
/// Three writes that belong together: the persona file, the blueprint's
/// `claude_md = "custom"` so `place` stops seeding a template over it, and the
/// generation sidecar. Doing this through aello rather than by hand is what
/// keeps the config edit safe — `Config` is serialized from the struct, so a
/// key written into `config.toml` by anything else is dropped on the next save.
fn cmd_persona(name: String, from: PathBuf, project: Option<PathBuf>) -> Result<()> {
    let mut cfg = config::load()?;
    let bp = cfg
        .find(&name)
        .with_context(|| format!("no blueprint named '{name}'"))?;
    let bp_name = bp.name.clone();

    let content = std::fs::read_to_string(&from)
        .with_context(|| format!("could not read the persona at {}", from.display()))?;
    if content.trim().is_empty() {
        anyhow::bail!("{} is empty — refusing to blank a persona", from.display());
    }

    let project = match project {
        Some(p) => p,
        None => std::env::current_dir().context("could not read the current directory")?,
    };
    let env = project::env_dir(&project, &bp_name);
    let (gen, date) = project::accept_persona(&env, &content)?;

    // Flip to `custom` last: if the write above failed, the config still points
    // at whatever was seeding the persona before.
    if let Some(bp) = cfg.blueprints.iter_mut().find(|b| b.name == bp_name) {
        bp.claude_md = Some(templates::CUSTOM.to_string());
    }
    config::save(&cfg)?;

    println!("{bp_name}: persona accepted as gen{gen} ({date})");
    println!("  {}", env.join("CLAUDE.md").display());
    println!("  claude_md = \"custom\" — aello will not overwrite it");
    Ok(())
}

/// Print a bundled doc to stdout, or list them all when no name is given. The
/// docs ship inside the binary (see `docs.rs`), so this works on any install.
fn cmd_docs(name: Option<String>) -> Result<()> {
    match name {
        None => {
            println!("Reference docs — print one with `aello docs <name>`:\n");
            let all = docs::all();
            // Width from the longest slug, so adding a doc can't break the column.
            let w = all.iter().map(|d| d.slug.len()).max().unwrap_or(0);
            for d in &all {
                println!("  {:<w$}  {}", d.slug, d.title);
            }
        }
        Some(slug) => match docs::get(&slug) {
            Some(d) => print!("{}", d.body),
            None => {
                let avail: Vec<String> = docs::all().into_iter().map(|d| d.slug).collect();
                bail!("no doc '{slug}'. Available: {}", avail.join(", "));
            }
        },
    }
    Ok(())
}

/// Print a clap-generated completion script for `shell` to stdout. Generated
/// from the derived CLI, so it stays in sync with the commands automatically.
fn cmd_completions(shell: clap_complete::Shell) -> Result<()> {
    let mut cmd = Cli::command();
    clap_complete::generate(shell, &mut cmd, "aello", &mut std::io::stdout());
    Ok(())
}

fn cmd_list(json: bool) -> Result<()> {
    let cfg = config::load()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&cfg.blueprints)?);
        return Ok(());
    }
    if cfg.blueprints.is_empty() {
        println!("No blueprints yet. Add one with: aello add <name> --model <model>");
        return Ok(());
    }
    let name_w = cfg.blueprints.iter().map(|b| b.name.len()).max().unwrap_or(4).max(4);
    let model_w = cfg.blueprints.iter().map(|b| b.model.len()).max().unwrap_or(5).max(5);
    let cm_w = cfg
        .blueprints
        .iter()
        .map(|b| b.claude_md.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(9)
        .max(9);
    println!("{:<name_w$}  {:<model_w$}  {:<cm_w$}  ROLE", "NAME", "MODEL", "CLAUDE.md");
    for b in &cfg.blueprints {
        println!(
            "{:<name_w$}  {:<model_w$}  {:<cm_w$}  {}",
            b.name,
            b.model,
            b.claude_md.as_deref().unwrap_or("-"),
            b.role.as_str(),
        );
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names_accepted() {
        for n in ["test", "my-agent", "agent_1", "ABC123"] {
            assert!(validate_name(n).is_ok(), "{n} should be valid");
        }
    }

    #[test]
    fn invalid_names_rejected() {
        for n in ["", "bad name", "a/b", "x.y", "a:b", "café", "ｆｕｌｌ",
                  "../../evil", "..", "a\\b", "a\nb"] {
            assert!(validate_name(n).is_err(), "{n:?} should be rejected");
        }
    }

    #[test]
    fn reserved_device_names_rejected() {
        for n in ["con", "CON", "nul", "Nul", "PRN", "aux", "com1", "COM9", "lpt1", "LPT9"] {
            assert!(validate_name(n).is_err(), "{n:?} should be rejected (reserved)");
        }
        // Near-misses that are NOT reserved must still pass.
        for n in ["con1", "com", "com0", "com10", "lpt", "console", "nul-agent"] {
            assert!(validate_name(n).is_ok(), "{n:?} should be valid");
        }
    }

    #[test]
    fn valid_models_accepted() {
        for m in ["opus", "Sonnet", "HAIKU", "default", "claude-opus-4-8", "claude-fable-5"] {
            assert!(validate_model(m).is_ok(), "{m} should be valid");
        }
    }

    #[test]
    fn invalid_models_rejected() {
        for m in ["", "opu", "sonnett", "gpt-4", "opus4", "claude-"] {
            assert!(validate_model(m).is_err(), "{m:?} should be rejected");
        }
    }

    #[test]
    fn validate_model_returns_the_normalised_value() {
        // The check ran against a trimmed+lowercased copy while callers stored
        // the raw string, so `--model " opus "` validated and then reached
        // settings.json verbatim, quotes and all.
        assert_eq!(validate_model("  opus  ").unwrap(), "opus");
        assert_eq!(validate_model("SONNET").unwrap(), "sonnet");
        assert_eq!(validate_model(" Claude-Opus-4-8 ").unwrap(), "claude-opus-4-8");
    }

    #[test]
    fn over_long_names_are_rejected() {
        // Previously accepted here and failed later inside create_dir_all with a
        // raw OS error naming a path the user never typed.
        assert!(validate_name(&"a".repeat(64)).is_ok());
        let long = "a".repeat(65);
        let err = validate_name(&long).unwrap_err().to_string();
        assert!(err.contains("65 characters"), "unhelpful message: {err}");
    }

    /// `--role` takes the same three words everywhere: CLI, `list` output, TUI.
    #[test]
    fn role_flag_parses_the_names_we_print() {
        use clap::ValueEnum;
        for r in Role::ALL {
            assert_eq!(Role::from_str(r.as_str(), true).unwrap(), *r);
        }
        assert!(Role::from_str("owner", true).is_err());
    }
}
