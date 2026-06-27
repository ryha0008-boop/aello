use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

mod auth;
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

use models::{Blueprint, Capabilities, Instance};

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
        /// Claude model, e.g. sonnet, opus, haiku.
        #[arg(long)]
        model: String,
        /// Global persona: a built-in template (coder, sysadmin) or a path to a
        /// CLAUDE.md file, placed into the env dir on first run.
        #[arg(long)]
        claude_md: Option<String>,
        /// `/sync` maintains a project-level CLAUDE.md.
        #[arg(long)]
        project_md: bool,
        /// `/sync` commits and pushes to GitHub.
        #[arg(long)]
        github: bool,
        /// `/sync` keeps CHANGELOG.md current.
        #[arg(long)]
        changelog: bool,
        /// `/sync` keeps the docs/ directory current.
        #[arg(long)]
        docs: bool,
        /// `/sync` keeps README.md current.
        #[arg(long)]
        readme: bool,
    },
    /// List all blueprints.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove a blueprint by name.
    Remove { name: String },
    /// Edit an existing blueprint's model, persona, or capabilities.
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
    /// Generate + store a shared Claude login token (runs `claude setup-token`).
    Login,
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
    Update,
    /// Show bundled reference docs (no name lists them).
    Docs {
        /// Doc to print (slug, e.g. `concepts`). Omit to list available docs.
        name: Option<String>,
    },
    // More subcommands land here in later phases (sessions, ...).
}

/// Flags for `aello edit`. Capability flags are tri-state: `--github` enables,
/// `--no-github` disables, omitting both leaves it unchanged. Changes take
/// effect on the next `aello run` (the global persona is never re-clobbered).
#[derive(clap::Args)]
struct EditArgs {
    /// Blueprint to edit.
    name: String,
    /// New model (alias like opus/sonnet/haiku or a full claude-* id).
    #[arg(long)]
    model: Option<String>,
    /// New global persona (built-in name or path to a CLAUDE.md file).
    #[arg(long)]
    claude_md: Option<String>,
    /// Enable the project-CLAUDE.md capability.
    #[arg(long)]
    project_md: bool,
    /// Disable the project-CLAUDE.md capability.
    #[arg(long)]
    no_project_md: bool,
    /// Enable the GitHub capability (attribution, scaffolds, /sync commit+push).
    #[arg(long)]
    github: bool,
    /// Disable the GitHub capability.
    #[arg(long)]
    no_github: bool,
    /// Enable the CHANGELOG.md capability.
    #[arg(long)]
    changelog: bool,
    /// Disable the CHANGELOG.md capability.
    #[arg(long)]
    no_changelog: bool,
    /// Enable the docs/ capability.
    #[arg(long)]
    docs: bool,
    /// Disable the docs/ capability.
    #[arg(long)]
    no_docs: bool,
    /// Enable the README.md capability.
    #[arg(long)]
    readme: bool,
    /// Disable the README.md capability.
    #[arg(long)]
    no_readme: bool,
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
        Some(Commands::Add { name, model, claude_md, project_md, github, changelog, docs, readme }) => {
            cmd_add(name, model, claude_md, Capabilities { project_md, github, changelog, docs, readme })
        }
        Some(Commands::List { json }) => cmd_list(json),
        Some(Commands::Remove { name }) => cmd_remove(name),
        Some(Commands::Edit(args)) => cmd_edit(args),
        Some(Commands::Run { name, resume, prompt, extra }) => cmd_run(name, resume, prompt, extra),
        Some(Commands::Init) => cmd_init(),
        Some(Commands::Login) => cmd_login(),
        Some(Commands::GithubSetup { name, public, yes }) => github::run(name, public, yes),
        Some(Commands::Update) => update::run(),
        Some(Commands::Docs { name }) => cmd_docs(name),
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
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        bail!("name '{name}' must contain only letters, digits, '-' or '_'");
    }
    Ok(())
}

