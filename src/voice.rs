//! `aello voice` — the off switch for the `voice` capability.
//!
//! The TTS hook keeps its state (voice pool, per-session leases, mute flags) in
//! one machine-wide data dir shared by every env, not inside any project. That's
//! what lets a mute here apply to all of them at once, and it's why this works
//! from any directory — including one with no env placed in it, which is exactly
//! where you are when a machine you didn't expect to talk starts talking.
//!
//! aello writes that file directly rather than shelling out to a copy of the
//! hook: no Python on PATH and no placed env are needed to shut it up. The
//! contract is small — `global` (bool), `projects` (map of path → bool), and a
//! `run/stop` token whose value changing tells a speaking worker to stand down.
//! Unknown keys are preserved, so the hook owns everything else.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where the hook keeps its shared state. Mirrors `speak.py`'s `data_dir()`:
/// per-OS application data, not the config dir aello uses for itself.
fn data_dir() -> Result<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| crate::config::home_dir().map(|h| h.join("AppData").join("Local")))
    } else if cfg!(target_os = "macos") {
        crate::config::home_dir().map(|h| h.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| crate::config::home_dir().map(|h| h.join(".local").join("share")))
    };
    Ok(base.context("could not determine the data directory")?.join("revoiced"))
}

fn read_state(dir: &Path) -> serde_json::Value {
    std::fs::read_to_string(dir.join("state.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Write state back the way the hook does — to a temp file, then rename — so a
/// worker reading concurrently never sees a half-written file.
///
/// The temp name carries this process id. Both languages used to stage through
/// the same fixed `state.tmp`, so aello and a `speak.py` worker writing at once
/// could interleave into one file and rename the result into place — and both
/// readers fall back to empty defaults on a corrupt state, losing the pool.
fn write_state(dir: &Path, v: &serde_json::Value) -> Result<()> {
    std::fs::create_dir_all(dir).context("could not create the voice state dir")?;
    let tmp = dir.join(format!("state.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(v)?)
        .context("could not write the voice state")?;
    std::fs::rename(&tmp, dir.join("state.json")).context("could not replace the voice state")
}

/// Interrupt whatever is speaking right now. Workers poll this token while the
/// player runs and terminate it when the value changes; any new value will do.
fn bump_stop_token(dir: &Path) -> Result<()> {
    let run = dir.join("run");
    std::fs::create_dir_all(&run).context("could not create the voice run dir")?;
    let token = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "stop".into());
    std::fs::write(run.join("stop"), token).context("could not write the stop token")
}

/// The hook keys muted projects by resolved path, so match its normalisation or
/// a mute set here won't be seen there.
fn current_project() -> Result<String> {
    let cwd = std::env::current_dir().context("could not read the current directory")?;
    let resolved = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    // canonicalize() yields a \\?\ prefix on Windows; the hook stores a plain path.
    let s = resolved.to_string_lossy().to_string();
    Ok(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

pub fn mute(project_only: bool) -> Result<()> {
    let dir = data_dir()?;
    let mut state = read_state(&dir);
    let obj = state.as_object_mut().context("voice state is not an object")?;
    if project_only {
        let target = current_project()?;
        obj.entry("projects")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("voice state 'projects' is not an object")?
            .insert(target.clone(), serde_json::Value::Bool(true));
        write_state(&dir, &state)?;
        println!("muted: {target}");
        // Deliberately no stop token here. It is machine-wide, so bumping it for
        // a per-project mute cut off whatever another project was saying — and
        // dropped every other project's queued line too, since workers re-check
        // the token after taking the speaker lock. speak.py has always scoped it
        // this way; only a global mute silences what is already playing.
        return Ok(());
    }
    obj.insert("global".into(), serde_json::Value::Bool(true));
    write_state(&dir, &state)?;
    println!("muted (all projects)");
    // Muting should also stop the sentence already playing, not just the next.
    bump_stop_token(&dir)
}

pub fn unmute(project_only: bool) -> Result<()> {
    let dir = data_dir()?;
    let mut state = read_state(&dir);
    let obj = state.as_object_mut().context("voice state is not an object")?;
    if project_only {
        let target = current_project()?;
        if let Some(p) = obj.get_mut("projects").and_then(|p| p.as_object_mut()) {
            p.remove(&target);
        }
        write_state(&dir, &state)?;
        println!("unmuted: {target}");
    } else {
        obj.insert("global".into(), serde_json::Value::Bool(false));
        write_state(&dir, &state)?;
        println!("unmuted (all projects)");
    }
    Ok(())
}

/// Read the machine-wide mute flag. The TUI shows this in its footer, so it has
/// to be a plain query — no printing, and no error when the hook has never run
/// and the state file doesn't exist yet.
pub fn is_globally_muted() -> bool {
    data_dir()
        .map(|dir| read_state(&dir).get("global").and_then(|g| g.as_bool()).unwrap_or(false))
        .unwrap_or(false)
}

/// Flip the machine-wide mute, returning the new state. Muting also cuts off the
/// sentence already playing, exactly as `aello voice mute` does. Print-free: the
/// TUI owns the alternate screen, so a stray println would corrupt the display.
pub fn toggle_global_mute() -> Result<bool> {
    let dir = data_dir()?;
    let mut state = read_state(&dir);
    let obj = state.as_object_mut().context("voice state is not an object")?;
    let muted = !obj.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
    obj.insert("global".into(), serde_json::Value::Bool(muted));
    write_state(&dir, &state)?;
    if muted {
        bump_stop_token(&dir)?;
    }
    Ok(muted)
}

/// Stop the current utterance without changing any mute setting.
pub fn stop() -> Result<()> {
    bump_stop_token(&data_dir()?)?;
    println!("stopped");
    Ok(())
}

pub fn status() -> Result<()> {
    let dir = data_dir()?;
    let state = read_state(&dir);
    let muted = state.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
    let projects: Vec<&str> = state
        .get("projects")
        .and_then(|p| p.as_object())
        .map(|p| {
            p.iter()
                .filter(|(_, v)| v.as_bool().unwrap_or(false))
                .map(|(k, _)| k.as_str())
                .collect()
        })
        .unwrap_or_default();
    let presets = state.get("presets").and_then(|p| p.as_array()).map_or(0, |a| a.len());
    let leases = state.get("leases").and_then(|l| l.as_object()).map_or(0, |o| o.len());

    println!("state         : {}", dir.join("state.json").display());
    println!("global mute   : {muted}");
    println!(
        "muted projects: {}",
        if projects.is_empty() { "none".to_string() } else { projects.join(", ") }
    );
    println!("voice pool    : {presets} preset(s), {leases} leased");
    if presets == 0 {
        println!("                (empty pool — the hook uses its built-in default voice)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_flag_round_trips_and_preserves_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        // State as the hook writes it: keys aello knows nothing about must survive.
        write_state(
            dir.path(),
            &serde_json::json!({
                "global": false,
                "presets": [{"id": "a"}],
                "leases": {"s1": {"preset": "a"}},
                "duck": 15
            }),
        )
        .unwrap();

        let mut state = read_state(dir.path());
        state.as_object_mut().unwrap().insert("global".into(), serde_json::Value::Bool(true));
        write_state(dir.path(), &state).unwrap();

        let back = read_state(dir.path());
        assert_eq!(back["global"], serde_json::json!(true));
        assert_eq!(back["duck"], serde_json::json!(15));
        assert_eq!(back["presets"].as_array().unwrap().len(), 1);
        assert!(back["leases"].get("s1").is_some());
    }

    #[test]
    fn stop_token_changes_each_time() {
        let dir = tempfile::tempdir().unwrap();
        bump_stop_token(dir.path()).unwrap();
        let first = std::fs::read_to_string(dir.path().join("run").join("stop")).unwrap();
        // A worker only compares the value, so it just has to differ.
        std::thread::sleep(std::time::Duration::from_millis(2));
        bump_stop_token(dir.path()).unwrap();
        let second = std::fs::read_to_string(dir.path().join("run").join("stop")).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn toggling_the_mute_flips_it_and_leaves_the_hook_s_keys_alone() {
        let dir = tempfile::tempdir().unwrap();
        write_state(
            dir.path(),
            &serde_json::json!({"global": false, "presets": [{"id": "a"}], "duck": 15}),
        )
        .unwrap();

        // The same logic the TUI's M key runs, against a temp dir rather than
        // the real machine-wide one.
        let flip = |dir: &Path| {
            let mut state = read_state(dir);
            let obj = state.as_object_mut().unwrap();
            let muted = !obj.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
            obj.insert("global".into(), serde_json::Value::Bool(muted));
            write_state(dir, &state).unwrap();
            muted
        };

        assert!(flip(dir.path()), "first press mutes");
        assert_eq!(read_state(dir.path())["global"], serde_json::json!(true));
        assert!(!flip(dir.path()), "second press unmutes");

        let back = read_state(dir.path());
        assert_eq!(back["global"], serde_json::json!(false));
        assert_eq!(back["duck"], serde_json::json!(15));
        assert_eq!(back["presets"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn missing_state_file_reads_as_empty_object() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_state(dir.path()), serde_json::json!({}));
    }
}
