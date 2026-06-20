//! Placing a blueprint into a project: the env dir, its `.aello.toml`,
//! `settings.json`, optional CLAUDE.md, and the PostCompact hook script.

use crate::models::{Capabilities, Instance};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const POST_COMPACT_SCRIPT: &str = include_str!("hooks_post_compact.py");

/// Stack-agnostic CI workflow seeded for `github` blueprints. On every push to
/// `main` it bumps the patch in a plain `VERSION` file and commits it back with
/// `[skip ci]` — a `GITHUB_TOKEN` push does not re-trigger workflows, so there's
/// no loop. Mirrors aello's own release lessons; deliberately tied to no build
/// system, so it drops into any project.
const VERSION_WORKFLOW: &str = r#"name: version

# Auto-bump the patch in VERSION on every push to main, then commit it back with
# [skip ci] so the bump commit does not re-trigger this workflow (GITHUB_TOKEN
# pushes never do). Seeded by aello — stack-agnostic; VERSION is a plain x.y.z
# file. Bump minor/major by hand in VERSION for bigger releases.
on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: write

jobs:
  bump:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: main
      - name: bump patch in VERSION
        run: |
          cur=$(cat VERSION 2>/dev/null || echo 0.0.0)
          IFS=. read -r MA MI PA <<< "$cur"
          new="$MA.$MI.$((PA + 1))"
          echo "$new" > VERSION
          echo "bumped $cur -> $new"
          git config user.name  "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git commit -am "release: v$new [skip ci]"
          git push origin main
"#;

/// Env dir for a blueprint inside a project — `project/.claude-env-<name>`.
pub fn env_dir(project: &Path, name: &str) -> PathBuf {
    project.join(format!(".claude-env-{name}"))
}

/// Mark the env as onboarded so interactive `claude` skips its first-run
/// wizard (theme/login) and goes straight in — auth is handled by the shared
/// token. Merges `hasCompletedOnboarding: true` into `.claude.json`.
pub fn mark_onboarded(env_dir: &Path) -> Result<()> {
    let path = env_dir.join(".claude.json");
    let mut v: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert("hasCompletedOnboarding".into(), serde_json::Value::Bool(true));
    }
    std::fs::write(&path, serde_json::to_string_pretty(&v)?)
        .context("could not write .claude.json")
}

#[allow(dead_code)] // used in later phases (edit/sessions)
pub fn load_instance(env_dir: &Path) -> Option<Instance> {
    let text = std::fs::read_to_string(env_dir.join(".aello.toml")).ok()?;
    toml::from_str(&text).ok()
}

/// Place an instance into its env dir: write `.aello.toml`, and seed
/// `settings.json`, CLAUDE.md, and the PostCompact hook if absent. Then, from
/// `caps`, regenerate the `/sync` skill and scaffold the project files this
/// blueprint maintains.
pub fn place(
    env_dir: &Path,
    inst: &Instance,
    claude_md: Option<&str>,
    caps: &Capabilities,
) -> Result<()> {
    std::fs::create_dir_all(env_dir).context("could not create env dir")?;

    std::fs::write(env_dir.join(".aello.toml"), toml::to_string_pretty(inst)?)
        .context("could not write .aello.toml")?;

    let settings = env_dir.join("settings.json");
    if !settings.exists() {
        std::fs::write(&settings, settings_json(&inst.model))
            .context("could not write settings.json")?;
    }

    // Global persona — set once, never clobbered (the user may have edited it).
    if let Some(content) = claude_md {
        let path = env_dir.join("CLAUDE.md");
        if !path.exists() {
            std::fs::write(&path, content).context("could not write CLAUDE.md")?;
        }
    }

    // Always refresh the hook script so updates (e.g. AELLO_CONTEXTDB support)
    // propagate to existing envs on the next run.
    std::fs::create_dir_all(env_dir.join("hooks")).context("could not create hooks dir")?;
    std::fs::write(env_dir.join("hooks").join("post-compact.py"), POST_COMPACT_SCRIPT)
        .context("could not write post-compact.py")?;

    // Regenerate the tailored /sync skill from current caps (or remove it if the
    // blueprint no longer maintains anything).
    let skill = env_dir.join("skills").join("sync").join("SKILL.md");
    if caps.any() {
        std::fs::create_dir_all(skill.parent().unwrap())
            .context("could not create skills dir")?;
        std::fs::write(&skill, crate::templates::render_sync_skill(caps, &inst.name))
            .context("could not write sync SKILL.md")?;
    } else if skill.exists() {
        let _ = std::fs::remove_file(&skill);
    }

    // Scaffold the project-dir files this blueprint maintains (only if missing).
    let project = env_dir.parent().unwrap_or(env_dir);
    scaffold_project(project, caps)?;

    Ok(())
}