/// Short aliases Claude Code accepts in settings.json "model".
const MODEL_ALIASES: &[&str] = &["opus", "sonnet", "haiku", "default"];

/// Reject typo'd models before they reach settings.json. Accept a known alias
/// (case-insensitive) or any full `claude-*` model id (forward-compatible with
/// new releases without an exact-version allowlist).
pub(crate) fn validate_model(model: &str) -> Result<()> {
    let m = model.trim().to_lowercase();
    if m.is_empty() {
        bail!("model cannot be empty");
    }
    if MODEL_ALIASES.contains(&m.as_str()) || m.strip_prefix("claude-").is_some_and(|r| !r.is_empty()) {
        return Ok(());
    }
    bail!(
        "unknown model '{model}'. Use an alias ({}) or a full model id like claude-opus-4-8",
        MODEL_ALIASES.join(", ")
    );
}

fn cmd_add(
    name: String,
    model: String,
    claude_md: Option<String>,
    caps: Capabilities,
) -> Result<()> {
    validate_name(&name)?;
    validate_model(&model)?;
    // Catch a typo'd built-in / missing template path at add time, not first run.
    if let Some(cm) = &claude_md {
        templates::resolve(cm)?;
    }
    let mut cfg = config::load()?;
    if cfg.find(&name).is_some() {
        bail!("blueprint '{name}' already exists");
    }
    cfg.blueprints.push(Blueprint { name: name.clone(), model, claude_md, caps });
    config::save(&cfg)?;
    println!("Added blueprint '{name}'.");
    Ok(())
}

fn cmd_remove(name: String) -> Result<()> {
    let mut cfg = config::load()?;
    let before = cfg.blueprints.len();
    cfg.blueprints.retain(|b| b.name != name);
    if cfg.blueprints.len() == before {
        bail!("no blueprint named '{name}'");
    }
    config::save(&cfg)?;
    println!("Removed blueprint '{name}'.");
    Ok(())
}

/// Resolve a tri-state capability flag: `on` wins, then `off`, else keep
/// `current`. Setting both is a usage error.
fn tri(on: bool, off: bool, current: bool, flag: &str) -> Result<bool> {
    if on && off {
        bail!("--{flag} and --no-{flag} cannot be used together");
    }
    Ok(if on { true } else if off { false } else { current })
}

