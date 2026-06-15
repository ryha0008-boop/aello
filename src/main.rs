use clap::{Parser, Subcommand};

/// Isolated Claude Code environments — like venvs, but for AI agents.
#[derive(Parser)]
#[command(name = "aello", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    // Subcommands land here in later phases (add, list, run, hook, ...).
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        None => {
            // No args → interactive mode (Phase 6). Placeholder for now.
            println!("aello {} — interactive mode coming soon", env!("CARGO_PKG_VERSION"));
        }
        Some(_) => {}
    }
}