/// Create the docs the enabled capabilities expect, only when absent — so a
/// fresh project gets its CHANGELOG/README/docs/CLAUDE.md, and existing files
/// are left untouched. GitHub has no file to scaffold (the repo/remote is
/// handled interactively by `/sync`).
fn scaffold_project(project: &Path, caps: &Capabilities) -> Result<()> {
    let name = project
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    if caps.changelog {
        let p = project.join("CHANGELOG.md");
        if !p.exists() {
            std::fs::write(&p, "# Changelog\n\n## [Unreleased]\n")
                .context("could not write CHANGELOG.md")?;
        }
    }
    if caps.readme {
        let p = project.join("README.md");
        if !p.exists() {
            std::fs::write(&p, format!("# {name}\n")).context("could not write README.md")?;
        }
    }
    if caps.docs {
        std::fs::create_dir_all(project.join("docs")).context("could not create docs dir")?;
    }
    if caps.project_md {
        let p = project.join("CLAUDE.md");
        if !p.exists() {
            std::fs::write(&p, format!("# {name}\n\nProject-specific instructions for Claude.\n"))
                .context("could not write project CLAUDE.md")?;
        }
    }
    if caps.github {
        // Keep env dirs (and the credentials inside them) out of the repo.
        ensure_gitignore_entry(project, ".claude-env-*")?;
        // Normalize line endings so multi-OS blueprints sharing a repo don't
        // churn CRLF/LF on every commit.
        let ga = project.join(".gitattributes");
        if !ga.exists() {
            std::fs::write(&ga, "* text=auto\n").context("could not write .gitattributes")?;
        }
        // Seed a stack-agnostic VERSION + patch-bump CI workflow for the target
        // project (mirrors aello's own release machinery, build-system agnostic).
        let ver = project.join("VERSION");
        if !ver.exists() {
            std::fs::write(&ver, "0.1.0\n").context("could not write VERSION")?;
        }
        let wf = project.join(".github").join("workflows").join("version.yml");
        if !wf.exists() {
            std::fs::create_dir_all(wf.parent().unwrap())
                .context("could not create .github/workflows dir")?;
            std::fs::write(&wf, VERSION_WORKFLOW).context("could not write version.yml")?;
        }
    }
    Ok(())
}

/// Ensure `entry` exists as its own line in the project's `.gitignore`, creating
/// the file or appending as needed. Idempotent — a matching line (ignoring
/// surrounding whitespace) is never duplicated. Preserves existing content.
fn ensure_gitignore_entry(project: &Path, entry: &str) -> Result<()> {
    let path = project.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == entry) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(entry);
    out.push('\n');
    std::fs::write(&path, out).context("could not write .gitignore")
}

/// settings.json for an isolated Claude env: subscription auth (no keys, no env
/// block), bypass permissions, and the single PostCompact transcript hook.
pub fn settings_json(model: &str) -> String {
    let py = if cfg!(windows) { "python" } else { "python3" };
    format!(
        r#"{{
  "model": {},
  "skipDangerousModePermissionPrompt": true,
  "permissions": {{
    "defaultMode": "bypassPermissions"
  }},
  "hooks": {{
    "PostCompact": [{{"hooks":[{{"type":"command","command":"{} \"$CLAUDE_CONFIG_DIR/hooks/post-compact.py\""}}]}}]
  }}
}}
"#,
        json_str(model),
        py
    )
}