fn cmd_edit(args: EditArgs) -> Result<()> {
    let mut cfg = config::load()?;
    let Some(idx) = cfg.blueprints.iter().position(|b| b.name == args.name) else {
        bail!("no blueprint named '{}'", args.name);
    };
    let bp = &mut cfg.blueprints[idx];
    let mut changed = false;

    if let Some(model) = args.model {
        validate_model(&model)?;
        bp.model = model;
        changed = true;
    }
    if let Some(cm) = args.claude_md {
        templates::resolve(&cm)?; // reject a typo'd built-in / missing path now
        bp.claude_md = Some(cm);
        changed = true;
    }

    let before = bp.caps.clone();
    bp.caps.project_md = tri(args.project_md, args.no_project_md, bp.caps.project_md, "project-md")?;
    bp.caps.github = tri(args.github, args.no_github, bp.caps.github, "github")?;
    bp.caps.changelog = tri(args.changelog, args.no_changelog, bp.caps.changelog, "changelog")?;
    bp.caps.docs = tri(args.docs, args.no_docs, bp.caps.docs, "docs")?;
    bp.caps.readme = tri(args.readme, args.no_readme, bp.caps.readme, "readme")?;
    changed |= bp.caps != before;

    if !changed {
        bail!("nothing to change — pass --model, --claude-md, or a capability flag");
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
    let cfg = config::load()?;
    let bp_name = match name {
        Some(n) => {
            if cfg.find(&n).is_none() {
                bail!("no blueprint named '{n}'");
            }
            n
        }
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

    let project = std::env::current_dir().context("could not determine current directory")?;
    let env = project::env_dir(&project, &bp.name);
    let inst = Instance { name: bp.name.clone(), model: bp.model.clone() };

    // Resolve the global persona: a built-in template name or a file path.
    let claude_md = match &bp.claude_md {
        Some(spec) => match templates::resolve(spec) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("warning: {e:#} — skipping CLAUDE.md");
                None
            }
        },
        None => None,
    };

    project::place(&env, &inst, claude_md.as_deref(), &bp.caps)?;

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

fn cmd_login() -> Result<()> {
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

/// First-run wizard: ensure a shared login token exists, then walk the user
/// through creating their first blueprint. Idempotent — re-running it with a
/// token and blueprints already present just reports and exits.
fn cmd_init() -> Result<()> {
    let mut cfg = config::load()?;

    if cfg.oauth_token.is_none() {
        println!("No shared login token yet — let's create one.");
        cmd_login()?;
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
    validate_model(&model)?;
    let persona = prompt_optional("Persona (coder/sysadmin/path, blank for none)")?;
    if let Some(p) = &persona {
        templates::resolve(p)?; // fail now on a bad name/path, not on first run
    }

    println!("\nCapabilities — what /sync maintains (Enter accepts the default):");
    let caps = Capabilities {
        github: prompt_bool("  github (commit + push, repo scaffolding)", true)?,
        project_md: prompt_bool("  project CLAUDE.md", false)?,
        changelog: prompt_bool("  CHANGELOG.md", false)?,
        docs: prompt_bool("  docs/ directory", false)?,
        readme: prompt_bool("  README.md", false)?,
    };

    cfg.blueprints.push(Blueprint {
        name: name.clone(),
        model,
        claude_md: persona,
        caps,
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

/// Yes/No prompt on stdin; blank or anything unrecognized → `default`.
fn prompt_bool(label: &str, default: bool) -> Result<bool> {
    use std::io::Write;
    let hint = if default { "Y/n" } else { "y/N" };
    print!("{label} [{hint}]: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).context("could not read input")? == 0 {
        bail!("unexpected end of input — run `aello init` in an interactive terminal");
    }
    Ok(match line.trim().to_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    })
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

/// Print a bundled doc to stdout, or list them all when no name is given. The
/// docs ship inside the binary (see `docs.rs`), so this works on any install.
fn cmd_docs(name: Option<String>) -> Result<()> {
    match name {
        None => {
            println!("Reference docs — print one with `aello docs <name>`:\n");
            for d in docs::all() {
                println!("  {:<14} {}", d.slug, d.title);
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
    println!("{:<name_w$}  {:<model_w$}  {:<cm_w$}  SYNC", "NAME", "MODEL", "CLAUDE.md");
    for b in &cfg.blueprints {
        println!(
            "{:<name_w$}  {:<model_w$}  {:<cm_w$}  {}",
            b.name,
            b.model,
            b.claude_md.as_deref().unwrap_or("-"),
            caps_label(&b.caps),
        );
    }
    Ok(())
}

/// Compact one-line summary of enabled capabilities for `list`.
fn caps_label(c: &Capabilities) -> String {
    let mut tags = Vec::new();
    if c.project_md {
        tags.push("project-md");
    }
    if c.github {
        tags.push("github");
    }
    if c.changelog {
        tags.push("changelog");
    }
    if c.docs {
        tags.push("docs");
    }
    if c.readme {
        tags.push("readme");
    }
    if tags.is_empty() {
        "-".to_string()
    } else {
        tags.join(",")
    }
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
        for n in ["", "bad name", "a/b", "x.y", "a:b"] {
            assert!(validate_name(n).is_err(), "{n:?} should be rejected");
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
    fn tri_state_resolves() {
        assert!(tri(false, false, true, "x").unwrap()); // omitted keeps current (true)
        assert!(!tri(false, false, false, "x").unwrap()); // omitted keeps current (false)
        assert!(tri(true, false, false, "x").unwrap()); // --x turns on
        assert!(!tri(false, true, true, "x").unwrap()); // --no-x turns off
        assert!(tri(true, true, false, "x").is_err()); // both = error
    }
}
