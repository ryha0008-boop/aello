use clap::{Parser, Subcommand};

mod update;

/// Isolated Claude Code environments — like venvs, but for AI agents.
#[derive(Parser)]
#[command(name = "aello", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Update aello to the latest build from GitHub.
    Update,
    // More subcommands land here in later phases (add, list, run, hook, ...).
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
        Some(Commands::Update) => update::run(),
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
