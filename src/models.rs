use serde::{Deserialize, Serialize};

/// Per-blueprint capabilities chosen at creation. Each one scaffolds its files
/// (when placed) and adds a matching section to the generated `/sync` skill, so
/// `/sync` only covers what this blueprint actually has — a no-GitHub project
/// gets no git talk. Old configs without this section load all-false.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities {
    /// Maintain a project-level CLAUDE.md (in the project dir, not the env).
    #[serde(default)]
    pub project_md: bool,
    /// `/sync` commits and pushes to GitHub.
    #[serde(default)]
    pub github: bool,
    /// Keep CHANGELOG.md current.
    #[serde(default)]
    pub changelog: bool,
    /// Keep the docs/ directory current.
    #[serde(default)]
    pub docs: bool,
    /// Keep README.md current.
    #[serde(default)]
    pub readme: bool,
}

impl Capabilities {
    /// True if anything is enabled — i.e. there's a reason to seed `/sync`.
    pub fn any(&self) -> bool {
        self.project_md || self.github || self.changelog || self.docs || self.readme
    }
}

/// A global AI identity stored in aello's config. Placing a blueprint into a
/// project produces an Instance (see Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blueprint {
    pub name: String,
    pub model: String,
    /// Global persona: a built-in template name (`coder`, `sysadmin`) or a path
    /// to a CLAUDE.md file, placed into the env dir as global instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_md: Option<String>,
    /// What this blueprint maintains via `/sync`. See [`Capabilities`].
    #[serde(default)]
    pub caps: Capabilities,
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
    /// Long-lived Claude OAuth token (from `claude setup-token`), passed to
    /// every env as CLAUDE_CODE_OAUTH_TOKEN. Doesn't rotate, so concurrent envs
    /// share it safely. Set via `aello login`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_token: Option<String>,
}

impl Config {
    pub fn find(&self, name: &str) -> Option<&Blueprint> {
        self.blueprints.iter().find(|b| b.name == name)
    }
}
