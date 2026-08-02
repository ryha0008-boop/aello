use serde::{Deserialize, Serialize};

/// Per-blueprint capabilities chosen at creation. Each one scaffolds its files
/// (when placed) and adds a matching section to the generated `/sync` skill, so
/// `/sync` only covers what this blueprint actually has — a no-GitHub project
/// gets no git talk. Old configs without this section load all-false.
///
/// `voice` used to live here. It is now unconditional — every env speaks, and
/// silence is a runtime setting (`aello voice mute`), not a property of the
/// blueprint. Serde ignores the leftover `voice = …` in an existing config on
/// load, and the next save drops it.
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
    /// True if anything `/sync` covers is enabled — i.e. there's a reason to seed
    /// the skill.
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

    /// Case-insensitive name lookup, returning the conflicting blueprint's
    /// stored name. Env dir names (`.claude-env-<name>`) collide
    /// case-insensitively on Windows/macOS default filesystems, so two
    /// blueprints whose names differ only in case would map to a single on-disk
    /// env dir and clobber each other's state.
    pub fn find_name_conflict(&self, name: &str) -> Option<&str> {
        self.blueprints
            .iter()
            .find(|b| b.name.eq_ignore_ascii_case(name))
            .map(|b| b.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bp(name: &str) -> Blueprint {
        Blueprint { name: name.into(), model: "opus".into(), claude_md: None, caps: Capabilities::default() }
    }

    #[test]
    fn name_conflict_is_case_insensitive() {
        let cfg = Config { blueprints: vec![bp("Coder")], ..Default::default() };
        assert_eq!(cfg.find_name_conflict("coder"), Some("Coder"));
        assert_eq!(cfg.find_name_conflict("CODER"), Some("Coder"));
        assert_eq!(cfg.find_name_conflict("Coder"), Some("Coder"));
        assert_eq!(cfg.find_name_conflict("other"), None);
    }
}
