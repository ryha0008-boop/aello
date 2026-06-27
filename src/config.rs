use crate::models::Config;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Config directory. Honours AELLO_CONFIG_DIR for tests/overrides, else the
/// per-OS application config dir via the `directories` crate.
pub fn config_dir() -> Result<PathBuf> {
    if let Ok(d) = std::env::var("AELLO_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    let pd = directories::ProjectDirs::from("", "", "aello")
        .context("could not determine config directory")?;
    Ok(pd.config_dir().to_path_buf())
}

/// Default contextdb path when unset.
pub const DEFAULT_CONTEXTDB: &str = "~/aello/contextdb";

/// Resolve the unified contextdb path (config value or default), expanding `~`.
pub fn contextdb_dir(cfg: &Config) -> PathBuf {
    let raw = cfg.contextdb.as_deref().unwrap_or(DEFAULT_CONTEXTDB);
    expand_home(raw)
}

pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

fn home() -> Option<PathBuf> {
    home_dir()
}

/// Expand a leading `~` to the home directory; otherwise pass through. Splits
/// the remainder on both separators so the result uses native components (no
/// mixed `C:\Users\H\aello/contextdb`).
fn expand_home(p: &str) -> PathBuf {
    if p == "~" {
        return home().unwrap_or_else(|| PathBuf::from(p));
    }
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(h) = home() {
            let mut path = h;
            for comp in rest.split(['/', '\\']).filter(|c| !c.is_empty()) {
                path.push(comp);
            }
            return path;
        }
    }
    PathBuf::from(p)
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

/// Load config, returning an empty default if the file does not exist yet.
pub fn load() -> Result<Config> {
    let path = config_path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
        }
        Err(_) => Ok(Config::default()),
    }
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("could not create config dir")?;
    }
    let text = toml::to_string_pretty(cfg).context("failed to serialize config")?;
    // Atomic write: config.toml holds the only copy of the non-rotating OAuth
    // token, so a crash mid-write must not truncate it. Write a sibling temp
    // file, then rename over the target (atomic on the same filesystem).
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("could not write {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("could not replace {}", path.display()));
    }
    Ok(())
}
