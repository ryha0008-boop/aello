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

/// Which CLI a blueprint drives. Chosen at `add` time and fixed thereafter —
/// the two agents share nothing on disk, so switching one would strand its env.
///
/// The split is deliberate and total: a Claude blueprint lives in
/// `.claude-env-<name>` and is configured by an environment variable; a Cline
/// blueprint lives in `.cline-env-<name>` and is configured by command-line
/// flags. Nothing is shared but the project directory itself. Everything that
/// differs between them is behind this enum or in `cline.rs`, so neither can
/// quietly acquire the other's assumptions.
///
/// `#[serde(default)]` on the field is the whole migration: every blueprint
/// written before this loads as `Claude`, which is what it was.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum Agent {
    /// Claude Code. Subscription auth via the shared OAuth token.
    #[default]
    Claude,
    /// The Cline CLI. Needs its own provider credential — see [`ClineAuth`].
    Cline,
}

impl Agent {
    pub const ALL: &'static [Agent] = &[Agent::Claude, Agent::Cline];

    pub fn as_str(&self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Cline => "cline",
        }
    }

    /// The env dir prefix. Claude's is unchanged from before this enum existed:
    /// 39 envs are on disk under `.claude-env-*` and a rename would strand all
    /// of them, so the new agent gets the new prefix and the old one keeps its.
    pub fn env_prefix(&self) -> &'static str {
        match self {
            Agent::Claude => ".claude-env-",
            Agent::Cline => ".cline-env-",
        }
    }

    /// Where this agent's env dir for `name` lives inside `project`.
    ///
    /// Every path that names an env dir goes through here. `project::env_dir` is
    /// hardcoded to Claude's prefix, and the sites that reached for it without
    /// asking the blueprint which agent it was — `remove --purge`, `edit
    /// --rename`, the TUI's local filter, its session list and its delete
    /// warning — all silently did nothing at all to a Cline env. Nothing to
    /// delete, nothing to move, nothing said: a `.cline-env-<name>` holding a
    /// plaintext API key stayed on disk after its blueprint was purged.
    pub fn env_dir(&self, project: &std::path::Path, name: &str) -> std::path::PathBuf {
        project.join(format!("{}{name}", self.env_prefix()))
    }

    /// The gitignore line that covers this agent's env dirs.
    pub fn gitignore_pattern(&self) -> &'static str {
        match self {
            Agent::Claude => ".claude-env-*",
            Agent::Cline => ".cline-env-*",
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code — shared subscription login",
            Agent::Cline => "Cline CLI — its own provider key, metered",
        }
    }
}

/// Credentials for the Cline CLI, stored once and shared by every Cline env
/// exactly as `oauth_token` is shared by every Claude env.
///
/// Separate from `oauth_token` on purpose: the two are different accounts with
/// different billing, and `aello login` asks which one you mean rather than
/// overloading a single field. Cline reads it from
/// `<data-dir>/settings/providers.json`, which is installed by shelling out to
/// `cline auth` on every run — never by writing that file directly. Cline
/// rewrites it on its next run and drops a hand-written `apiKey` outright, so
/// the env then reaches the provider carrying no credential at all.
///
/// **Metered.** Unlike the Claude token, every turn spent through this costs
/// money per token. That is a change in what aello is, not just a new field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClineAuth {
    /// Cline provider id, e.g. `openrouter`, `cline`, `anthropic`.
    pub provider: String,
    /// The provider's API key. Absent for a provider that authenticates some
    /// other way — `cline auth` records `tokenSource` for those and stores no
    /// key in `providers.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Model id for that provider, e.g. `openai/gpt-5.6-luna-pro`.
    pub model: String,
    /// Optional base URL override (self-hosted, proxy, Azure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// A global AI identity stored in aello's config. Placing a blueprint into a
