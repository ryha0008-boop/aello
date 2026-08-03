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

/// What a blueprint is responsible for in a project — the choice made at `add`
/// time, changed with `aello edit --role`.
///
/// This replaces picking five capability booleans individually. The flags were
/// never independent in practice: across every repo worked by more than one
/// blueprint, the shape is one **maintainer** holding everything and
/// **contributors** that commit their own code and nothing else. Three roles
/// keep that distinction — which is the whole multi-agent point, so it must not
/// be flattened away — while removing 29 unused combinations from the surface.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum Role {
    /// Owns the repo's prose: project CLAUDE.md, CHANGELOG, docs/, README, plus
    /// git. One per repo.
    Maintainer,
    /// Commits, pushes, and logs its own change in the CHANGELOG. Never touches
    /// CLAUDE.md, docs/ or README — those belong to the maintainer.
    Contributor,
    /// Works alone: no `/sync`, no git duties, nothing scaffolded.
    #[default]
    Standalone,
}

impl Role {
    pub const ALL: &'static [Role] = &[Role::Maintainer, Role::Contributor, Role::Standalone];

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Maintainer => "maintainer",
            Role::Contributor => "contributor",
            Role::Standalone => "standalone",
        }
    }

    /// One-line description, shared by `aello list`, the TUI picker and `--help`.
    pub fn describe(&self) -> &'static str {
        match self {
            Role::Maintainer => "owns the docs: CLAUDE.md, CHANGELOG, docs/, README + git",
            Role::Contributor => "commits, pushes, and logs its own change",
            Role::Standalone => "no /sync — works alone",
        }
    }

    /// What `/sync` covers for this role. The rest of the codebase still works in
    /// terms of [`Capabilities`]; a role is just the set of combinations we offer.
    pub fn caps(&self) -> Capabilities {
        match self {
            Role::Maintainer => Capabilities {
                project_md: true,
                github: true,
                changelog: true,
                docs: true,
                readme: true,
            },
            Role::Contributor => Capabilities {
                github: true,
                changelog: true,
                ..Default::default()
            },
            Role::Standalone => Capabilities::default(),
        }
    }

    /// Fold a pre-0.2 five-boolean config into a role. Anything that maintained
    /// prose a contributor may not touch (project CLAUDE.md, docs/, README) is a
    /// maintainer; anything left holding only git duties is a contributor.
    pub fn from_caps(c: &Capabilities) -> Role {
        if !c.any() {
            Role::Standalone
        } else if c.project_md || c.docs || c.readme {
            Role::Maintainer
        } else {
            Role::Contributor
        }
    }
}

/// A global AI identity stored in aello's config. Placing a blueprint into a
/// project produces an Instance (see Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blueprint {
    pub name: String,
    pub model: String,
    /// Global persona: `coder`, `none`, `custom`, or a path to a CLAUDE.md
    /// file. `custom` means the env's own CLAUDE.md is authoritative and aello
    /// writes nothing — that is the steady state once a persona is generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_md: Option<String>,
    /// What this blueprint is responsible for. See [`Role`].
    #[serde(default)]
    pub role: Role,
    /// Pre-0.2 configs stored five capability booleans here. Read once by
    /// [`Config::migrate_roles`], folded into `role`, and never written back —
    /// so the key disappears from `config.toml` on the next save.
    #[serde(rename = "caps", default, skip_serializing)]
    pub legacy_caps: Option<Capabilities>,
}

impl Blueprint {
    /// What `/sync` covers, derived from the role.
    pub fn caps(&self) -> Capabilities {
        self.role.caps()
    }
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
    /// Fold any pre-0.2 `caps` tables into roles. Run on every load rather than
    /// once behind a marker: it's cheap, idempotent, and a config restored from a
    /// backup or edited by hand heals itself instead of silently loading as
    /// `standalone`. The old key is dropped whenever something next saves.
    pub fn migrate_roles(&mut self) {
        for bp in &mut self.blueprints {
            if let Some(caps) = bp.legacy_caps.take() {
                bp.role = Role::from_caps(&caps);
            }
        }
    }

