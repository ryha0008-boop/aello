use serde::{Deserialize, Serialize};

/// A global AI identity stored in aello's config. Placing a blueprint into a
/// project produces an Instance (see Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blueprint {
    pub name: String,
    pub model: String,
    /// Path to a CLAUDE.md template, copied into the env dir at placement time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_md: Option<String>,
}

/// A blueprint placed into a project directory. Stored as `.aello.toml` inside
/// the env dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub name: String,
    pub model: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub blueprints: Vec<Blueprint>,
    /// Unified folder for PostCompact transcripts (per-machine). `~` allowed.
    /// Unset → default `~/aello/contextdb`. Configurable from the TUI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contextdb: Option<String>,
    /// Share one Claude login across all envs via a central cache. Unset → on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_login: Option<bool>,
}

impl Config {
    pub fn find(&self, name: &str) -> Option<&Blueprint> {
        self.blueprints.iter().find(|b| b.name == name)
    }
}