/// project produces an Instance (see Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blueprint {
    pub name: String,
    pub model: String,
    /// Which CLI this blueprint drives. Absent means `claude` — see [`Agent`].
    #[serde(default)]
    pub agent: Agent,
    /// Global persona: `coder`, `none`, `custom`, or a path to a CLAUDE.md
    /// file. `custom` means the env's own CLAUDE.md is authoritative and aello
    /// writes nothing — that is the steady state once a persona is generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_md: Option<String>,
    /// What this blueprint is responsible for. See [`Role`].
    #[serde(default)]
    pub role: Role,
    /// Where `/sync`'s `claude-internal/<name>/` mirror is written. Absent — the
    /// normal case — means inside the project, which is what every blueprint did
    /// before this existed.
    ///
    /// It exists for one situation: **a public repo**. The mirror is this env's
    /// memory, persona and handoff, and `/sync` stages it by path, so in a public
    /// repo it is a publish rather than a backup. Deleting the mirror is not the
    /// answer — being in git is exactly what makes an env restorable from another
    /// machine — so the destination moves instead. Point it at a working tree of
    /// a **private** repo and the product stays public while the memory does not.
    ///
    /// A path, not a git URL: aello never clones or pushes on the user's behalf,
    /// and `/sync` is the thing that commits. `~` is expanded. The
    /// `<blueprint>/` component is still appended, so several blueprints can
    /// share one destination without clobbering each other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_root: Option<String>,
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
    /// Carried down from [`Blueprint::mirror_root`] so `place` can resolve the
    /// mirror without reaching back into `config.toml`. `#[serde(default)]` is
    /// the whole migration: every `.aello.toml` written before this loads with
    /// `None`, which is the in-project mirror they already had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_root: Option<String>,
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
    /// Shared Cline provider credential. Set via `aello login --agent cline`.
    /// Deliberately not merged with `oauth_token`: different account, different
    /// billing, and one being set says nothing about the other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cline: Option<ClineAuth>,
    /// Path to this machine's secret store script (`vault.ps1`), when it has
    /// one. Set via `aello vault <path>`. A **path, never a secret** — the file
    /// it names is the only thing that ever sees a plaintext credential.
    ///
    /// Per-machine because `config.toml` is: the store is Windows DPAPI, so a
    /// VPS leaves this unset and keeps the `config.toml` fallback. Opt-in rather
    /// than detected — a detector is a cache that goes stale when the checkout
    /// moves, and it would make one repo behave differently on two machines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault: Option<String>,
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
            agent: Agent::Claude,
            claude_md: None,
            role: Role::Standalone,
            mirror_root: None,
            legacy_caps: None,
        }
    }

    /// The entire migration for 39 existing blueprints is this default. A config
    /// written before the agent field existed must load as Claude and, once
    /// saved, must not start claiming to be something else.
    #[test]
    fn a_blueprint_without_an_agent_is_a_claude_one() {
        let text = "[[blueprints]]\nname = \"Old\"\nmodel = \"opus\"\nrole = \"maintainer\"\n";
        let cfg: Config = toml::from_str(text).unwrap();
        assert_eq!(cfg.blueprints[0].agent, Agent::Claude);

        let out = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&out).unwrap();
        assert_eq!(back.blueprints[0].agent, Agent::Claude);
    }

    /// The two agents must never resolve to the same directory: one env dir
    /// holding both a Claude config and a Cline config is exactly the mixing
    /// this split exists to prevent, and it would be silent — each CLI would
    /// simply ignore the other's files.
    #[test]
    fn the_two_agents_never_share_an_env_dir_or_an_ignore_line() {
        assert_ne!(Agent::Claude.env_prefix(), Agent::Cline.env_prefix());
        assert_ne!(Agent::Claude.gitignore_pattern(), Agent::Cline.gitignore_pattern());
        // Claude's prefix is load-bearing: 39 env dirs on this machine are named
        // with it, and changing it strands every one of them.
        assert_eq!(Agent::Claude.env_prefix(), ".claude-env-");
        // And neither prefix may be a prefix of the other, or one agent's glob
        // would match the other's dirs.
        assert!(!Agent::Cline.env_prefix().starts_with(Agent::Claude.env_prefix()));
        assert!(!Agent::Claude.env_prefix().starts_with(Agent::Cline.env_prefix()));
    }

    /// A Cline credential must survive a round trip with its key intact, and a
    /// config with no Cline block must not grow one.
    #[test]
    fn the_cline_credential_is_separate_from_the_claude_token() {
        let mut cfg = Config { blueprints: vec![bp("a")], ..Default::default() };
        cfg.oauth_token = Some("sk-ant-oat01-xxx".into());
        let out = toml::to_string_pretty(&cfg).unwrap();
        assert!(!out.contains("[cline]"), "an unset Cline login was written anyway: {out}");

        cfg.cline = Some(ClineAuth {
            provider: "openrouter".into(),
            api_key: Some("sk-or-v1-xxx".into()),
            model: "openai/gpt-5.6-luna-pro".into(),
            base_url: None,
        });
        let back: Config = toml::from_str(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
        let c = back.cline.unwrap();
        assert_eq!(c.provider, "openrouter");
        assert_eq!(c.api_key.as_deref(), Some("sk-or-v1-xxx"));
        // Setting one login must never disturb the other.
        assert_eq!(back.oauth_token.as_deref(), Some("sk-ant-oat01-xxx"));
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