    /// Fold pre-`custom` persona values into the three the add flow now offers.
    ///
    /// Two moves, both idempotent and both run on every load for the same reason
    /// `migrate_roles` is — a restored backup heals itself instead of loading
    /// with values nothing understands:
    ///
    /// - **absent → `none`.** A blank persona becomes a stated decision rather
    ///   than a missing key, which is what lets `custom` mean something.
    /// - **`sysadmin` → `custom`.** The template is gone, and the env that used
    ///   it already holds the text `place` wrote at first placement — personas
    ///   are never clobbered. Calling it `coder` would be false; `custom` says
    ///   what is true, that the persona now lives in the env dir.
    ///
    /// Paths are left exactly as they are: several blueprints point at persona
    /// files the user maintains, and some share one file deliberately.
    pub fn migrate_personas(&mut self) {
        for bp in &mut self.blueprints {
            match bp.claude_md.as_deref() {
                None => bp.claude_md = Some(crate::templates::NONE.to_string()),
                Some("sysadmin") => bp.claude_md = Some(crate::templates::CUSTOM.to_string()),
                _ => {}
            }
        }
    }

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
        Blueprint {
            name: name.into(),
            model: "opus".into(),
            claude_md: None,
            role: Role::Standalone,
            legacy_caps: None,
        }
    }

    #[test]
    fn persona_migration_folds_absent_and_sysadmin() {
        let absent = bp("a");
        let mut sysadmin = bp("b");
        sysadmin.claude_md = Some("sysadmin".into());
        let mut coder = bp("c");
        coder.claude_md = Some("coder".into());
        let mut path = bp("d");
        path.claude_md = Some(r"C:\personas\driver.CLAUDE.md".into());

        let mut cfg = Config { blueprints: vec![absent, sysadmin, coder, path], ..Config::default() };
        cfg.migrate_personas();

        // A missing key becomes a stated decision.
        assert_eq!(cfg.blueprints[0].claude_md.as_deref(), Some("none"));
        // The dropped template points at the env's own copy, which place() left
        // alone — calling this "coder" would claim text the env does not have.
        assert_eq!(cfg.blueprints[1].claude_md.as_deref(), Some("custom"));
        // Untouched: still a real template, and still a user-maintained file.
        assert_eq!(cfg.blueprints[2].claude_md.as_deref(), Some("coder"));
        assert_eq!(cfg.blueprints[3].claude_md.as_deref(), Some(r"C:\personas\driver.CLAUDE.md"));

        // Idempotent — it runs on every load.
        cfg.migrate_personas();
        assert_eq!(cfg.blueprints[0].claude_md.as_deref(), Some("none"));
        assert_eq!(cfg.blueprints[1].claude_md.as_deref(), Some("custom"));
    }

    /// The two fleet blueprints that didn't hold an all-or-nothing cap set are
    /// the whole reason roles have a middle tier — pin their mapping.
    #[test]
    fn legacy_caps_fold_into_roles() {
        let all = Capabilities {
            project_md: true, github: true, changelog: true, docs: true, readme: true,
        };
        assert_eq!(Role::from_caps(&all), Role::Maintainer);
        assert_eq!(Role::from_caps(&Capabilities::default()), Role::Standalone);
        // ShellyFrontEndDev: git duties only → contributor, unchanged behaviour.
        assert_eq!(
            Role::from_caps(&Capabilities { github: true, changelog: true, ..Default::default() }),
            Role::Contributor
        );
        // PersonallyDev: four of five, no readme → maintainer (gains README upkeep).
        assert_eq!(
            Role::from_caps(&Capabilities {
                project_md: true, github: true, changelog: true, docs: true, readme: false,
            }),
            Role::Maintainer
        );
        // github-only secondaries → contributor.
        assert_eq!(
            Role::from_caps(&Capabilities { github: true, ..Default::default() }),
            Role::Contributor
        );
    }

    /// A role round-trips through its capability expansion, so `from_caps` is a
    /// true inverse for the three sets we actually offer.
    #[test]
    fn roles_round_trip_through_caps() {
        for r in Role::ALL {
            assert_eq!(Role::from_caps(&r.caps()), *r, "{} did not round-trip", r.as_str());
        }
    }

    #[test]
    fn old_config_migrates_and_drops_the_caps_key() {
        let text = r#"
[[blueprints]]
name = "Old"
model = "opus"
[blueprints.caps]
project_md = true
github = true
changelog = true
docs = true
readme = true
"#;
        let mut cfg: Config = toml::from_str(text).unwrap();
        cfg.migrate_roles();
        assert_eq!(cfg.blueprints[0].role, Role::Maintainer);
        let out = toml::to_string_pretty(&cfg).unwrap();
        assert!(out.contains(r#"role = "maintainer""#), "role not written: {out}");
        assert!(!out.contains("caps"), "legacy caps key survived a save: {out}");
    }

    /// A config already on roles must not be clobbered by the migration.
    #[test]
    fn new_config_is_left_alone() {
        let text = "[[blueprints]]\nname = \"New\"\nmodel = \"opus\"\nrole = \"contributor\"\n";
        let mut cfg: Config = toml::from_str(text).unwrap();
        cfg.migrate_roles();
        assert_eq!(cfg.blueprints[0].role, Role::Contributor);
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
