//! Reading Claude Code session transcripts for a placed env.
//!
//! Claude stores per-project session `.jsonl` files under
//! `<env_dir>/projects/<encoded-project-path>/`. The file stem is the session
//! id you pass to `claude --resume`.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Session {
    pub id: String,
    pub modified: SystemTime,
    pub size: u64,
}

/// Claude's project-dir naming: replace each `\`, `/`, `:` with `-`.
pub fn encode_project_path(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if matches!(c, '\\' | '/' | ':') { '-' } else { c })
        .collect()
}

/// Sessions for this env in this project, newest first.
pub fn list(env_dir: &Path, project: &Path) -> Vec<Session> {
    let dir = env_dir.join("projects").join(encode_project_path(project));
    let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
    let mut out: Vec<Session> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.is_dir() || p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                return None;
            }
            let id = p.file_stem()?.to_string_lossy().into_owned();
            let meta = std::fs::metadata(&p).ok()?;
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            Some(Session { id, modified, size: meta.len() })
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

/// Format a SystemTime as "YYYY-MM-DD HH:MM" (UTC), no external deps.
pub fn format_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;

    // Gregorian calendar from days-since-epoch (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };

    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}")
}
