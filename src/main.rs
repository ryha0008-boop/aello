use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

mod config;
mod models;
mod update;

use models::Blueprint;

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
    /// Update aello to the latest build from GitHub.
    Update,
    // More subcommands land here in later phases (run, edit, hook, ...).
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
        None => {
            // No args → interactive mode (Phase 6). Placeholder for now.
            println!("aello {} — interactive mode coming soon", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(Commands::Add { name, model, claude_md }) => cmd_add(name, model, claude_md),
        Some(Commands::List { json }) => cmd_list(json),
        Some(Commands::Update) => update::run(),
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

/// Blueprint names map to env-dir names, so keep them filesystem-safe.
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("name cannot be empty");
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        bail!("name '{name}' must contain only letters, digits, '-' or '_'");
    }
    Ok(())
}

fn cmd_add(name: String, model: String, claude_md: Option<String>) -> Result<()> {
    validate_name(&name)?;
    let mut cfg = config::load()?;
    if cfg.find(&name).is_some() {
        bail!("blueprint '{name}' already exists");
    }
    cfg.blueprints.push(Blueprint { name: name.clone(), model, claude_md });
    config::save(&cfg)?;
    println!("Added blueprint '{name}'.");
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
}