/// Minimal JSON string encoder for the model value.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_dir_naming() {
        let p = env_dir(Path::new("/proj"), "coder");
        assert!(p.ends_with(".claude-env-coder"));
    }

    #[test]
    fn settings_json_is_valid() {
        let s = settings_json("sonnet");
        let v: serde_json::Value = serde_json::from_str(&s).expect("valid JSON");
        assert_eq!(v["model"], "sonnet");
        assert_eq!(v["permissions"]["defaultMode"], "bypassPermissions");
        assert!(v["hooks"]["PostCompact"].is_array());
    }

    #[test]
    fn place_seeds_sync_and_scaffolds_selected_files() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into() };
        let caps = Capabilities { changelog: true, github: true, ..Default::default() };

        place(&env, &inst, Some("# persona"), &caps).unwrap();

        // /sync skill seeded inside the env, reflecting caps.
        let skill = std::fs::read_to_string(env.join("skills/sync/SKILL.md")).unwrap();
        assert!(skill.contains("Commit + push"));
        assert!(skill.contains("CHANGELOG.md"));
        assert!(!skill.contains("README.md"));

        // Scaffolds land in the PROJECT dir, only for enabled caps.
        assert!(proj.path().join("CHANGELOG.md").exists());
        assert!(!proj.path().join("README.md").exists()); // readme not selected
        assert!(!proj.path().join("docs").exists()); // docs not selected
        assert!(env.join("CLAUDE.md").exists()); // global persona in the env
    }

    #[test]
    fn github_cap_gitignores_env_dirs_idempotently() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into() };
        let caps = Capabilities { github: true, ..Default::default() };
        let gi = proj.path().join(".gitignore");

        // Pre-existing .gitignore with unrelated content, no trailing newline.
        std::fs::write(&gi, "target/\n*.log").unwrap();

        // First placement appends the entry, preserving existing lines.
        place(&env, &inst, None, &caps).unwrap();
        let after_first = std::fs::read_to_string(&gi).unwrap();
        assert!(after_first.contains("target/"));
        assert!(after_first.contains("*.log"));
        assert_eq!(after_first.matches(".claude-env-*").count(), 1);

        // Second placement must NOT duplicate the entry.
        place(&env, &inst, None, &caps).unwrap();
        let after_second = std::fs::read_to_string(&gi).unwrap();
        assert_eq!(after_second.matches(".claude-env-*").count(), 1);
    }

    #[test]
    fn github_cap_scaffolds_release_hygiene() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into() };
        let caps = Capabilities { github: true, ..Default::default() };

        place(&env, &inst, None, &caps).unwrap();

        let ga = std::fs::read_to_string(proj.path().join(".gitattributes")).unwrap();
        assert!(ga.contains("text=auto"));
        let ver = std::fs::read_to_string(proj.path().join("VERSION")).unwrap();
        assert_eq!(ver.trim(), "0.1.0");
        let wf = std::fs::read_to_string(
            proj.path().join(".github/workflows/version.yml"),
        )
        .unwrap();
        assert!(wf.contains("bump patch in VERSION"));
        assert!(wf.contains("[skip ci]"));

        // A no-github blueprint seeds none of these in a fresh project.
        let fresh = tempfile::tempdir().unwrap();
        let fenv = env_dir(fresh.path(), "bare");
        place(&fenv, &Instance { name: "bare".into(), model: "haiku".into() }, None,
              &Capabilities { changelog: true, ..Default::default() }).unwrap();
        assert!(!fresh.path().join(".gitattributes").exists());
        assert!(!fresh.path().join("VERSION").exists());
        assert!(!fresh.path().join(".github").exists());
    }

    #[test]
    fn no_github_cap_writes_no_gitignore() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "bare");
        let inst = Instance { name: "bare".into(), model: "haiku".into() };
        let caps = Capabilities { changelog: true, ..Default::default() };

        place(&env, &inst, None, &caps).unwrap();
        assert!(!proj.path().join(".gitignore").exists());
    }

    #[test]
    fn place_without_caps_seeds_no_sync_skill() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "bare");
        let inst = Instance { name: "bare".into(), model: "sonnet".into() };

        place(&env, &inst, None, &Capabilities::default()).unwrap();

        assert!(!env.join("skills/sync/SKILL.md").exists());
        assert!(!proj.path().join("CHANGELOG.md").exists());
    }
}
