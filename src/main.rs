use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

mod config;
mod launch;
mod models;
mod project;
mod tui;
mod update;

use models::{Blueprint, Instance};

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
        /// Path to a CLAUDE.md template copied into the env dir on first run.
        #[arg(long)]
        claude_md: Option<String>,
    },
    /// List all blueprints.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove a blueprint by name.
    Remove { name: String },
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
    /// Update aello to the latest build from GitHub.
    Update,
    // More subcommands land here in later phases (edit, sessions, ...).
}

fn main() {
    // Windows leaves the previous binary as aello.exe.old after a self-update;
    // remove it on the next launch.
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }

    let cli = Cli::parse();
    let result = match cli.command {
        None => tui::run(),
        Some(Commands::Add { name, model, claude_md }) => cmd_add(name, model, claude_md),
        Some(Commands::List { json }) => cmd_list(json),
        Some(Commands::Remove { name }) => cmd_remove(name),
        Some(Commands::Run { name, resume, prompt, extra }) => cmd_run(name, resume, prompt, extra),
        Some(Commands::Update) => update::run(),
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
    if MODEL_ALIASES.contains(&m.as_str()) || m.starts_with("claude-") {
        return Ok(());
    }
    bail!(
        "unknown model '{model}'. Use an alias ({}) or a full model id like claude-opus-4-8",
        MODEL_ALIASES.join(", ")
    );
}

fn cmd_add(name: String, model: String, claude_md: Option<String>) -> Result<()> {
    validate_name(&name)?;
    validate_model(&model)?;
    let mut cfg = config::load()?;
    if cfg.find(&name).is_some() {
        bail!("blueprint '{name}' already exists");
    }
    cfg.blueprints.push(Blueprint { name: name.clone(), model, claude_md });
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

fn cmd_run(
    name: Option<String>,
    resume: Option<Option<String>>,
    prompt: Option<String>,
    extra: Vec<String>,
) -> Result<()> {
    let cfg = config::load()?;
    let bp: &Blueprint = match &name {
        Some(n) => cfg.find(n).with_context(|| format!("no blueprint named '{n}'"))?,
        None => match cfg.blueprints.as_slice() {
            [one] => one,
            [] => bail!("no blueprints — add one with: aello add <name> --model <model>"),
            _ => bail!("multiple blueprints — specify one: aello run <name>"),
        },
    };

    let project = std::env::current_dir().context("could not determine current directory")?;
    let env = project::env_dir(&project, &bp.name);
    let inst = Instance { name: bp.name.clone(), model: bp.model.clone() };

    // Read the CLAUDE.md template contents if the blueprint points at one.
    let claude_md = match &bp.claude_md {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(c) => Some(c),
            Err(_) => {
                eprintln!("warning: claude_md '{path}' not found — skipping");
                None
            }
        },
        None => None,
    };

    project::place(&env, &inst, claude_md.as_deref())?;

    if !env.join(".credentials.json").exists() {
        println!("Launching '{}' — new env, Claude will prompt login on first use.", bp.name);
    }

    // `--resume` with no value means "continue most recent".
    let resume = match resume {
        Some(Some(s)) if s.is_empty() => Some(None),
        other => other,
    };
    let code = launch::launch(&env, resume.as_ref(), prompt.as_deref(), &extra)?;
    std::process::exit(code);
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
    println!("{:<name_w$}  {:<model_w$}  CLAUDE.md", "NAME", "MODEL");
    for b in &cfg.blueprints {
        println!(
            "{:<name_w$}  {:<model_w$}  {}",
            b.name,
            b.model,
            b.claude_md.as_deref().unwrap_or("-")
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
        for m in ["", "opu", "sonnett", "gpt-4", "opus4"] {
            assert!(validate_model(m).is_err(), "{m:?} should be rejected");
        }
    }
}
