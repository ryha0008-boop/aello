//! Placing a blueprint into a project: the env dir, its `.aello.toml`,
//! `settings.json`, optional CLAUDE.md, and the PostCompact hook script.

use crate::models::Instance;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const POST_COMPACT_SCRIPT: &str = include_str!("hooks_post_compact.py");

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
/// `settings.json`, CLAUDE.md, and the PostCompact hook if absent.
pub fn place(env_dir: &Path, inst: &Instance, claude_md: Option<&str>) -> Result<()> {
    std::fs::create_dir_all(env_dir).context("could not create env dir")?;

    std::fs::write(env_dir.join(".aello.toml"), toml::to_string_pretty(inst)?)
        .context("could not write .aello.toml")?;

    let settings = env_dir.join("settings.json");
    if !settings.exists() {
        std::fs::write(&settings, settings_json(&inst.model))
            .context("could not write settings.json")?;
    }

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

    Ok(())
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
}
