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

/// Expand a leading `~` to the home directory; otherwise pass through. Splits
/// the remainder on both separators so the result uses native components (no
/// mixed `C:\Users\H\aello/contextdb`).
fn expand_home(p: &str) -> PathBuf {
    if p == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(p));
    }
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(h) = home_dir() {
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

/// Load config, returning an empty default only if the file does not exist yet.
///
/// A missing file is the normal first-run case → empty default. But every other
/// I/O error (a transient share-violation from OneDrive / an AV scanner / the
/// Windows Search Indexer holding a lock, a permission glitch, etc.) must
/// **propagate**, not collapse to a default: callers are `load → mutate → save`
/// sandwiches, so returning an empty default here would make the next `save()`
/// overwrite `config.toml` — destroying every blueprint AND the non-rotating
/// OAuth token — over a momentary read failure.
pub fn load() -> Result<Config> {
    load_path(&config_path()?)
}

fn load_path(path: &std::path::Path) -> Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let mut cfg: Config = toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            // Pre-0.2 configs carry five capability booleans per blueprint; fold
            // them into a role here so nothing downstream ever sees the old shape.
            cfg.migrate_roles();
            cfg.migrate_personas();
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("could not create config dir")?;
        // config.toml holds the plaintext, non-rotating OAuth token, so keep the
        // dir owner-only on Unix (best effort; no-op on Windows).
        restrict_dir(parent);
    }
    let text = toml::to_string_pretty(cfg).context("failed to serialize config")?;
    // Atomic write: config.toml holds the only copy of the non-rotating OAuth
    // token, so a crash mid-write must not truncate it. Write a sibling temp
    // file, then rename over the target (atomic on the same filesystem).
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("could not write {}", tmp.display()))?;
    // Lock the token down to owner-only BEFORE the rename, so the file is never
    // briefly world-readable at its final path (no-op on Windows).
    restrict_file(&tmp);
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("could not replace {}", path.display()));
    }
    Ok(())
}

/// Restrict a file to owner read/write (`0600`) on Unix; no-op elsewhere.
/// Best-effort: a permission-set failure must not block saving the config.
#[cfg(unix)]
fn restrict_file(p: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
}

/// Restrict a directory to owner-only (`0700`) on Unix; no-op elsewhere.
#[cfg(unix)]
fn restrict_dir(p: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_file(_p: &std::path::Path) {}
#[cfg(not(unix))]
fn restrict_dir(_p: &std::path::Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty_default() {
        let dir = std::env::temp_dir().join(format!("aello-cfg-load-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let cfg = load_path(&dir.join("does-not-exist.toml")).expect("NotFound must default");
        assert!(cfg.blueprints.is_empty());
        assert!(cfg.oauth_token.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn restrict_file_sets_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("aello-cfg-perm-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("config.toml.tmp");
        std::fs::write(&f, "x").unwrap();
        restrict_file(&f);
        let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn non_notfound_error_propagates() {
        // A path that exists but is a directory yields a non-NotFound read
        // error; load() must NOT collapse it to an empty default (doing so would
        // let the next save() overwrite a real config + token).
        let dir = std::env::temp_dir().join(format!("aello-cfg-err-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        assert!(load_path(&dir).is_err());
    }
}
